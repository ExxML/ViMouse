#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unexpected_cfgs)] // objc 0.2 internally uses #[cfg(cargo-clippy)], which triggers unexpected_cfgs

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

#[cfg(target_os = "macos")]
mod caps_lock_remap;
#[cfg(not(target_os = "macos"))]
mod caps_lock_suppress;
mod config;
mod cursor_visibility;
mod input;
mod monitor;
mod overlay;
mod platform_input;
mod state;

use crate::input::{spawn_input_hook, spawn_motion_loop};
use crate::monitor::collect_monitors;
use crate::overlay::create_topmost_anchor;
#[cfg(target_os = "windows")]
use crate::overlay::create_window_overlay_owner_hwnd;
use crate::overlay::{
    create_event_loop, create_window, create_window_overlay, hide_window_overlay, key_label,
    show_mode_overlay_window, update_mode_overlay, GridOverlayState, GridSurface, MarkGlyph,
    MarkOverlayState, MarkSurface, ModeOverlayState, ModeSurface,
};
use crate::platform_input::{mouse_button_is_down, shutdown_platform_input};
use crate::state::{Action, SharedState};
use crate::state::{Mode, MonitorInfo};
use fs2::FileExt;
use rdev::Button;
use std::sync::{Arc, Condvar, Mutex};
use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct UISnapshot {
    selected_monitor: usize,
    mode_state: ModeOverlayState,
    grid_state: GridOverlayState,
    letters_state: GridOverlayState,
    mark_state: MarkOverlayState,
}

// Build the mark glyph list from the marks map: each set mark becomes its key's label glyph
// at the mark's virtual-desktop position. Sorted by label so the list compares stably for
// change detection (HashMap iteration order is nondeterministic).
fn mark_glyphs(
    marks: &std::collections::HashMap<rdev::Key, crate::state::Point>,
) -> Vec<MarkGlyph> {
    let mut glyphs: Vec<MarkGlyph> = marks
        .iter()
        .filter_map(|(key, position)| {
            key_label(*key).map(|label| MarkGlyph {
                label,
                position: *position,
            })
        })
        .collect();
    glyphs.sort_unstable_by_key(|g| g.label);
    glyphs
}

fn current_ui_snapshot(shared: &Arc<Mutex<SharedState>>) -> UISnapshot {
    let state = shared.lock().expect("shared state poisoned");
    let monitor = state
        .monitors
        .get(state.selected_monitor)
        .copied()
        .expect("selected monitor out of bounds");
    UISnapshot {
        selected_monitor: state.selected_monitor,
        mode_state: ModeOverlayState {
            visible: state.show_overlays && state.show_mode_line,
            mode: state.mode,
            monitor,
        },
        grid_state: GridOverlayState {
            visible: state.show_overlays && state.show_grid && state.mode == Mode::Normal,
            show_letters: false,
            monitor,
        },
        letters_state: GridOverlayState {
            visible: state.show_overlays && state.show_grid_letters && state.mode == Mode::Normal,
            show_letters: true,
            monitor,
        },
        mark_state: MarkOverlayState {
            visible: state.show_overlays && state.mode == Mode::Normal,
            marks: mark_glyphs(&state.marks),
        },
    }
}

struct ModeSlot {
    window: Window,
    surface: ModeSurface,
    monitor: MonitorInfo,
}

struct GridSlot {
    window: Window,
    surface: GridSurface,
    monitor: MonitorInfo,
    show_letters: bool,
}

struct MarkSlot {
    window: Window,
    surface: MarkSurface,
    monitor: MonitorInfo,
}

fn create_mode_slots(
    event_loop: &EventLoop<()>,
    first_window: Window,
    monitors: &[MonitorInfo],
    mode: Mode,
) -> Result<Vec<ModeSlot>, String> {
    let mut windows = Vec::with_capacity(monitors.len());
    windows.push(first_window);
    for _ in 1..monitors.len() {
        windows.push(create_window(event_loop));
    }

    let mut slots = Vec::with_capacity(monitors.len());
    for (window, monitor) in windows.into_iter().zip(monitors.iter().copied()) {
        let mut surface = ModeSurface::new(&window);
        let overlay = ModeOverlayState {
            visible: false,
            mode,
            monitor,
        };
        update_mode_overlay(&window, &mut surface, &overlay)?;
        slots.push(ModeSlot {
            window,
            surface,
            monitor,
        });
    }

    Ok(slots)
}

