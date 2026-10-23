//! wgpu presentation for the canvas: one device, created on a background
//! thread after the window's first frame, rendering straight into the canvas
//! HWND. Two pipelines: `photo` draws a photograph through its crop, quarter
//! turns and straightening angle (and dims outside the crop while cropping);
//! `quad` draws rectangles, rounded rectangles, textures and text. Both end
//! in `present`, which converts to the monitor's ICC profile when one is in
//! use (docs/windows-ui-features.md §6).

use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};
use wgpu::util::DeviceExt;

pub const LUT_SIZE: u32 = 4096;
const PHOTO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const SHADER: &str = r#"
struct Globals {
    viewport: vec4f,
    cm0: vec4f,
    cm1: vec4f,
    cm2: vec4f,
    // x: colour management on
    flags: vec4f,
};
@group(1) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(1) var lut: texture_2d<f32>;
@group(1) @binding(2) var lut_sampler: sampler;

@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

fn encode_srgb(c: vec3f) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0));
    return select(1.055 * pow(x, vec3f(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3f(0.0031308));
}

fn decode_srgb(c: vec3f) -> vec3f {
    return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

// Linear sRGB to the display's encoding: sRGB, or the monitor profile
// through its matrix and the inverse of its tone curves. The LUT is indexed
// by the sRGB encoding so the shadows get as many entries as the highlights.
fn present(linear: vec3f) -> vec3f {
    if (globals.flags.x < 0.5) {
        return encode_srgb(linear);
    }
    let m = vec3f(dot(globals.cm0.xyz, linear), dot(globals.cm1.xyz, linear), dot(globals.cm2.xyz, linear));
    let u = (encode_srgb(m) * f32(4095) + 0.5) / f32(4096);
    return vec3f(
        textureSampleLevel(lut, lut_sampler, vec2f(u.r, 0.5), 0.0).r,
        textureSampleLevel(lut, lut_sampler, vec2f(u.g, 0.5), 0.0).g,
        textureSampleLevel(lut, lut_sampler, vec2f(u.b, 0.5), 0.0).b,
    );
}

fn to_clip(p: vec2f) -> vec4f {
    return vec4f(p.x / globals.viewport.x * 2.0 - 1.0, 1.0 - p.y / globals.viewport.y * 2.0, 0.0, 1.0);
}

// MARK: Photo

struct Photo {
    // origin of the turned frame on screen (px), screen px per photo px, angle (radians, clockwise)
    origin_scale: vec4f,
    // shown size (after quarter turns), quarter turns, mode (0 normal, 1 cropping)
    shown: vec4f,
    // the crop in the turned frame (photo px)
    clip: vec4f,
    // the quad drawn, on screen
    quad: vec4f,
    // x: dimming outside the crop while cropping
    extra: vec4f,
};
@group(2) @binding(0) var<uniform> photo: Photo;

struct PhotoOut {
    @builtin(position) position: vec4f,
};

@vertex
fn photo_vs(@builtin(vertex_index) index: u32) -> PhotoOut {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    var out: PhotoOut;
    out.position = to_clip(photo.quad.xy + corner * photo.quad.zw);
    return out;
}

@fragment
fn photo_fs(in: PhotoOut) -> @location(0) vec4f {
    let scale = photo.origin_scale.z;
    let angle = photo.origin_scale.w;
    let size = photo.shown.xy;
    let t = (in.position.xy - photo.origin_scale.xy) / scale;
    // Turn back by the angle about the centre (y down, clockwise positive).
    let c = size * 0.5;
    let d = t - c;
    let s = sin(angle);
    let k = cos(angle);
    let g = c + vec2f(d.x * k + d.y * s, -d.x * s + d.y * k);
    var alpha = 1.0;
    if (photo.shown.w > 0.5) {
        // Cropping: the turned photograph's edge, antialiased.
        let inside = min(min(g.x, size.x - g.x), min(g.y, size.y - g.y)) * scale;
        alpha = clamp(inside + 0.5, 0.0, 1.0);
        if (alpha <= 0.0) {
            discard;
        }
    }
    // Shown fraction back through the quarter turns to the unrotated texture.
    var f = g / size;
    let turns = u32(photo.shown.z + 0.5);
    for (var i = 0u; i < turns; i++) {
        f = vec2f(f.y, 1.0 - f.x);
    }
    var rgb = textureSample(image, image_sampler, f).rgb;
    if (photo.shown.w > 0.5) {
        let clip = photo.clip;
        let outside = t.x < clip.x || t.y < clip.y || t.x > clip.x + clip.z || t.y > clip.y + clip.w;
        if (outside) {
            rgb = rgb * photo.extra.x;
        }
    }
    return vec4f(present(rgb), alpha);
}

// MARK: Quads

struct Quad {
    @location(0) rect: vec4f,
    @location(1) uv: vec4f,
    // sRGB-encoded colour and alpha
    @location(2) color: vec4f,
    // x: corner radius, y: stroke width (0 fills), z: flags (1 textured, 2 turned anticlockwise, 4 coverage mask)
    @location(3) params: vec4f,
};

struct QuadOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) local: vec2f,
    @location(2) size: vec2f,
    @location(3) color: vec4f,
    @location(4) params: vec4f,
};

