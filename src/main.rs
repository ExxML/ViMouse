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
mod input;
mod monitor;
mod overlay;
mod platform_input;
mod state;

use crate::input::{spawn_input_hook, spawn_motion_loop};
use crate::monitor::collect_monitors;
#[cfg(target_os = "windows")]
use crate::overlay::create_overlay_owner_hwnd;
use crate::overlay::create_topmost_anchor;
use crate::overlay::{
    create_event_loop, create_overlay_window, create_window, key_label, paint_icon_overlay,
    show_icon_overlay_window, GridOverlayState, GridSurface, IconOverlayState, IconSurface,
    MarkGlyph, MarkOverlayState, MarkSurface,
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
    icon_overlay: IconOverlayState,
    grid_state: GridOverlayState,
    letters_state: GridOverlayState,
    mark_state: MarkOverlayState,
    show_overlays: bool,
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
        show_overlays: state.show_overlays,
        icon_overlay: IconOverlayState {
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
            monitor,
            marks: mark_glyphs(&state.marks),
        },
    }
}

struct IconSlot {
    window: Window,
    surface: IconSurface,
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

fn create_icon_slots(
    event_loop: &EventLoop<()>,
    first_window: Window,
    monitors: &[MonitorInfo],
    mode: Mode,
) -> Result<Vec<IconSlot>, String> {
    let mut windows = Vec::with_capacity(monitors.len());
    windows.push(first_window);
    for _ in 1..monitors.len() {
        windows.push(create_window(event_loop));
    }

    let mut slots = Vec::with_capacity(monitors.len());
    for (window, monitor) in windows.into_iter().zip(monitors.iter().copied()) {
        let mut surface = IconSurface::new(&window);
        let overlay = IconOverlayState { mode, monitor };
        paint_icon_overlay(&window, &mut surface, &overlay)?;
        window.set_visible(false);
        slots.push(IconSlot {
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
        windows.push(create_overlay_window(event_loop, owner));
        #[cfg(not(target_os = "windows"))]
        windows.push(create_overlay_window(event_loop));
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
        windows.push(create_overlay_window(event_loop, owner));
        #[cfg(not(target_os = "windows"))]
        windows.push(create_overlay_window(event_loop));
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

fn find_icon_slot(slots: &[IconSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn find_grid_slot(slots: &[GridSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn find_mark_slot(slots: &[MarkSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn paint_icon_slot_or_exit(
    slot: &mut IconSlot,
    mode: Mode,
    show: bool,
    control_flow: &mut ControlFlow,
) {
    let overlay = IconOverlayState {
        mode,
        monitor: slot.monitor,
    };
    match paint_icon_overlay(&slot.window, &mut slot.surface, &overlay) {
        Ok(()) if show => show_icon_overlay_window(&slot.window),
        Ok(()) => slot.window.set_visible(false),
        Err(error) => {
            eprintln!("icon overlay render error: {error}");
            shutdown_platform_input();
            *control_flow = ControlFlow::Exit;
        }
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

fn update_mark_slot(slot: &mut MarkSlot, visible: bool, marks: &[MarkGlyph]) {
    slot.surface.update(
        &slot.window,
        &MarkOverlayState {
            visible,
            monitor: slot.monitor,
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

    #[cfg(target_os = "macos")]
    {
        let default_panic = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::caps_lock_remap::shutdown();
            default_panic(info);
        }));
    }

    let event_loop = create_event_loop();
    let bootstrap_window = create_window(&event_loop);
    #[cfg(target_os = "windows")]
    let grid_owner = create_overlay_owner_hwnd();
    #[cfg(target_os = "windows")]
    let bootstrap_grid_window = create_overlay_window(&event_loop, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let bootstrap_grid_window = create_overlay_window(&event_loop);
    #[cfg(target_os = "windows")]
    let bootstrap_letters_window = create_overlay_window(&event_loop, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let bootstrap_letters_window = create_overlay_window(&event_loop);
    #[cfg(target_os = "windows")]
    let bootstrap_mark_window = create_overlay_window(&event_loop, grid_owner);
    #[cfg(not(target_os = "windows"))]
    let bootstrap_mark_window = create_overlay_window(&event_loop);

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
        mut last_icon_overlay,
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
            IconOverlayState {
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
                monitor,
                marks: mark_glyphs(&state.marks),
            },
            state.monitors.clone(),
        )
    };

    let mut icon_slots = match create_icon_slots(
        &event_loop,
        bootstrap_window,
        &monitors,
        last_icon_overlay.mode,
    ) {
        Ok(slots) => slots,
        Err(error) => {
            eprintln!("initial icon overlay render error: {error}");
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
    let mut last_show_overlays = true;
    show_icon_overlay_window(&icon_slots[last_selected_monitor].window);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            WinitEvent::MainEventsCleared | WinitEvent::UserEvent(()) => {
                let snap = current_ui_snapshot(&shared);
                let selected_monitor = snap.selected_monitor;

                let show_overlays = snap.show_overlays;
                let icon_overlay = snap.icon_overlay;
                let icon_changed = last_icon_overlay != icon_overlay;
                let overlays_changed = last_show_overlays != show_overlays;
                if icon_changed || overlays_changed {
                    let monitor_changed = last_selected_monitor != selected_monitor;
                    if monitor_changed {
                        icon_slots[last_selected_monitor].window.set_visible(false);
                    }

                    last_icon_overlay = icon_overlay;
                    last_show_overlays = show_overlays;
                    if overlays_changed || monitor_changed {
                        paint_icon_slot_or_exit(
                            &mut icon_slots[selected_monitor],
                            last_icon_overlay.mode,
                            show_overlays,
                            control_flow,
                        );
                    } else {
                        icon_slots[selected_monitor].window.request_redraw();
                    }
                }

                let grid_state = snap.grid_state;
                if last_grid_state != grid_state {
                    if last_selected_monitor != selected_monitor && last_grid_state.visible {
                        grid_slots[last_selected_monitor].window.set_visible(false);
                    }

                    last_grid_state = grid_state;
                    update_grid_slot(&mut grid_slots[selected_monitor], last_grid_state.visible);
                }

                let letters_state = snap.letters_state;
                if last_letters_state != letters_state {
                    if last_selected_monitor != selected_monitor && last_letters_state.visible {
                        letters_slots[last_selected_monitor]
                            .window
                            .set_visible(false);
                    }

                    last_letters_state = letters_state;
                    update_grid_slot(
                        &mut letters_slots[selected_monitor],
                        last_letters_state.visible,
                    );
                }

                let mark_state = snap.mark_state;
                if last_mark_state != mark_state {
                    if last_selected_monitor != selected_monitor && last_mark_state.visible {
                        mark_slots[last_selected_monitor].window.set_visible(false);
                    }

                    last_mark_state = mark_state;
                    update_mark_slot(
                        &mut mark_slots[selected_monitor],
                        last_mark_state.visible,
                        &last_mark_state.marks,
                    );
                }

                last_selected_monitor = selected_monitor;
            }
            WinitEvent::WindowEvent { window_id, event } => match event {
                WindowEvent::Resized(_) => {
                    if let Some(index) = find_icon_slot(&icon_slots, window_id) {
                        let show = index == last_selected_monitor && last_show_overlays;
                        paint_icon_slot_or_exit(
                            &mut icon_slots[index],
                            last_icon_overlay.mode,
                            show,
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
                        if index == last_selected_monitor {
                            update_mark_slot(
                                &mut mark_slots[index],
                                last_mark_state.visible,
                                &last_mark_state.marks,
                            );
                        }
                    }
                }
                WindowEvent::CloseRequested => {
                    shutdown_platform_input();
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            WinitEvent::RedrawRequested(window_id) => {
                if let Some(index) = find_icon_slot(&icon_slots, window_id) {
                    let show = index == last_selected_monitor && last_show_overlays;
                    paint_icon_slot_or_exit(
                        &mut icon_slots[index],
                        last_icon_overlay.mode,
                        show,
                        control_flow,
                    );
                }
            }
            _ => {}
        }
    });
}