fn create_grid_slots(
    event_loop: &EventLoop<()>,
    first_window: Window,
    monitors: &[MonitorInfo],
    show_letters: bool,
    #[cfg(target_os = "windows")] owner: windows_sys::Win32::Foundation::HWND,
) -> Vec<GridSlot> {
    let mut windows = Vec::with_capacity(monitors.len());
    windows.push(first_window);
    for _ in 1..monitors.len() {
        #[cfg(target_os = "windows")]
        windows.push(create_window_overlay(event_loop, owner));
        #[cfg(not(target_os = "windows"))]
        windows.push(create_window_overlay(event_loop));
    }

    let mut slots = Vec::with_capacity(monitors.len());
    for (window, monitor) in windows.into_iter().zip(monitors.iter().copied()) {
        let surface = GridSurface::new(&window, &monitor);
        slots.push(GridSlot {
            window,
            surface,
            monitor,
            show_letters,
        });
    }

    slots
}

fn create_mark_slots(
    event_loop: &EventLoop<()>,
    first_window: Window,
    monitors: &[MonitorInfo],
    #[cfg(target_os = "windows")] owner: windows_sys::Win32::Foundation::HWND,
) -> Vec<MarkSlot> {
    let mut windows = Vec::with_capacity(monitors.len());
    windows.push(first_window);
    for _ in 1..monitors.len() {
        #[cfg(target_os = "windows")]
        windows.push(create_window_overlay(event_loop, owner));
        #[cfg(not(target_os = "windows"))]
        windows.push(create_window_overlay(event_loop));
    }

    windows
        .into_iter()
        .zip(monitors.iter().copied())
        .map(|(window, monitor)| MarkSlot {
            window,
            surface: MarkSurface::new(),
            monitor,
        })
        .collect()
}