@vertex
fn quad_vs(@builtin(vertex_index) index: u32, quad: Quad) -> QuadOut {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    var out: QuadOut;
    out.position = to_clip(quad.rect.xy + corner * quad.rect.zw);
    let flags = u32(quad.params.z + 0.5);
    var t = corner;
    if ((flags & 2u) != 0u) {
        t = vec2f(1.0 - corner.y, corner.x);
    }
    out.uv = mix(quad.uv.xy, quad.uv.zw, t);
    out.local = corner * quad.rect.zw;
    out.size = quad.rect.zw;
    out.color = quad.color;
    out.params = quad.params;
    return out;
}

fn rounded_box(p: vec2f, half: vec2f, radius: f32) -> f32 {
    let q = abs(p) - half + radius;
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn quad_fs(in: QuadOut) -> @location(0) vec4f {
    let flags = u32(in.params.z + 0.5);
    var color = vec4f(decode_srgb(in.color.rgb), in.color.a);
    if ((flags & 1u) != 0u) {
        let texel = textureSample(image, image_sampler, in.uv);
        if ((flags & 4u) != 0u) {
            color.a = color.a * texel.r;
        } else {
            color = color * texel;
        }
    }
    let radius = in.params.x;
    let stroke = in.params.y;
    if (radius > 0.0 || stroke > 0.0) {
        let d = rounded_box(in.local - in.size * 0.5, in.size * 0.5, radius);
        var coverage = clamp(0.5 - d, 0.0, 1.0);
        if (stroke > 0.0) {
            coverage = coverage * clamp(d + stroke + 0.5, 0.0, 1.0);
        }
        color.a = color.a * coverage;
    }
    return vec4f(present(color.rgb), color.a);
}

// MARK: Mipmaps

struct MipOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn mip_vs(@builtin(vertex_index) index: u32) -> MipOut {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    var out: MipOut;
    out.position = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn mip_fs(in: MipOut) -> @location(0) vec4f {
    return textureSampleLevel(image, image_sampler, in.uv, 0.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct Globals {
    pub viewport: [f32; 4],
    pub cm: [[f32; 4]; 3],
    pub flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct PhotoUniform {
    pub origin_scale: [f32; 4],
    pub shown: [f32; 4],
    pub clip: [f32; 4],
    pub quad: [f32; 4],
    pub extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct QuadInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

pub const TEXTURED: f32 = 1.0;
pub const TURNED: f32 = 2.0;
pub const MASK: f32 = 4.0;

impl QuadInstance {
    pub fn fill(rect: [f32; 4], color: [f32; 4]) -> Self {
        Self { rect, uv: [0.0, 0.0, 1.0, 1.0], color, params: [0.0; 4] }
    }
    pub fn rounded(rect: [f32; 4], color: [f32; 4], radius: f32, stroke: f32) -> Self {
        Self { rect, uv: [0.0, 0.0, 1.0, 1.0], color, params: [radius, stroke, 0.0, 0.0] }
    }
    pub fn image(rect: [f32; 4], uv: [f32; 4], turned: bool) -> Self {
        Self { rect, uv, color: [1.0; 4], params: [0.0, 0.0, TEXTURED + if turned { TURNED } else { 0.0 }, 0.0] }
    }
    pub fn text(rect: [f32; 4], color: [f32; 4]) -> Self {
        Self { rect, uv: [0.0, 0.0, 1.0, 1.0], color, params: [0.0, 0.0, TEXTURED + MASK, 0.0] }
    }
}

/// A texture ready to draw.
pub struct GpuImage {
    pub texture: wgpu::Texture,
    pub bind_group: wgpu::BindGroup,
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub format: wgpu::TextureFormat,
    image_layout: wgpu::BindGroupLayout,
    pub globals_layout: wgpu::BindGroupLayout,
    photo_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pub lut_sampler: wgpu::Sampler,
    mip_pipeline: wgpu::RenderPipeline,
    mip_sampler: wgpu::Sampler,
    photo_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    white: OnceLock<GpuImage>,
}

static SHARED: OnceLock<Arc<Gpu>> = OnceLock::new();

pub fn shared() -> Option<&'static Arc<Gpu>> {
    SHARED.get()
}

/// Creates the Vulkan device and a surface for `hwnd`.
pub fn init(hwnd: isize) -> Result<(Arc<Gpu>, wgpu::Surface<'static>)> {
    let mut last_error = anyhow!("No graphics adapter");
    for backends in [wgpu::Backends::VULKAN] {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends, ..Default::default() });
        let handle = Win32WindowHandle::new(std::num::NonZeroIsize::new(hwnd).context("No canvas window")?);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
                raw_window_handle: RawWindowHandle::Win32(handle),
            })
        };
        let surface = match surface {
            Ok(surface) => surface,
            Err(error) => {
                last_error = error.into();
                continue;
            }
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }));
        let Some(adapter) = adapter else { continue };
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("negs canvas"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits { max_texture_dimension_2d: 8192, ..wgpu::Limits::downlevel_defaults() }
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        ))?;
        let caps = surface.get_capabilities(&adapter);
        let format = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .or_else(|| caps.formats.first().copied())
            .context("The canvas has no usable surface format")?;
        let gpu = Arc::new(Gpu::new(instance, adapter, device, queue, format));
        let _ = SHARED.set(gpu.clone());
        return Ok((gpu, surface));
    }
    Err(last_error)
}

