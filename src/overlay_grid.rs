use crate::config::JUMP_GRID;
use crate::state::{MonitorInfo, Shared};
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder, WindowLevel};
#[cfg(not(target_os = "macos"))]
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
#[cfg(target_os = "macos")]
use winit::dpi::{LogicalPosition, LogicalSize};
#[cfg(target_os = "linux")]
use winit::platform::x11::{WindowBuilderExtX11, WindowExtX11, XWindowType};
#[cfg(target_os = "windows")]
use winit::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};

const GRID_COLS: usize = JUMP_GRID[0].len();
const GRID_ROWS: usize = JUMP_GRID.len();

const LINE_THICKNESS: usize = 1;

// Pre-multiplied ARGB for UpdateLayeredWindow on Windows (alpha=200 grey line)
const LINE_ALPHA: u8 = 200;
const LINE_GREY: u8 = 128;

#[derive(Clone, Debug, PartialEq)]
pub struct GridOverlayState {
    pub visible: bool,
    pub monitor: MonitorInfo,
}

pub fn current_grid_state(shared: &Shared) -> GridOverlayState {
    let state = shared.lock().expect("shared state poisoned");
    let monitor = state
        .monitors
        .get(state.selected_monitor)
        .copied()
        .expect("selected monitor out of bounds");
    GridOverlayState {
        visible: state.show_grid && state.mode == crate::state::Mode::Normal,
        monitor,
    }
}

// Per-platform grid surface state.
pub struct GridSurface {
    imp: GridSurfaceImp,
}

impl GridSurface {
    pub fn new(window: &Window) -> Self {
        Self {
            imp: GridSurfaceImp::new(window),
        }
    }

    pub fn update(&mut self, window: &Window, state: &GridOverlayState) {
        if !state.visible {
            window.set_visible(false);
            return;
        }
        position_grid_window(window, &state.monitor);
        let (w, h) = monitor_size_physical(&state.monitor);
        set_grid_window_size(window, &state.monitor, w, h);
        window.set_visible(true);
        self.imp.paint(window, w, h);
    }
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
struct GridSurfaceImp;

#[cfg(target_os = "windows")]
impl GridSurfaceImp {
    fn new(_window: &Window) -> Self {
        Self
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32) {
        use std::ptr;
        use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
        use windows_sys::Win32::Graphics::Gdi::{
            AC_SRC_ALPHA, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BI_RGB,
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
            DIB_RGB_COLORS, GetDC, ReleaseDC, SelectObject,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

        let hwnd = window.hwnd() as HWND;

        let pixel_count = (w * h) as usize;
        let mut pixels: Vec<u32> = vec![0u32; pixel_count];
        fill_grid_argb_premult(&mut pixels, w as usize, h as usize);

        unsafe {
            let screen_dc = GetDC(ptr::null_mut());
            let mem_dc = CreateCompatibleDC(screen_dc);

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [std::mem::zeroed()],
            };

            let mut dib_bits: *mut std::ffi::c_void = ptr::null_mut();
            let hbm = CreateDIBSection(
                mem_dc,
                &bmi,
                DIB_RGB_COLORS,
                &mut dib_bits,
                ptr::null_mut(),
                0,
            );
            if hbm.is_null() || dib_bits.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);
                return;
            }

            let old_bm = SelectObject(mem_dc, hbm);

