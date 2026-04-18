#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod input;
mod monitor;
mod overlay;
mod platform_input;
mod state;

use crate::input::{spawn_input_hook, spawn_motion_loop};
use crate::monitor::collect_monitors;
use crate::overlay::{
    create_event_loop, create_pixels, create_window, current_overlay, paint_overlay,
    show_overlay_window,
};
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

    let event_loop = create_event_loop();
    let window = create_window(&event_loop);

    // Discover monitors first so the initial cursor state and overlay use the same coordinate space.
    let monitors = collect_monitors(&window);
    let initial_cursor = monitors
        .first()
        .copied()
        .expect("no monitors available")
        .center();

    let mut state = SharedState::new(initial_cursor, 0, monitors);
    state.pending_actions.push(Action::MouseMove(initial_cursor));

    let shared = Arc::new(Mutex::new(state));

    spawn_input_hook(Arc::clone(&shared));
    spawn_motion_loop(Arc::clone(&shared));

    // Paint once before showing the overlay to avoid a blank startup flash.
    let mut pixels = create_pixels(&window);
    let mut last_overlay = current_overlay(&shared);
    if let Err(error) = paint_overlay(&window, &mut pixels, &last_overlay) {
        eprintln!("initial overlay render error: {error}");
        return;
    }
    show_overlay_window(&window);

    event_loop.run(move |event, _, control_flow| {
        // The overlay only changes when the mode or focused monitor changes.
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33));

        match event {
            WinitEvent::MainEventsCleared => {
                let overlay = current_overlay(&shared);
                if last_overlay != overlay {
                    window.request_redraw();
                    last_overlay = overlay;
                }
            }
            WinitEvent::RedrawRequested(_) => {
                if let Err(error) = paint_overlay(&window, &mut pixels, &last_overlay) {
                    eprintln!("overlay render error: {error}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            WinitEvent::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
