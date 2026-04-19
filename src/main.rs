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
use crate::overlay_grid::{GridSurface, create_grid_window, current_grid_state};
use crate::overlay_icon::{
    create_event_loop, create_pixels, create_window, current_overlay_icon, paint_overlay_icon,
    show_overlay_icon_window,
};
use crate::platform_input::shutdown_platform_input;
use crate::state::{Action, SharedState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::ControlFlow;

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
    let window = create_window(&event_loop);
    let grid_window = create_grid_window(&event_loop);

    // Discover monitors first so the initial cursor state and overlay use the same coordinate space.
    let monitors = collect_monitors(&window);
    let initial_cursor = monitors
        .first()
        .copied()
        .expect("no monitors available")
        .center();

    let mut state = SharedState::new(initial_cursor, 0, monitors);
    state
        .pending_actions
        .push(Action::MouseMove(initial_cursor));

    let shared = Arc::new(Mutex::new(state));

    spawn_input_hook(Arc::clone(&shared));
    spawn_motion_loop(Arc::clone(&shared));

    // Paint once before showing the overlay icon to avoid a blank startup flash.
    let mut pixels = create_pixels(&window);
    let initial_monitor = shared.lock().expect("shared state poisoned").monitors[0];
    let mut grid_surface = GridSurface::new(&grid_window, &initial_monitor);
    let mut last_overlay_icon = current_overlay_icon(&shared);
    let mut last_grid_state = current_grid_state(&shared);

    if let Err(error) = paint_overlay_icon(&window, &mut pixels, &last_overlay_icon) {
        eprintln!("initial overlay icon render error: {error}");
        shutdown_platform_input();
        return;
    }
    show_overlay_icon_window(&window);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33));

        match event {
            WinitEvent::MainEventsCleared => {
                let overlay_icon = current_overlay_icon(&shared);
                if last_overlay_icon != overlay_icon {
                    window.request_redraw();
                    last_overlay_icon = overlay_icon;
                }

                let grid_state = current_grid_state(&shared);
                if last_grid_state != grid_state {
                    last_grid_state = grid_state;
                    grid_surface.update(&grid_window, &last_grid_state);
                }
            }
            WinitEvent::RedrawRequested(window_id) => {
                if window_id == window.id() {
                    if let Err(error) =
                        paint_overlay_icon(&window, &mut pixels, &last_overlay_icon)
                    {
                        eprintln!("overlay icon render error: {error}");
                        shutdown_platform_input();
                        *control_flow = ControlFlow::Exit;
                    }
                }
            }
            WinitEvent::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                shutdown_platform_input();
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