            // DIB stores BGRA in memory; our pixel value is pre-multiplied 0xAARRGGBB.
            // Rearrange to BGRA bytes: store as 0xAARRGGBB u32 in little-endian = B G R A.
            let dib_slice = std::slice::from_raw_parts_mut(dib_bits as *mut u32, pixel_count);
            for (dst, &src) in dib_slice.iter_mut().zip(pixels.iter()) {
                let a = (src >> 24) as u8;
                let r = (src >> 16) as u8;
                let g = (src >> 8) as u8;
                let b = src as u8;
                // Store BGRA: byte0=B, byte1=G, byte2=R, byte3=A
                *dst = (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | ((a as u32) << 24);
            }

            let blend = BLENDFUNCTION {
                BlendOp: 0, // AC_SRC_OVER
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let outer = window.outer_position().unwrap_or_default();
            let pt_dst = POINT { x: outer.x, y: outer.y };
            let sz = SIZE { cx: w as i32, cy: h as i32 };
            let pt_src = POINT { x: 0, y: 0 };

            UpdateLayeredWindow(
                hwnd,
                screen_dc,
                &pt_dst,
                &sz,
                mem_dc,
                &pt_src,
                0,
                &blend,
                ULW_ALPHA,
            );

            SelectObject(mem_dc, old_bm);
            DeleteObject(hbm);
            DeleteDC(mem_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
    }
}

// Fill pre-multiplied ARGB pixels for the grid lines (Windows DIB order: 0xAARRGGBB).
#[cfg(target_os = "windows")]
fn fill_grid_argb_premult(pixels: &mut [u32], w: usize, h: usize) {
    // Pre-multiplied: R,G,B are multiplied by A/255.
    let pm = (LINE_GREY as u32 * LINE_ALPHA as u32) / 255;
    let line_pixel: u32 = ((LINE_ALPHA as u32) << 24) | (pm << 16) | (pm << 8) | pm;

    for col in 1..GRID_COLS {
        let x_center = col * w / GRID_COLS;
        let x_start = x_center.saturating_sub(LINE_THICKNESS / 2);
        let x_end = (x_start + LINE_THICKNESS).min(w);
        for y in 0..h {
            for x in x_start..x_end {
                pixels[y * w + x] = line_pixel;
            }
        }
    }

    for row in 1..GRID_ROWS {
        let y_center = row * h / GRID_ROWS;
        let y_start = y_center.saturating_sub(LINE_THICKNESS / 2);
        let y_end = (y_start + LINE_THICKNESS).min(h);
        for y in y_start..y_end {
            for x in 0..w {
                pixels[y * w + x] = line_pixel;
            }
        }
    }
}

// ── macOS / Linux implementation (raw wgpu with correct transparent alpha mode) ─

// pixels::wgpu re-exports the wgpu crate used internally by pixels, so we stay
// on the same version without adding a separate dependency.
#[cfg(not(target_os = "windows"))]
use pixels::wgpu;

#[cfg(not(target_os = "windows"))]
struct GridSurfaceImp {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    texture_size: (u32, u32),
}

#[cfg(not(target_os = "windows"))]
impl GridSurfaceImp {
    fn new(window: &Window) -> Self {

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = unsafe { instance.create_surface(window) }
            .expect("grid wgpu surface creation failed");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            power_preference: wgpu::PowerPreference::default(),
        }))
        .expect("grid wgpu adapter not found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                limits: adapter.limits(),
                ..Default::default()
            },
            None,
        ))
        .expect("grid wgpu device request failed");

        let caps = surface.get_capabilities(&adapter);

        // Pick an sRGB surface format if available.
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        // Pick the best available alpha mode for transparency.
        // macOS Metal exposes [Opaque, PostMultiplied]; prefer PostMultiplied.
        // Linux Vulkan may expose PreMultiplied/Inherit depending on compositor.
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .iter()
        .copied()
        .find(|m| caps.alpha_modes.contains(m))
        .unwrap_or(caps.alpha_modes[0]);

        let size = window.inner_size();
        let w = size.width.max(1);
        let h = size.height.max(1);

        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width: w,
                height: h,
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode,
                view_formats: vec![],
            },
        );

        // Create a 1×1 placeholder texture; it will be replaced in paint().
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let pipeline = Self::build_pipeline(&device, surface_format, &texture);

        Self { surface, device, queue, surface_format, alpha_mode, pipeline, texture, texture_size: (w, h) }
    }

    fn build_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat, texture: &wgpu::Texture) -> wgpu::RenderPipeline {
        // Fullscreen triangle shader: samples the grid texture and blits it to the surface.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(GRID_SHADER)),
        });

        let tex_view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let _bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&tex_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        })
    }

    fn paint(&mut self, window: &Window, w: u32, h: u32) {
        // Resize surface and texture if monitor size changed.
        if self.texture_size != (w, h) {
            self.surface.configure(
                &self.device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: self.surface_format,
                    width: w,
                    height: h,
                    present_mode: wgpu::PresentMode::AutoVsync,
                    alpha_mode: self.alpha_mode,
                    view_formats: vec![],
                },
            );
            self.texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.pipeline = Self::build_pipeline(&self.device, self.surface_format, &self.texture);
            self.texture_size = (w, h);
        }

        // Build the RGBA pixel data.
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        draw_grid_rgba(&mut pixels, w as usize, h as usize);

        // Upload to the GPU texture.
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        // Acquire surface frame and render.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => { eprintln!("grid surface error: {e}"); return; }
        };
        let view = frame.texture.create_view(&Default::default());
        let tex_view = self.texture.create_view(&Default::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = self.pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&tex_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        window.request_redraw();
    }
}

#[cfg(not(target_os = "windows"))]
const GRID_SHADER: &str = r#"
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;

struct VertOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    // Fullscreen triangle covering clip space [-1,1]x[-1,1].
    var pos = array<vec2<f32>,3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    var uv  = array<vec2<f32>,3>(vec2(0.0,1.0),   vec2(2.0,1.0),  vec2(0.0,-1.0));
    var o: VertOut;
    o.pos = vec4(pos[vi], 0.0, 1.0);
    o.uv  = uv[vi];
    return o;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return textureSample(t, s, in.uv);
}
"#;

