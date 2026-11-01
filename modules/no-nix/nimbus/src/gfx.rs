//! GPU side: one wgpu device shared by every output, one swapchain per
//! layer surface. A frame is a single render pass: an instanced SDF draw for
//! all shapes, then glyphon for text and icons.

use std::{collections::HashMap, ptr::NonNull};

use glyphon::{
    Cache, Color, ColorMode, ContentType, CustomGlyph, FontSystem, RasterizeCustomGlyphRequest,
    RasterizedCustomGlyph, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
    Viewport,
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};

use crate::theme::Rgba;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Shape {
    pub rect: [f32; 4],
    pub geom: [f32; 4],
    pub fill_a: [f32; 4],
    pub fill_b: [f32; 4],
    pub rim_a: [f32; 4],
    pub rim_b: [f32; 4],
    pub shadow: [f32; 4],
    pub grad: [f32; 4],
    pub spot: [f32; 4],
    pub bloom: [f32; 4],
    /// Prism Ink: rim strength (edge-normal colours), iridescent fill
    /// amount, lighten toward white, unused.
    pub prism: [f32; 4],
}

const SHAPE_ATTRS: [wgpu::VertexAttribute; 11] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4,
    5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
    10 => Float32x4,
];

/// A piece of text placed by the scene. `key` indexes the scene's buffer cache.
pub struct TextDraw {
    pub key: u64,
    pub x: f32,
    pub y: f32,
    pub color: Rgba,
    pub clip: [i32; 4],
}

pub struct IconDraw {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Rgba,
}

/// A decoded RGBA8 image (straight alpha) handed to the GPU by key.
#[derive(Clone)]
pub struct ImageData {
    pub w: u32,
    pub h: u32,
    pub rgba: std::sync::Arc<Vec<u8>>,
}

/// A textured rounded rect in physical pixels. Drawn after shapes and
/// before text.
#[derive(Clone, Copy, Debug)]
pub struct ImageDraw {
    /// Key of a texture uploaded via `DrawList::uploads` (now or earlier).
    pub key: u64,
    pub rect: [f32; 4],
    pub radius: f32,
    /// Source region in 0..1 texture coordinates (u0, v0, u1, v1).
    pub uv: [f32; 4],
    pub alpha: f32,
    /// Circular reveal mask: (cx, cy, radius, softness); radius < 0 = off.
    pub reveal: [f32; 4],
    /// Order-map wipe (wallpaper transitions, see `wipe.wgsl`): the key of
    /// an order texture laid over this image's UVs and (progress, soft
    /// edge, refraction px, rim strength).
    pub mask: Option<(u64, [f32; 4])>,
    /// Colour multiplier (1 = as uploaded).
    pub bright: f32,
    /// Large, smooth upscales (blurred backgrounds): B-spline sampling and
    /// output dither, so dark gradients don't band.
    pub smooth: bool,
}

impl ImageDraw {
    pub fn new(key: u64, rect: [f32; 4]) -> Self {
        Self {
            key,
            rect,
            radius: 0.0,
            uv: [0.0, 0.0, 1.0, 1.0],
            alpha: 1.0,
            reveal: [0.0, 0.0, -1.0, 0.0],
            mask: None,
            bright: 1.0,
            smooth: false,
        }
    }
}

/// Texture keys whose owner went away without drawing a last frame (a
/// window dropped together with its surface): the next render frees them.
static ORPHANS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

/// Free these textures on the next render (any surface).
pub fn free_later(keys: impl IntoIterator<Item = u64>) {
    if let Ok(mut o) = ORPHANS.lock() {
        o.extend(keys);
    }
}

