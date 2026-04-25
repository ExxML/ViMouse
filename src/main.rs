#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod caps_lock_remap;
#[cfg(not(target_os = "macos"))]
mod caps_lock_suppress;
mod config;
mod input;
mod monitor;
mod overlay_grid;
mod overlay_icon;
mod platform_input;
mod state;

use crate::input::{spawn_input_hook, spawn_motion_loop};
use crate::monitor::collect_monitors;
use crate::overlay_grid::GridOverlayState;
use crate::overlay_grid::{create_grid_window, GridSurface};
use crate::overlay_icon::{
    create_event_loop, create_window, paint_overlay_icon, reassert_topmost,
    show_overlay_icon_window, IconSurface, OverlayIconState,
};
use crate::platform_input::{mouse_button_is_down, shutdown_platform_input};
use crate::state::{Action, SharedState};
use crate::state::{Mode, MonitorInfo};
use rdev::Button;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct UISnapshot {
    selected_monitor: usize,
    overlay_icon: OverlayIconState,
    grid_state: GridOverlayState,
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
        overlay_icon: OverlayIconState {
            mode: state.mode,
            monitor,
        },
        grid_state: GridOverlayState {
            visible: state.show_grid && state.mode == Mode::Normal,
            monitor,
        },
    }
}

struct OverlayIconSlot {
    window: Window,
    surface: IconSurface,
    monitor: MonitorInfo,
}

struct GridSlot {
    window: Window,
    surface: GridSurface,
    monitor: MonitorInfo,
}

