// Overlay subsystem: the always-on-top windows ViMouse draws (mode line, jump grid, grid
// letters, marks) and the rendering machinery they share.
//
// - overlay_surface: generic fullscreen transparent compositor + window creation.
// - overlay_glyph:   shared 8×8 font glyph rasterization.
// - grid_overlay:    jump-grid layout (lines + cell letters) over overlay_surface.
// - mark_overlay:    mark-glyph layout over overlay_surface.
// - mode_overlay:    mode indicator line (own softbuffer path, not overlay_surface).

mod grid_overlay;
mod mark_overlay;
mod mode_overlay;
mod overlay_glyph;
mod overlay_surface;

pub use grid_overlay::{GridOverlayState, GridSurface};
pub use mark_overlay::{MarkGlyph, MarkOverlayState, MarkSurface};
pub use mode_overlay::{
    create_event_loop, create_window, show_mode_overlay_window, update_mode_overlay,
    ModeOverlayState, ModeSurface,
};
pub use overlay_glyph::key_label;
#[cfg(target_os = "windows")]
pub use overlay_surface::create_overlay_owner_hwnd;
pub use overlay_surface::create_overlay_window;
pub use overlay_surface::create_topmost_anchor;
pub use overlay_surface::hide_overlay_window;

// Raise an overlay window above menus, Spotlight, tooltips, and drag images, and pin it to
// every Space so another app entering native fullscreen can't leave it behind.
#[cfg(target_os = "macos")]
fn raise_overlay_window_level(window: &winit::window::Window) {
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