#[derive(Default)]
pub struct DrawList {
    pub shapes: Vec<Shape>,
    /// Images drawn *before* the shapes (full-screen backgrounds).
    pub backdrop: Vec<ImageDraw>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub icons: Vec<IconDraw>,
    /// Textures to (re)upload before drawing. Textures live on the GPU,
    /// shared by all surfaces, until freed.
    pub uploads: Vec<(u64, ImageData)>,
    pub frees: Vec<u64>,
    /// Glass panels (logical surface px, corner radius): the compositor
    /// blurs what is behind exactly these.
    pub blur: Vec<(crate::bar::Rect, f32)>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.backdrop.clear();
        self.images.clear();
        self.texts.clear();
        self.icons.clear();
        self.uploads.clear();
        self.frees.clear();
        self.blur.clear();
    }

    /// Blur behind a panel while it is mostly visible (the region is
    /// all-or-nothing, so a fading panel drops it halfway).
    pub fn glass(&mut self, r: crate::bar::Rect, radius: f32, alpha: f32) {
        if alpha > 0.5 && r.w > 2.0 && r.h > 2.0 {
            self.blur.push((r, radius));
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct ImageInst {
    rect: [f32; 4],
    uv: [f32; 4],
    params: [f32; 4],
    reveal: [f32; 4],
    wipe: [f32; 4],
}

const IMAGE_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
const WIPE_ATTRS: [wgpu::VertexAttribute; 5] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];

pub struct Icons {
    trees: Vec<resvg::usvg::Tree>,
}

impl Icons {
    pub fn load(paths: &[Option<std::path::PathBuf>]) -> Self {
        let opt = resvg::usvg::Options::default();
        let trees = paths
            .iter()
            .map(|p| {
                p.as_ref()
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|d| resvg::usvg::Tree::from_data(&d, &opt).ok())
                    .unwrap_or_else(|| {
                        resvg::usvg::Tree::from_str("<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'/>", &opt)
                            .unwrap()
                    })
            })
            .collect();
        Self { trees }
    }

    fn rasterize(&self, req: RasterizeCustomGlyphRequest) -> Option<RasterizedCustomGlyph> {
        let tree = self.trees.get(req.id as usize)?;
        let size = tree.size();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(req.width as u32, req.height as u32)?;
        let transform = resvg::usvg::Transform::from_scale(
            req.width as f32 / size.width(),
            req.height as f32 / size.height(),
        )
        .post_translate(req.x_bin.as_float(), req.y_bin.as_float());
        resvg::render(tree, transform, &mut pixmap.as_mut());
        // Symbolic icons are monochrome: keep coverage only and tint on the GPU.
        let data = pixmap.data().iter().skip(3).step_by(4).copied().collect();
        Some(RasterizedCustomGlyph { data, content_type: ContentType::Mask })
    }
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    display: Option<RawDisplayHandle>,
    shared: Option<Shared>,
    pub fonts: FontSystem,
    swash: SwashCache,
}

/// Format-dependent state, created on the first surface.
struct Shared {
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    image_pipeline: wgpu::RenderPipeline,
    /// Same, masked by an order map (wallpaper transitions).
    wipe_pipeline: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: HashMap<u64, (wgpu::Texture, wgpu::BindGroup)>,
    glyph_cache: Cache,
    atlas: TextAtlas,
}