impl Gpu {
    fn new(instance: wgpu::Instance, adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let info = adapter.get_info();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("canvas"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let uniform_entry = |binding, dynamic| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: None },
            count: None,
        };
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[uniform_entry(0, false), texture_entry(1), sampler_entry(2)],
        });
        let photo_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photo"),
            entries: &[uniform_entry(0, true)],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("trilinear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let lut_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lut"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mip_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mip"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let pipeline = |label, layouts: &[&wgpu::BindGroupLayout], vs, fs, buffers: &[wgpu::VertexBufferLayout], target, blend| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: layouts, push_constant_ranges: &[] });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: target, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let quad_buffer = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
        };
        let photo_pipeline = pipeline("photo", &[&image_layout, &globals_layout, &photo_layout], "photo_vs", "photo_fs", &[], format, blend);
        let quad_pipeline = pipeline("quad", &[&image_layout, &globals_layout], "quad_vs", "quad_fs", &[quad_buffer], format, blend);
        let mip_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mip"), bind_group_layouts: &[&image_layout], push_constant_ranges: &[] });
        let mip_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mip"),
            layout: Some(&mip_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("mip_vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("mip_fs"),
                compilation_options: Default::default(),
                targets: &[Some(PHOTO_FORMAT.into())],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            instance,
            adapter,
            device,
            queue,
            info,
            format,
            image_layout,
            globals_layout,
            photo_layout,
            sampler,
            lut_sampler,
            mip_pipeline,
            mip_sampler,
            photo_pipeline,
            quad_pipeline,
            white: OnceLock::new(),
        }
    }

    fn bind_image(&self, view: &wgpu::TextureView, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.image_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// Uploads sRGB RGBA pixels with a full mip chain, generated on the GPU.
    pub fn upload(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
        let levels = 32 - width.max(height).leading_zeros();
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("picture"),
            size,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PHOTO_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            size,
        );
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mips") });
        for level in 1..levels {
            let source = texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level - 1, mip_level_count: Some(1), ..Default::default() });
            let target = texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() });
            let bind = self.bind_image(&source, &self.mip_sampler);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.mip_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        let view = texture.create_view(&Default::default());
        let bind_group = self.bind_image(&view, &self.sampler);
        GpuImage { texture, bind_group }
    }

    /// A one-channel coverage mask (text).
    pub fn upload_mask(&self, coverage: &[u8], width: u32, height: u32) -> GpuImage {
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("text"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            coverage,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.bind_image(&view, &self.lut_sampler);
        GpuImage { texture, bind_group }
    }

    pub fn white(&self) -> &GpuImage {
        self.white.get_or_init(|| self.upload_mask(&[255], 1, 1))
    }

    /// The colour-management LUT: one row of `LUT_SIZE` RGBA half floats.
    pub fn lut_texture(&self, lut: &[[f32; 3]]) -> wgpu::TextureView {
        let data: Vec<u16> = lut.iter().flat_map(|c| [half(c[0]), half(c[1]), half(c[2]), half(1.0)]).collect();
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("lut"),
                size: wgpu::Extent3d { width: lut.len() as u32, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&data),
        );
        texture.create_view(&Default::default())
    }

    pub fn globals_bind_group(&self, buffer: &wgpu::Buffer, lut: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &self.globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(lut) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.lut_sampler) },
            ],
        })
    }
}

