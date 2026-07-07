// Overlay subsystem: the always-on-top windows ViMouse draws (mode icon, jump grid, grid
// letters, marks) and the rendering machinery they share.
//
// - overlay_surface: generic fullscreen transparent compositor + window creation.
// - overlay_glyph:   shared 8×8 font glyph rasterization.
// - grid_overlay:    jump-grid layout (lines + cell letters) over overlay_surface.
// - mark_overlay:    mark-glyph layout over overlay_surface.
// - icon_overlay:    mode indicator (own softbuffer path, not overlay_surface).

mod grid_overlay;
mod icon_overlay;
mod mark_overlay;
mod overlay_glyph;
mod overlay_surface;

pub use grid_overlay::{GridOverlayState, GridSurface};
pub use icon_overlay::{
    create_event_loop, create_window, paint_icon_overlay, show_icon_overlay_window,
    IconOverlayState, IconSurface,
};
pub use mark_overlay::{MarkGlyph, MarkOverlayState, MarkSurface};
pub use overlay_glyph::key_label;
#[cfg(target_os = "windows")]
pub use overlay_surface::create_overlay_owner_hwnd;
pub use overlay_surface::create_overlay_window;
pub use overlay_surface::create_topmost_anchor;
pub use overlay_surface::hide_overlay_window;