fn find_mode_slot(slots: &[ModeSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn find_grid_slot(slots: &[GridSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn find_mark_slot(slots: &[MarkSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

// Renders the slot with `state`'s mode, but on the slot's own monitor and with `visible`
// resolved by the caller (only the focused monitor's slot is ever shown).
fn update_mode_slot_or_exit(
    slot: &mut ModeSlot,
    state: &ModeOverlayState,
    visible: bool,
    control_flow: &mut ControlFlow,
) {
    let overlay = ModeOverlayState {
        visible,
        mode: state.mode,
        monitor: slot.monitor,
    };
    if let Err(error) = update_mode_overlay(&slot.window, &mut slot.surface, &overlay) {
        eprintln!("mode overlay render error: {error}");
        shutdown_platform_input();
        *control_flow = ControlFlow::Exit;
    }
}

fn update_grid_slot(slot: &mut GridSlot, visible: bool) {
    slot.surface.update(
        &slot.window,
        &GridOverlayState {
            visible,
            show_letters: slot.show_letters,
            monitor: slot.monitor,
        },
    );
}

// Marks are global, so a slot shows whenever overlays are visible AND it owns a mark
// (keeps mark-free monitors from flashing empty window overlays).
fn update_mark_slot(slot: &mut MarkSlot, visible: bool, marks: &[MarkGlyph]) {
    let has_marks = marks.iter().any(|m| slot.monitor.contains(m.position));
    slot.surface.update(
        &slot.window,
        &slot.monitor,
        &MarkOverlayState {
            visible: visible && has_marks,
            marks: marks.to_vec(),
        },
    );
}

fn main() {
    let lock_path = std::env::temp_dir().join("vimouse.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .expect("failed to open lock file");
    if lock_file.try_lock_exclusive().is_err() {
        return;
    }

    #[cfg(target_os = "macos")]
    if !crate::platform_input::macos_grab::is_accessibility_trusted(true) {
        eprintln!("Accessibility permission required. Grant access in System Settings → Privacy & Security → Accessibility, then relaunch.");
        std::process::exit(1);
    }

    #[cfg(not(target_os = "macos"))]
    {
        crate::caps_lock_suppress::suppress();
    }

    ctrlc::set_handler(|| {
        shutdown_platform_input();
        std::process::exit(0);
    })
    .expect("failed to set Ctrl+C handler");

    // Restore any global state we mutate (hidden cursor, macOS caps-lock remap) on panic, so an
    // unwinding crash never leaves the user without a cursor. Hard kills can't be caught here, but
    // the cursor hide is connection-scoped on macOS/Linux and the OS restores it on process death.
    {
        let default_panic = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::cursor_visibility::set_cursor_hidden(false);
            #[cfg(target_os = "macos")]
            crate::caps_lock_remap::shutdown();
            default_panic(info);
        }));
    }

    let event_loop = create_event_loop();
    let bootstrap_window = create_window(&event_loop);
    #[cfg(target_os = "windows")]
    let grid_owner = create_window_overlay_owner_hwnd();
    #[cfg(target_os = "windows")]
    let bootstrap_grid_window = create_window_overlay(&event_loop, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let bootstrap_grid_window = create_window_overlay(&event_loop);
    #[cfg(target_os = "windows")]
    let bootstrap_letters_window = create_window_overlay(&event_loop, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let bootstrap_letters_window = create_window_overlay(&event_loop);
    #[cfg(target_os = "windows")]
    let bootstrap_mark_window = create_window_overlay(&event_loop, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let bootstrap_mark_window = create_window_overlay(&event_loop);

    let monitors = collect_monitors(&bootstrap_window);
    let primary_monitor = monitors.first().copied().expect("no monitors available");
    let initial_cursor = primary_monitor.center();

    // Keeps ViMouse's overlays above every other window for the whole session. Held here so it
    // lives for the process lifetime; dropping it destroys the anchor (except on Windows)
    let _topmost_anchor = create_topmost_anchor(&event_loop, &primary_monitor);

    let mut state = SharedState::new(initial_cursor, 0, monitors);
    if mouse_button_is_down(Button::Left) {
        state
            .pending_actions
            .push(Action::ButtonRelease(Button::Left));
    }
    if mouse_button_is_down(Button::Right) {
        state
            .pending_actions
            .push(Action::ButtonRelease(Button::Right));
    }
    state
        .pending_actions
        .push(Action::MouseMove(initial_cursor));

    let shared = Arc::new(Mutex::new(state));
    let motion_waker = Arc::new(Condvar::new());
    // Proxy lets the input hook thread wake the winit event loop without polling.
    let ui_waker = event_loop.create_proxy();

    spawn_input_hook(
        Arc::clone(&shared),
        Arc::clone(&motion_waker),
        ui_waker.clone(),
    );
    spawn_motion_loop(Arc::clone(&shared), motion_waker, ui_waker);

    let (
        mut last_mode_state,
        mut last_grid_state,
        mut last_letters_state,
        mut last_mark_state,
        monitors,
    ) = {
        let state = shared.lock().expect("shared state poisoned");
        let monitor = state
            .monitors
            .get(state.selected_monitor)
            .copied()
            .expect("selected monitor out of bounds");
        (
            ModeOverlayState {
                visible: state.show_mode_line,
                mode: state.mode,
                monitor,
            },
            GridOverlayState {
                visible: state.show_grid && state.mode == Mode::Normal,
                show_letters: false,
                monitor,
            },
            GridOverlayState {
                visible: state.show_grid_letters && state.mode == Mode::Normal,
                show_letters: true,
                monitor,
            },
            MarkOverlayState {
                visible: state.mode == Mode::Normal,
                marks: mark_glyphs(&state.marks),
            },
            state.monitors.clone(),
        )
    };

    let mut mode_slots = match create_mode_slots(
        &event_loop,
        bootstrap_window,
        &monitors,
        last_mode_state.mode,
    ) {
        Ok(slots) => slots,
        Err(error) => {
            eprintln!("initial mode overlay render error: {error}");
            shutdown_platform_input();
            return;
        }
    };

    #[cfg(target_os = "windows")]
    let mut grid_slots = create_grid_slots(
        &event_loop,
        bootstrap_grid_window,
        &monitors,
        false,
        grid_owner,
    );
    #[cfg(not(target_os = "windows"))]
    let mut grid_slots = create_grid_slots(&event_loop, bootstrap_grid_window, &monitors, false);
    #[cfg(target_os = "windows")]
    let mut letters_slots = create_grid_slots(
        &event_loop,
        bootstrap_letters_window,
        &monitors,
        true,
        grid_owner,
    );
    #[cfg(not(target_os = "windows"))]
    let mut letters_slots =
        create_grid_slots(&event_loop, bootstrap_letters_window, &monitors, true);
    #[cfg(target_os = "windows")]
    let mut mark_slots =
        create_mark_slots(&event_loop, bootstrap_mark_window, &monitors, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let mut mark_slots = create_mark_slots(&event_loop, bootstrap_mark_window, &monitors);

    let mut last_selected_monitor = {
        let snap = current_ui_snapshot(&shared);
        snap.selected_monitor
    };
    if last_mode_state.visible {
        show_mode_overlay_window(&mode_slots[last_selected_monitor].window);
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            WinitEvent::MainEventsCleared | WinitEvent::UserEvent(()) => {
                let snap = current_ui_snapshot(&shared);
                let selected_monitor = snap.selected_monitor;

                let mode_state = snap.mode_state;
                if last_mode_state != mode_state {
                    let visibility_changed = last_mode_state.visible != mode_state.visible;
                    let monitor_changed = last_selected_monitor != selected_monitor;
                    if monitor_changed {
                        hide_window_overlay(&mode_slots[last_selected_monitor].window);
                    }

                    last_mode_state = mode_state;
                    // A color-only change repaints via RedrawRequested; showing, hiding, or
                    // moving to another monitor also needs the window resized and repositioned.
                    if visibility_changed || monitor_changed {
                        update_mode_slot_or_exit(
                            &mut mode_slots[selected_monitor],
                            &last_mode_state,
                            last_mode_state.visible,
                            control_flow,
                        );
                    } else {
                        mode_slots[selected_monitor].window.request_redraw();
                    }
                }

                let grid_state = snap.grid_state;
                if last_grid_state != grid_state {
                    if last_selected_monitor != selected_monitor && last_grid_state.visible {
                        hide_window_overlay(&grid_slots[last_selected_monitor].window);
                    }

                    last_grid_state = grid_state;
                    update_grid_slot(&mut grid_slots[selected_monitor], last_grid_state.visible);
                }

                let letters_state = snap.letters_state;
                if last_letters_state != letters_state {
                    if last_selected_monitor != selected_monitor && last_letters_state.visible {
                        hide_window_overlay(&letters_slots[last_selected_monitor].window);
                    }

                    last_letters_state = letters_state;
                    update_grid_slot(
                        &mut letters_slots[selected_monitor],
                        last_letters_state.visible,
                    );
                }

                let mark_state = snap.mark_state;
                if last_mark_state != mark_state {
                    last_mark_state = mark_state;
                    // Marks are global: repaint every slot, not just the focused one.
                    for slot in mark_slots.iter_mut() {
                        update_mark_slot(slot, last_mark_state.visible, &last_mark_state.marks);
                    }
                }

                last_selected_monitor = selected_monitor;
            }
            WinitEvent::WindowEvent { window_id, event } => match event {
                WindowEvent::Resized(_) => {
                    if let Some(index) = find_mode_slot(&mode_slots, window_id) {
                        let visible = index == last_selected_monitor && last_mode_state.visible;
                        update_mode_slot_or_exit(
                            &mut mode_slots[index],
                            &last_mode_state,
                            visible,
                            control_flow,
                        );
                    } else if let Some(index) = find_grid_slot(&grid_slots, window_id) {
                        if index == last_selected_monitor {
                            update_grid_slot(&mut grid_slots[index], last_grid_state.visible);
                        }
                    } else if let Some(index) = find_grid_slot(&letters_slots, window_id) {
                        if index == last_selected_monitor {
                            update_grid_slot(&mut letters_slots[index], last_letters_state.visible);
                        }
                    } else if let Some(index) = find_mark_slot(&mark_slots, window_id) {
                        // Marks are global: any mark slot may need repainting.
                        update_mark_slot(
                            &mut mark_slots[index],
                            last_mark_state.visible,
                            &last_mark_state.marks,
                        );
                    }
                }
                WindowEvent::CloseRequested => {
                    shutdown_platform_input();
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            WinitEvent::RedrawRequested(window_id) => {
                if let Some(index) = find_mode_slot(&mode_slots, window_id) {
                    let visible = index == last_selected_monitor && last_mode_state.visible;
                    update_mode_slot_or_exit(
                        &mut mode_slots[index],
                        &last_mode_state,
                        visible,
                        control_flow,
                    );
                }
            }
            _ => {}
        }
    });
}