#[cfg(not(target_os = "windows"))]
fn draw_grid_rgba(frame: &mut [u8], w: usize, h: usize) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk.copy_from_slice(&[0, 0, 0, 0]);
    }
    for col in 1..GRID_COLS {
        let x_center = col * w / GRID_COLS;
        let x_start = x_center.saturating_sub(LINE_THICKNESS / 2);
        let x_end = (x_start + LINE_THICKNESS).min(w);
        for y in 0..h {
            for x in x_start..x_end {
                let i = (y * w + x) * 4;
                frame[i] = LINE_GREY;
                frame[i + 1] = LINE_GREY;
                frame[i + 2] = LINE_GREY;
                frame[i + 3] = LINE_ALPHA;
            }
        }
    }
    for row in 1..GRID_ROWS {
        let y_center = row * h / GRID_ROWS;
        let y_start = y_center.saturating_sub(LINE_THICKNESS / 2);
        let y_end = (y_start + LINE_THICKNESS).min(h);
        for y in y_start..y_end {
            for x in 0..w {
                let i = (y * w + x) * 4;
                frame[i] = LINE_GREY;
                frame[i + 1] = LINE_GREY;
                frame[i + 2] = LINE_GREY;
                frame[i + 3] = LINE_ALPHA;
            }
        }
    }
}

// ── Window creation ───────────────────────────────────────────────────────────

pub fn create_grid_window(event_loop: &EventLoop<()>) -> Window {
    let builder = WindowBuilder::new()
        .with_title("ViMouse Grid")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_active(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(PhysicalSize::new(1u32, 1u32));

    let builder = configure_grid_window_builder(builder);
    let window = builder.build(event_loop).expect("failed to create grid window");
    configure_grid_overlay_window(&window);
    window
}

#[cfg(target_os = "windows")]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_skip_taskbar(true)
}

#[cfg(target_os = "linux")]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
        .with_override_redirect(true)
        .with_x11_window_type(vec![XWindowType::Notification])
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn configure_grid_window_builder(builder: WindowBuilder) -> WindowBuilder {
    builder
}

fn configure_grid_overlay_window(window: &Window) {
    let _ = window.set_cursor_hittest(false);
    platform_configure_grid_window(window);
}

#[cfg(target_os = "windows")]
fn platform_configure_grid_window(window: &Window) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };
    unsafe {
        let hwnd = window.hwnd() as HWND;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ((ex & !WS_EX_APPWINDOW)
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW) as isize,
        );
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    }
}

#[cfg(target_os = "linux")]
fn platform_configure_grid_window(window: &Window) {
    use std::ffi::c_void;
    use x11_dl::xlib;
    let Some(display) = window.xlib_display() else { return };
    let Some(xwindow) = window.xlib_window() else { return };
    let Ok(xlib) = xlib::Xlib::open() else { return };
    unsafe {
        let display = display as *mut xlib::Display;
        let hints = {
            let p = (xlib.XGetWMHints)(display, xwindow);
            if p.is_null() { (xlib.XAllocWMHints)() } else { p }
        };
        if hints.is_null() { return; }
        (*hints).flags |= xlib::InputHint;
        (*hints).input = 0;
        (xlib.XSetWMHints)(display, xwindow, hints);
        (xlib.XFlush)(display);
        (xlib.XFree)(hints as *mut c_void);
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_configure_grid_window(_window: &Window) {}

// ── size / position helpers ───────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn monitor_size_physical(monitor: &MonitorInfo) -> (u32, u32) {
    let w = (monitor.width * monitor.scale_factor).round() as u32;
    let h = (monitor.height * monitor.scale_factor).round() as u32;
    (w.max(1), h.max(1))
}

#[cfg(not(target_os = "macos"))]
fn monitor_size_physical(monitor: &MonitorInfo) -> (u32, u32) {
    (monitor.width.round() as u32, monitor.height.round() as u32)
}

#[cfg(target_os = "macos")]
fn set_grid_window_size(window: &Window, monitor: &MonitorInfo, _w: u32, _h: u32) {
    window.set_inner_size(LogicalSize::new(monitor.width, monitor.height));
}

#[cfg(not(target_os = "macos"))]
fn set_grid_window_size(window: &Window, _monitor: &MonitorInfo, w: u32, h: u32) {
    window.set_inner_size(PhysicalSize::new(w, h));
}

#[cfg(target_os = "macos")]
fn position_grid_window(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(LogicalPosition::new(monitor.origin.x, monitor.origin.y));
}

#[cfg(not(target_os = "macos"))]
fn position_grid_window(window: &Window, monitor: &MonitorInfo) {
    window.set_outer_position(PhysicalPosition::new(
        monitor.origin.x.round() as i32,
        monitor.origin.y.round() as i32,
    ));
}