fn create_overlay_icon_slots(
    event_loop: &EventLoop<()>,
    first_window: Window,
    monitors: &[MonitorInfo],
    mode: Mode,
) -> Result<Vec<OverlayIconSlot>, String> {
    let mut windows = Vec::with_capacity(monitors.len());
    windows.push(first_window);
    for _ in 1..monitors.len() {
        windows.push(create_window(event_loop));
    }

    let mut slots = Vec::with_capacity(monitors.len());
    for (window, monitor) in windows.into_iter().zip(monitors.iter().copied()) {
        let mut surface = IconSurface::new(&window);
        let overlay = OverlayIconState { mode, monitor };
        paint_overlay_icon(&window, &mut surface, &overlay)?;
        window.set_visible(false);
        slots.push(OverlayIconSlot {
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
) -> Vec<GridSlot> {
    let mut windows = Vec::with_capacity(monitors.len());
    windows.push(first_window);
    for _ in 1..monitors.len() {
        windows.push(create_grid_window(event_loop));
    }

    let mut slots = Vec::with_capacity(monitors.len());
    for (window, monitor) in windows.into_iter().zip(monitors.iter().copied()) {
        let mut surface = GridSurface::new(&window, &monitor);
        surface.prime(&window, &monitor);
        slots.push(GridSlot {
            window,
            surface,
            monitor,
        });
    }

    slots
}

fn find_overlay_icon_slot(slots: &[OverlayIconSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn find_grid_slot(slots: &[GridSlot], window_id: WindowId) -> Option<usize> {
    slots.iter().position(|slot| slot.window.id() == window_id)
}

fn paint_overlay_icon_slot_or_exit(
    slot: &mut OverlayIconSlot,
    mode: Mode,
    show: bool,
    control_flow: &mut ControlFlow,
) {
    let overlay = OverlayIconState {
        mode,
        monitor: slot.monitor,
    };
    match paint_overlay_icon(&slot.window, &mut slot.surface, &overlay) {
        Ok(()) if show => show_overlay_icon_window(&slot.window),
        Ok(()) => slot.window.set_visible(false),
        Err(error) => {
            eprintln!("overlay icon render error: {error}");
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
            monitor: slot.monitor,
        },
    );
}

fn main() {
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

    let event_loop = create_event_loop();
    let bootstrap_window = create_window(&event_loop);
    let bootstrap_grid_window = create_grid_window(&event_loop);

    let monitors = collect_monitors(&bootstrap_window);
    let initial_cursor = monitors
        .first()
        .copied()
        .expect("no monitors available")
        .center();

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

    spawn_input_hook(Arc::clone(&shared), Arc::clone(&motion_waker));
    spawn_motion_loop(Arc::clone(&shared), motion_waker);

    let (mut last_overlay_icon, mut last_grid_state, monitors) = {
        let state = shared.lock().expect("shared state poisoned");
        let monitor = state
            .monitors
            .get(state.selected_monitor)
            .copied()
            .expect("selected monitor out of bounds");
        (
            OverlayIconState {
                mode: state.mode,
                monitor,
            },
            GridOverlayState {
                visible: state.show_grid && state.mode == Mode::Normal,
                monitor,
            },
            state.monitors.clone(),
        )
    };

    let mut overlay_icon_slots = match create_overlay_icon_slots(
        &event_loop,
        bootstrap_window,
        &monitors,
        last_overlay_icon.mode,
    ) {
        Ok(slots) => slots,
        Err(error) => {
            eprintln!("initial overlay icon render error: {error}");
            shutdown_platform_input();
            return;
        }
    };

    let mut grid_slots = create_grid_slots(&event_loop, bootstrap_grid_window, &monitors);

    let mut last_selected_monitor = {
        let snap = current_ui_snapshot(&shared);
        snap.selected_monitor
    };
    show_overlay_icon_window(&overlay_icon_slots[last_selected_monitor].window);

    let mut topmost_reassert_ticks: u8 = 0;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33));

        match event {
            WinitEvent::MainEventsCleared => {
                let snap = current_ui_snapshot(&shared);
                let selected_monitor = snap.selected_monitor;

                let overlay_icon = snap.overlay_icon;
                if last_overlay_icon != overlay_icon {
                    let monitor_changed = last_selected_monitor != selected_monitor;
                    if monitor_changed {
                        overlay_icon_slots[last_selected_monitor]
                            .window
                            .set_visible(false);
                    }

                    last_overlay_icon = overlay_icon;
                    if monitor_changed {
                        paint_overlay_icon_slot_or_exit(
                            &mut overlay_icon_slots[selected_monitor],
                            last_overlay_icon.mode,
                            true,
                            control_flow,
                        );
                    } else {
                        overlay_icon_slots[selected_monitor].window.request_redraw();
                    }
                }

                let grid_state = snap.grid_state;
                if last_grid_state != grid_state {
                    let was_visible = last_grid_state.visible;

                    if last_selected_monitor != selected_monitor && last_grid_state.visible {
                        grid_slots[last_selected_monitor].window.set_visible(false);
                    }

                    last_grid_state = grid_state;
                    update_grid_slot(&mut grid_slots[selected_monitor], last_grid_state.visible);
                    if was_visible && !last_grid_state.visible {
                        topmost_reassert_ticks = 2;
                    }
                }

                last_selected_monitor = selected_monitor;

                if topmost_reassert_ticks > 0 {
                    topmost_reassert_ticks -= 1;
                    if topmost_reassert_ticks == 0 {
                        reassert_topmost(&overlay_icon_slots[last_selected_monitor].window);
                    }
                }
            }
            WinitEvent::WindowEvent { window_id, event } => match event {
                WindowEvent::Resized(_) => {
                    if let Some(index) = find_overlay_icon_slot(&overlay_icon_slots, window_id) {
                        let show = index == last_selected_monitor;
                        paint_overlay_icon_slot_or_exit(
                            &mut overlay_icon_slots[index],
                            last_overlay_icon.mode,
                            show,
                            control_flow,
                        );
                    } else if let Some(index) = find_grid_slot(&grid_slots, window_id) {
                        if index == last_selected_monitor {
                            update_grid_slot(&mut grid_slots[index], last_grid_state.visible);
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
                if let Some(index) = find_overlay_icon_slot(&overlay_icon_slots, window_id) {
                    let show = index == last_selected_monitor;
                    paint_overlay_icon_slot_or_exit(
                        &mut overlay_icon_slots[index],
                        last_overlay_icon.mode,
                        show,
                        control_flow,
                    );
                }
            }
            _ => {}
        }
    });
}