impl Gpu {
    pub fn new(display_ptr: Option<*mut std::ffi::c_void>) -> Self {
        Self::try_new(display_ptr).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Like `new`, but reports a missing Vulkan loader, adapter or device
    /// instead of panicking (the session locker must survive without a GPU).
    pub fn try_new(display_ptr: Option<*mut std::ffi::c_void>) -> Result<Self, String> {
        preload_vulkan_loader();
        let display = display_ptr.and_then(NonNull::new).map(|p| RawDisplayHandle::Wayland(WaylandDisplayHandle::new(p)));
        let new_instance = || {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            desc.backends = wgpu::Backends::VULKAN;
            wgpu::Instance::new(desc)
        };
        let request = |instance: &wgpu::Instance| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                ..Default::default()
            }))
        };
        let selected = select_vulkan_drivers();
        let mut instance = new_instance();
        if selected {
            // The loader has read it; keep it out of the processes we spawn.
            unsafe { std::env::remove_var("VK_LOADER_DRIVERS_SELECT") };
        }
        let adapter = match request(&instance) {
            Ok(a) => a,
            Err(_) if selected => {
                instance = new_instance();
                request(&instance).map_err(|e| format!("no Vulkan adapter: {e}"))?
            }
            Err(e) => return Err(format!("no Vulkan adapter: {e}")),
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nimbus"),
            // Everything we allocate fits in well under 1 MiB, so start with
            // the smallest block gpu-allocator allows (MemoryUsage starts at 8).
            memory_hints: wgpu::MemoryHints::Manual { suballocated_device_memory_block_size: 4 << 20..64 << 20 },
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;

        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), glyphon::fontdb::Database::new());
        let mut families = vec![crate::theme::current().font];
        families.extend_from_slice(crate::designs::Design::current().fonts());
        crate::fonts::load(fonts.db_mut(), &families);

        Ok(Self { instance, adapter, device, queue, display, shared: None, fonts, swash: SwashCache::new() })
    }

    /// Never panic on GPU errors: log validation errors, and call `lost`
    /// (from whatever thread wgpu is on) if the device goes away.
    pub fn survive_errors(&self, lost: impl Fn() + Send + 'static) {
        self.device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "nimbus: gpu error: {e}");
        }));
        self.device.set_device_lost_callback(move |reason, msg| {
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "nimbus: gpu device lost ({reason:?}): {msg}");
            lost();
        });
    }

    pub fn create_surface(&mut self, wl_surface_ptr: *mut std::ffi::c_void, w: u32, h: u32) -> SurfaceGfx {
        self.try_create_surface(wl_surface_ptr, w, h).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_create_surface(&mut self, wl_surface_ptr: *mut std::ffi::c_void, w: u32, h: u32) -> Result<SurfaceGfx, String> {
        let ptr = NonNull::new(wl_surface_ptr).ok_or("null wl_surface")?;
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr));
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: self.display,
                    raw_window_handle,
                })
                .map_err(|e| format!("create_surface: {e}"))?
        };
        let caps = surface.get_capabilities(&self.adapter);
        if caps.formats.is_empty() || caps.alpha_modes.is_empty() {
            return Err("surface not supported by the adapter".into());
        }
        // Non-sRGB target: blend in sRGB space, like browsers and Qt, so
        // design colours land exactly where specified.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .unwrap_or(caps.formats[0]);
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else {
            caps.alpha_modes[0]
        };
        // Frame pacing is driven by wl_surface.frame callbacks, so the
        // swapchain itself must never block the event loop.
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w.max(1),
            height: h.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&self.device, &config);
        Ok(self.make_target(Some(surface), config))
    }

    /// A render target with no swapchain, read back to RAM (snapshots).
    pub fn create_offscreen(&mut self, w: u32, h: u32) -> SurfaceGfx {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        self.make_target(None, config)
    }

    fn make_target(&mut self, surface: Option<wgpu::Surface<'static>>, config: wgpu::SurfaceConfiguration) -> SurfaceGfx {
        let format = config.format;
        let shared = self.shared.get_or_insert_with(|| Shared::new(&self.device, &self.queue, format));
        let globals = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &shared.bind_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let text = TextRenderer::new(&mut shared.atlas, &self.device, wgpu::MultisampleState::default(), None);
        let viewport = Viewport::new(&self.device, &shared.glyph_cache);
        SurfaceGfx {
            surface,
            config,
            globals,
            bind,
            instances: None,
            image_instances: None,
            text,
            viewport,
        }
    }

    /// Drop textures right away (their window is gone without a last
    /// frame that could carry the frees).
    pub fn free_textures(&mut self, keys: &[u64]) {
        if let Some(shared) = self.shared.as_mut() {
            for k in keys {
                shared.textures.remove(k);
            }
        }
    }

    pub fn render(
        &mut self,
        sg: &mut SurfaceGfx,
        list: &DrawList,
        buffers: &HashMap<u64, glyphon::Buffer>,
        icons: &Icons,
    ) -> bool {
        // Reclaim the previous frame's command encoders before this frame
        // takes new ones (wgpu only does it after a submit). Otherwise wgpu
        // keeps two more encoders alive, each with 16 command buffers of
        // 80 KiB on radv.
        let _ = self.device.poll(wgpu::PollType::Poll);
        self.prepare(sg, list, buffers, icons);
        let Some(surface) = &sg.surface else { return false };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&self.device, &sg.config);
                return false;
            }
            other => {
                use std::io::Write;
                let _ = writeln!(std::io::stderr().lock(), "nimbus: acquire failed: {other:?}");
                return false;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let enc = self.encode(sg, list, &view);
        self.queue.submit(Some(enc.finish()));
        self.queue.present(frame);
        self.shared.as_mut().unwrap().atlas.trim();
        true
    }

    /// Render offscreen and return straight-alpha RGBA8 pixels.
    pub fn render_offscreen(
        &mut self,
        sg: &mut SurfaceGfx,
        list: &DrawList,
        buffers: &HashMap<u64, glyphon::Buffer>,
        icons: &Icons,
    ) -> Vec<u8> {
        let (w, h) = (sg.config.width, sg.config.height);
        self.prepare(sg, list, buffers, icons);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: sg.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self.encode(sg, list, &view);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("map");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let r = &data[(y * row) as usize..(y * row + w * 4) as usize];
            for px in r.chunks_exact(4) {
                let a = px[3] as f32 / 255.0;
                let un = |c: u8| if a > 0.0 { ((c as f32 / a).min(255.0)) as u8 } else { 0 };
                out.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), px[3]]);
            }
        }
        out
    }

    fn prepare(&mut self, sg: &mut SurfaceGfx, list: &DrawList, buffers: &HashMap<u64, glyphon::Buffer>, icons: &Icons) {
        let shared = self.shared.as_mut().unwrap();
        for k in &list.frees {
            shared.textures.remove(k);
        }
        if let Ok(mut o) = ORPHANS.try_lock()
            && !o.is_empty()
        {
            for k in o.drain(..) {
                shared.textures.remove(&k);
            }
        }
        for (k, img) in &list.uploads {
            let size = wgpu::Extent3d { width: img.w.max(1), height: img.h.max(1), depth_or_array_layers: 1 };
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("image"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                tex.as_image_copy(),
                &img.rgba,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(img.w * 4), rows_per_image: Some(img.h) },
                size,
            );
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &shared.tex_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&shared.sampler) },
                ],
            });
            shared.textures.insert(*k, (tex, bind));
        }
        if !list.images.is_empty() || !list.backdrop.is_empty() {
            let inst: Vec<ImageInst> = list
                .backdrop
                .iter()
                .chain(&list.images)
                .map(|i| ImageInst {
                    rect: i.rect,
                    uv: i.uv,
                    params: [i.radius, i.alpha, i.bright, i.smooth as u32 as f32],
                    reveal: i.reveal,
                    wipe: i.mask.map_or([0.0; 4], |m| m.1),
                })
                .collect();
            let bytes: &[u8] = bytemuck::cast_slice(&inst);
            if sg.image_instances.as_ref().is_none_or(|b| b.size() < bytes.len() as u64) {
                sg.image_instances = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("images"),
                    size: (bytes.len() as u64).next_power_of_two(),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            self.queue.write_buffer(sg.image_instances.as_ref().unwrap(), 0, bytes);
        }
        let (w, h) = (sg.config.width, sg.config.height);
        self.queue
            .write_buffer(&sg.globals, 0, bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]));

        // Instance buffer grows geometrically and is reused.
        let bytes: &[u8] = bytemuck::cast_slice(&list.shapes);
        let need = (bytes.len() as u64).max(160);
        if sg.instances.as_ref().is_none_or(|b| b.size() < need) {
            sg.instances = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shapes"),
                size: need.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(sg.instances.as_ref().unwrap(), 0, bytes);
        }

        // Text + icons.
        sg.viewport.update(&self.queue, Resolution { width: w, height: h });
        let custom: Vec<CustomGlyph> = list
            .icons
            .iter()
            .map(|i| CustomGlyph {
                id: i.id,
                left: i.x,
                top: i.y,
                width: i.size,
                height: i.size,
                color: Some(color(i.color)),
                snap_to_physical_pixel: true,
                metadata: 0,
            })
            .collect();
        // Scenes without text (the wallpaper backdrop) pass no buffers.
        let empty = buffers.get(&0);
        let full = TextBounds { left: 0, top: 0, right: w as i32, bottom: h as i32 };
        let areas = list
            .texts
            .iter()
            .filter_map(|t| {
                Some(TextArea {
                    buffer: buffers.get(&t.key)?,
                    left: t.x,
                    top: t.y,
                    scale: 1.0,
                    bounds: TextBounds { left: t.clip[0], top: t.clip[1], right: t.clip[2], bottom: t.clip[3] },
                    default_color: color(t.color),
                    custom_glyphs: &[],
                })
            })
            .chain(empty.map(|empty| TextArea {
                buffer: empty,
                left: 0.0,
                top: 0.0,
                scale: 1.0,
                bounds: full,
                default_color: Color::rgb(255, 255, 255),
                custom_glyphs: &custom,
            }));
        let mut raster = |r| icons.rasterize(r);
        if let Err(e) = sg.text.prepare_with_custom(
            &self.device,
            &self.queue,
            &mut self.fonts,
            &mut shared.atlas,
            &sg.viewport,
            areas,
            &mut self.swash,
            &mut raster,
        ) {
            eprintln!("nimbus: text prepare: {e}");
        }
    }

    fn encode(&mut self, sg: &mut SurfaceGfx, list: &DrawList, view: &wgpu::TextureView) -> wgpu::CommandEncoder {
        let shared = self.shared.as_mut().unwrap();
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // One draw per image: each has its own texture binding.
            let draw_images = |pass: &mut wgpu::RenderPass<'_>, imgs: &[ImageDraw], base: usize| {
                if imgs.is_empty() {
                    return;
                }
                pass.set_pipeline(&shared.image_pipeline);
                pass.set_bind_group(0, &sg.bind, &[]);
                pass.set_vertex_buffer(0, sg.image_instances.as_ref().unwrap().slice(..));
                let mut wiping = false;
                for (n, img) in imgs.iter().enumerate() {
                    let Some((_, bind)) = shared.textures.get(&img.key) else { continue };
                    let mask = img.mask.and_then(|(k, _)| shared.textures.get(&k));
                    if mask.is_some() != wiping {
                        wiping = mask.is_some();
                        pass.set_pipeline(if wiping { &shared.wipe_pipeline } else { &shared.image_pipeline });
                    }
                    if let Some((_, m)) = mask {
                        pass.set_bind_group(2, m, &[]);
                    }
                    let n = (base + n) as u32;
                    pass.set_bind_group(1, bind, &[]);
                    pass.draw(0..4, n..n + 1);
                }
            };
            draw_images(&mut pass, &list.backdrop, 0);
            if !list.shapes.is_empty() {
                let n = std::mem::size_of_val(list.shapes.as_slice()) as u64;
                pass.set_pipeline(&shared.pipeline);
                pass.set_bind_group(0, &sg.bind, &[]);
                pass.set_vertex_buffer(0, sg.instances.as_ref().unwrap().slice(..n));
                pass.draw(0..4, 0..list.shapes.len() as u32);
            }
            draw_images(&mut pass, &list.images, list.backdrop.len());
            if let Err(e) = sg.text.render(&shared.atlas, &sg.viewport, &mut pass) {
                eprintln!("nimbus: text render: {e}");
            }
        }
        enc
    }

    pub fn resize(&self, sg: &mut SurfaceGfx, w: u32, h: u32) {
        if sg.config.width != w || sg.config.height != h {
            sg.config.width = w.max(1);
            sg.config.height = h.max(1);
            if let Some(surface) = &sg.surface {
                surface.configure(&self.device, &sg.config);
            }
        }
    }
}

