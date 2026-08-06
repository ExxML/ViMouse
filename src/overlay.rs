// Overlay subsystem: the always-on-top windows ViMouse draws (mode line, jump grid, grid
// letters, marks) and the rendering machinery they share.
//
// - surface_overlay: generic fullscreen transparent compositor + window creation.
// - glyph_overlay:   shared 8×8 font glyph rasterization.
// - grid_overlay:    jump-grid layout (lines + cell letters) over surface_overlay.
// - mark_overlay:    mark-glyph layout over surface_overlay.
// - mode_overlay:    mode indicator line (own softbuffer path, not surface_overlay).

mod glyph_overlay;
mod grid_overlay;
mod mark_overlay;
mod mode_overlay;
mod surface_overlay;

pub use glyph_overlay::key_label;
pub use grid_overlay::{GridOverlayState, GridSurface};
pub use mark_overlay::{MarkGlyph, MarkOverlayState, MarkSurface};
pub use mode_overlay::{
    create_event_loop, create_window, show_mode_overlay_window, update_mode_overlay,
    ModeOverlayState, ModeSurface,
};
pub use surface_overlay::create_topmost_anchor;
pub use surface_overlay::create_window_overlay;
#[cfg(target_os = "windows")]
pub use surface_overlay::create_window_overlay_owner_hwnd;
pub use surface_overlay::hide_window_overlay;

// Raise an window overlay above menus, Spotlight, tooltips, and drag images, and pin it to
// every Space so another app entering native fullscreen can't leave it behind.
#[cfg(target_os = "macos")]
fn raise_window_overlay_level(window: &winit::window::Window) {
    // NSWindowCollectionBehavior bits: join all Spaces, layer over other apps' fullscreen
    // windows, and ignore Space-switch animations.
    const CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
    const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;
    const STATIONARY: u64 = 1 << 4;
    // NSWindowAnimationBehaviorNone: the auxiliary collection behavior otherwise lets AppKit
    // infer a window-open/close animation on every order-front/order-out.
    const ANIMATION_BEHAVIOR_NONE: i64 = 2;
    unsafe {
        use objc::runtime::Object;
        use winit::platform::macos::WindowExtMacOS;
        let ns_window = window.ns_window() as *mut Object;
        // One below kCGScreenSaverWindowLevel (1000): above every transient system UI, below the screen saver.
        let _: () = msg_send![ns_window, setLevel: 999i64];
        let _: () = msg_send![
            ns_window,
            setCollectionBehavior: CAN_JOIN_ALL_SPACES | FULL_SCREEN_AUXILIARY | STATIONARY
        ];
        let _: () = msg_send![ns_window, setAnimationBehavior: ANIMATION_BEHAVIOR_NONE];
    }
}
