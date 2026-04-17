#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod input;
mod monitor;
mod overlay;
mod platform_cursor;
mod platform_input;
mod state;

use crate::input::{spawn_input_hook, spawn_motion_loop};
use crate::monitor::{collect_monitors, initial_cursor, monitor_index_for_point};
use crate::overlay::{
    create_event_loop, create_pixels, create_window, current_overlay, paint_overlay,
    show_overlay_window,
};
use crate::state::SharedState;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::ControlFlow;

fn main() {
    let event_loop = create_event_loop();
    let window = create_window(&event_loop);

    // Discover monitors first so cursor jumps and the overlay use the same coordinate space.
    let monitors = collect_monitors(&window);
    let initial_cursor = initial_cursor(&monitors);
    let initial_monitor = monitor_index_for_point(&monitors, initial_cursor).unwrap_or(0);

    let shared = Arc::new(Mutex::new(SharedState::new(
        initial_cursor,
        initial_monitor,
        monitors,
    )));

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