impl Shared {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shape"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shape.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shape"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Shape>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &SHAPE_ATTRS,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image"),
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
        let image_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });
        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &image_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImageInst>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &IMAGE_ATTRS,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let wipe_pipeline = wipe_pipeline(device, format, &bind_layout, &tex_layout);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let glyph_cache = Cache::new(device);
        let color_mode = if format.is_srgb() { ColorMode::Accurate } else { ColorMode::Web };
        let atlas = TextAtlas::with_color_mode(device, queue, &glyph_cache, format, color_mode);
        Self { pipeline, bind_layout, image_pipeline, wipe_pipeline, tex_layout, sampler, textures: HashMap::new(), glyph_cache, atlas }
    }
}

/// Images masked by an order map (`ImageDraw::mask`): the image pipeline
/// plus the map as a second texture group.
fn wipe_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat, globals: &wgpu::BindGroupLayout, tex: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("wipe"),
        source: wgpu::ShaderSource::Wgsl(include_str!("wipe.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(globals), Some(tex), Some(tex)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("wipe"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ImageInst>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &WIPE_ATTRS,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Point the Vulkan loader at the drivers for the GPUs actually present.
/// Left alone it loads and initialises every installed ICD, lavapipe and
/// its LLVM state included: ~6 MiB of private memory and ~30 ms of startup
/// for drivers we never use. Skipped when the user picks drivers
/// themselves or the kernel driver is unknown; returns whether it set one.
/// Make sure `libvulkan.so.1` (which wgpu dlopens by soname) can be found
/// even when the caller's environment doesn't point at the loader: niri,
/// swayidle and systemd units usually don't set LD_LIBRARY_PATH, and on
/// NixOS /run/opengl-driver/lib holds only the drivers. Loading it once by
/// absolute path makes the later soname lookup resolve to it.
fn preload_vulkan_loader() {
    use std::ffi::CString;
    const SONAME: &str = "libvulkan.so.1";
    let open = |p: &str| {
        let Ok(c) = CString::new(p) else { return false };
        !unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) }.is_null()
    };
    if open(SONAME) {
        return;
    }
    let mut dirs: Vec<std::path::PathBuf> = vec![];
    // Baked in at build time (build.rs): the loader the binary was built with.
    if let Some(d) = option_env!("NIMBUS_VULKAN_LIB_DIRS") {
        dirs.extend(d.split(':').filter(|s| !s.is_empty()).map(Into::into));
    }
    let user = std::env::var("USER").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    for d in [
        "/run/opengl-driver/lib".to_string(),
        "/run/current-system/sw/lib".to_string(),
        format!("/etc/profiles/per-user/{user}/lib"),
        format!("{home}/.nix-profile/lib"),
        "/usr/lib".to_string(),
        "/usr/lib64".to_string(),
        "/usr/lib/x86_64-linux-gnu".to_string(),
    ] {
        dirs.push(d.into());
    }
    for d in &dirs {
        let p = d.join(SONAME);
        if p.exists() && open(&p.to_string_lossy()) {
            return;
        }
    }
    // Last resort on NixOS: the newest vulkan-loader in the store.
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir("/nix/store")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.len() > 33 && n[33..].starts_with("vulkan-loader-") && !n.ends_with(".drv")))
        .map(|e| e.path().join("lib").join(SONAME))
        .filter(|p| p.exists())
        .collect();
    found.sort_by_key(|p| p.parent().and_then(|d| d.parent()).and_then(|d| d.file_name()).map(|n| n.to_string_lossy()[33..].to_string()));
    for p in found.iter().rev() {
        if open(&p.to_string_lossy()) {
            return;
        }
    }
}