/// One thing to draw, in order.
pub enum Item<'a> {
    Photo(&'a GpuImage, PhotoUniform),
    Quad(Option<&'a GpuImage>, QuadInstance),
}

/// Draws `items` over `clear` into the surface texture.
pub fn draw(gpu: &Gpu, target: &wgpu::TextureView, globals: &wgpu::BindGroup, clear: [f32; 3], items: &[Item]) {
    let align = gpu.device.limits().min_uniform_buffer_offset_alignment.max(256) as usize;
    let photos: Vec<&PhotoUniform> = items.iter().filter_map(|i| if let Item::Photo(_, u) = i { Some(u) } else { None }).collect();
    let mut uniform_bytes = vec![0u8; photos.len().max(1) * align];
    for (i, u) in photos.iter().enumerate() {
        let bytes = bytemuck::bytes_of(*u);
        uniform_bytes[i * align..i * align + bytes.len()].copy_from_slice(bytes);
    }
    let uniforms = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("photo uniforms"),
        contents: &uniform_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let photo_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &gpu.photo_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &uniforms,
                offset: 0,
                size: wgpu::BufferSize::new(std::mem::size_of::<PhotoUniform>() as u64),
            }),
        }],
    });
    let quads: Vec<QuadInstance> = items.iter().filter_map(|i| if let Item::Quad(_, q) = i { Some(*q) } else { None }).collect();
    let instances = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("quads"),
        contents: if quads.is_empty() { &[0u8; 64] } else { bytemuck::cast_slice(&quads) },
        usage: wgpu::BufferUsages::VERTEX,
    });
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("canvas"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: clear[0] as f64, g: clear[1] as f64, b: clear[2] as f64, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(1, globals, &[]);
        let (mut photo_index, mut quad_index) = (0u32, 0u32);
        let white = gpu.white();
        let mut i = 0;
        while i < items.len() {
            match &items[i] {
                Item::Photo(image, _) => {
                    pass.set_pipeline(&gpu.photo_pipeline);
                    pass.set_bind_group(0, &image.bind_group, &[]);
                    pass.set_bind_group(2, &photo_group, &[photo_index * align as u32]);
                    pass.draw(0..4, 0..1);
                    photo_index += 1;
                    i += 1;
                }
                Item::Quad(image, _) => {
                    // A run of quads with the same texture is one draw.
                    let start = quad_index;
                    let key = image.map(|g| g as *const GpuImage);
                    while i < items.len() {
                        match &items[i] {
                            Item::Quad(other, _) if other.map(|g| g as *const GpuImage) == key => {
                                quad_index += 1;
                                i += 1;
                            }
                            _ => break,
                        }
                    }
                    pass.set_pipeline(&gpu.quad_pipeline);
                    pass.set_bind_group(0, &image.unwrap_or(white).bind_group, &[]);
                    pass.set_vertex_buffer(0, instances.slice(..));
                    pass.draw(0..4, start..quad_index);
                }
            }
        }
    }
    gpu.queue.submit(Some(encoder.finish()));
}

/// f32 to IEEE half, rounding to nearest (for the LUT).
fn half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        let m = (mantissa | 0x80_0000) >> (1 - exponent);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    if exponent >= 31 {
        return sign | 0x7c00;
    }
    let rounded = ((exponent as u32) << 10 | mantissa >> 13) + ((mantissa >> 12) & 1);
    sign | rounded as u16
}

#[cfg(test)]
mod tests {
    use super::half;

    #[test]
    fn halves() {
        assert_eq!(half(0.0), 0);
        assert_eq!(half(1.0), 0x3c00);
        assert_eq!(half(0.5), 0x3800);
        assert_eq!(half(-2.0), 0xc000);
        assert_eq!(half(0.33325195), 0x3555);
    }
}