fn select_vulkan_drivers() -> bool {
    const USER: [&str; 5] =
        ["VK_LOADER_DRIVERS_SELECT", "VK_LOADER_DRIVERS_DISABLE", "VK_DRIVER_FILES", "VK_ICD_FILENAMES", "VK_ADD_DRIVER_FILES"];
    if USER.iter().any(|v| std::env::var_os(v).is_some()) {
        return false;
    }
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else { return false };
    let mut globs: Vec<&str> = vec![];
    for e in dir.flatten() {
        if !e.file_name().to_string_lossy().starts_with("renderD") {
            continue;
        }
        let Ok(driver) = std::fs::read_link(e.path().join("device/driver")) else { return false };
        // Kernel driver → ICD manifest file name (radeon_icd.x86_64.json, …).
        let glob = match driver.file_name().and_then(|n| n.to_str()).unwrap_or("") {
            "amdgpu" | "radeon" => "*radeon*,*amd*",
            "i915" | "xe" => "*intel*",
            "nouveau" => "*nouveau*",
            "nvidia" => "*nvidia*",
            "virtio_gpu" => "*virtio*",
            "msm" => "*freedreno*",
            "asahi" => "*asahi*",
            "panfrost" | "panthor" => "*panfrost*",
            "v3d" | "vc4" => "*broadcom*",
            "powervr" => "*powervr*",
            _ => return false,
        };
        if !globs.contains(&glob) {
            globs.push(glob);
        }
    }
    if globs.is_empty() {
        return false;
    }
    // Still single-threaded: Gpu::new runs before any thread is spawned.
    unsafe { std::env::set_var("VK_LOADER_DRIVERS_SELECT", globs.join(",")) };
    true
}

pub struct SurfaceGfx {
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    globals: wgpu::Buffer,
    bind: wgpu::BindGroup,
    instances: Option<wgpu::Buffer>,
    image_instances: Option<wgpu::Buffer>,
    text: TextRenderer,
    viewport: Viewport,
}

fn color(c: Rgba) -> Color {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color::rgba(q(c.0), q(c.1), q(c.2), q(c.3))
}
