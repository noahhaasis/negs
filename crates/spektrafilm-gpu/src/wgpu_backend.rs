/// wgpu compute backend — dispatches WGSL shaders on GPU via Metal/Vulkan.

#[cfg(feature = "wgpu-backend")]
use std::borrow::Cow;

#[cfg(feature = "wgpu-backend")]
use spektrafilm_math::image::ImageBuf;

#[cfg(feature = "wgpu-backend")]
use crate::{ComputeBackend, Lut3D, cpu_backend};

/// Borrow a `&[Scalar]` as `&[f32]` for GPU upload.
/// Zero-copy in f32 mode; allocates a converted Vec in f64 mode.
#[cfg(all(feature = "wgpu-backend", not(feature = "precision-f64")))]
#[inline]
fn scalars_to_f32(v: &[spektrafilm_math::precision::Scalar]) -> std::borrow::Cow<'_, [f32]> {
    std::borrow::Cow::Borrowed(v)
}

/// Default `WORKGROUP_SIZE` of the flat per-pixel shaders (one thread per
/// pixel or value, `idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x`). Each
/// pipeline's actual size is [`WgpuBackend::workgroup_size`].
#[cfg(all(feature = "wgpu-backend", test))]
const LINEAR_WORKGROUP_SIZE: u32 = 1024;

/// Default `WORKGROUP_SIZE` of grain.wgsl.
#[cfg(all(feature = "wgpu-backend", test))]
const GRAIN_WORKGROUP_SIZE: u32 = 256;

/// WebGPU's default (and Metal's actual) limit on workgroups per dispatch
/// dimension.
#[cfg(feature = "wgpu-backend")]
const MAX_WORKGROUPS_PER_DIMENSION: u32 = 65535;

#[cfg(feature = "wgpu-backend")]
const SPECTRAL_GUARD_WGSL: &str = include_str!("../../spektrafilm-shaders/wgsl/spectral_guard.wgsl");

/// `spectral_guard.wgsl`'s uniform: the front pass's RGB → adapted XYZ matrix
/// and its inverse, as std140 columns.
#[cfg(feature = "wgpu-backend")]
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpectralGuardParams {
    n_pixels: u32,
    _pad: [u32; 3],
    rgb_to_xyz: [[f32; 4]; 3],
    xyz_to_rgb: [[f32; 4]; 3],
}

#[cfg(feature = "wgpu-backend")]
impl SpectralGuardParams {
    fn new(n_pixels: u32, rgb_to_xyz: &[[f64; 3]; 3]) -> Self {
        let columns = |m: &[[f64; 3]; 3]| {
            std::array::from_fn(|j| [m[0][j] as f32, m[1][j] as f32, m[2][j] as f32, 0.0])
        };
        Self {
            n_pixels,
            _pad: [0; 3],
            rgb_to_xyz: columns(rgb_to_xyz),
            xyz_to_rgb: columns(&spektrafilm_math::colorspace::mat3_inverse(rgb_to_xyz)),
        }
    }
}

/// Workgroup grid for `n_threads` threads in groups of `workgroup_size`.
/// Wraps into y so neither dimension exceeds the 65535 limit: 1024-thread
/// groups in x alone stop at 67.1 M threads, 256-thread groups at 16.7 M.
/// The grid can overshoot `n_threads` by up to one row; shaders guard with
/// `idx >= n`.
#[cfg(feature = "wgpu-backend")]
fn linear_dispatch_grid(n_threads: u32, workgroup_size: u32) -> (u32, u32) {
    linear_dispatch_grid_capped(n_threads, workgroup_size, MAX_WORKGROUPS_PER_DIMENSION)
}

/// [`linear_dispatch_grid`] with an explicit per-dimension cap, so tests can
/// force the wrapped (y > 1) layout at small sizes.
#[cfg(feature = "wgpu-backend")]
fn linear_dispatch_grid_capped(n_threads: u32, workgroup_size: u32, max_per_dim: u32) -> (u32, u32) {
    let groups = n_threads.div_ceil(workgroup_size).max(1);
    let y = groups.div_ceil(max_per_dim);
    (groups.div_ceil(y), y)
}

/// Shaders built without naga's loop bounding, which makes every loop
/// iteration touch a volatile local; it cost the blurs about a fifth of
/// their time. Their loops all have finite bounds.
#[cfg(feature = "wgpu-backend")]
fn relaxed_loop_bounding(shader_source: &str) -> bool {
    [
        include_str!("../../spektrafilm-shaders/wgsl/ref_blur_fir_rows.wgsl"),
        include_str!("../../spektrafilm-shaders/wgsl/ref_blur_fir_cols.wgsl"),
        include_str!("../../spektrafilm-shaders/wgsl/ref_blur_iir_rows.wgsl"),
        include_str!("../../spektrafilm-shaders/wgsl/ref_blur_iir_cols.wgsl"),
    ]
    .iter()
    .any(|s| std::ptr::eq(s.as_ptr(), shader_source.as_ptr()))
}

/// Dispatch a flat per-pixel shader over `n_threads` threads in groups of
/// the pipeline's `workgroup_size`.
#[cfg(feature = "wgpu-backend")]
fn dispatch_linear(pass: &mut wgpu::ComputePass<'_>, n_threads: u32, workgroup_size: u32) {
    let (x, y) = linear_dispatch_grid(n_threads, workgroup_size);
    pass.dispatch_workgroups(x, y, 1);
}

/// The default of a shader's `override WORKGROUP_SIZE: u32 = N;`, or `None`
/// for shaders with a fixed `@workgroup_size`.
#[cfg(feature = "wgpu-backend")]
fn declared_workgroup_size(shader_source: &str) -> Option<u32> {
    let rest = shader_source.split_once("override WORKGROUP_SIZE: u32 = ")?.1;
    rest.split_once("u;")?.0.parse().ok()
}

/// Threads per workgroup of a fixed `@workgroup_size(x[, y[, z]])`.
#[cfg(all(feature = "wgpu-backend", target_os = "macos"))]
fn fixed_workgroup_threads(shader_source: &str) -> Option<u32> {
    let dims = shader_source.split_once("@workgroup_size(")?.1.split_once(')')?.0;
    dims.split(',').map(|d| d.trim().parse::<u32>().ok()).product()
}

/// `maxTotalThreadsPerThreadgroup` of compiled Metal pipelines. It depends
/// on the GPU and on each shader's register use, and Metal drops a larger
/// dispatch without an error (grain.wgsl hit this on Apple GPUs at 1024).
#[cfg(all(feature = "wgpu-backend", target_os = "macos"))]
mod metal_threadgroups {
    use wgpu::naga;

    pub(super) fn raw_device(device: &wgpu::Device) -> Option<metal::Device> {
        // SAFETY: only clones the retained MTLDevice handle out of wgpu.
        unsafe {
            device.as_hal::<wgpu::hal::api::Metal, _, _>(|d| d.map(|d| d.raw_device().lock().clone()))
        }
    }

    /// Apple-designed GPUs; Intel and AMD GPUs in Macs are only `Mac2`.
    pub(super) fn is_apple_gpu(device: &metal::Device) -> bool {
        device.supports_family(metal::MTLGPUFamily::Apple1)
    }

    /// The limit of the pipeline wgpu 24's Metal backend builds for
    /// `shader_source`'s `main` with one bind group of `entries` and
    /// `WORKGROUP_SIZE` set to `workgroup_size`. Mirrors wgpu-hal's
    /// `load_shader`, `create_pipeline_layout` and `create_compute_pipeline`:
    /// the same MSL (version, bounds checks, binding slots, sizes buffer) and
    /// the same pipeline descriptor (immutable read-only buffers), since a
    /// different compilation may allow more threads than the real pipeline.
    pub(super) fn max_threads(
        device: &metal::Device,
        shader_source: &str,
        entries: &[wgpu::BindGroupLayoutEntry],
        workgroup_size: Option<u32>,
    ) -> Result<u32, String> {
        let module = naga::front::wgsl::parse_str(shader_source).map_err(|e| e.to_string())?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .map_err(|e| e.to_string())?;
        let constants: std::collections::HashMap<String, f64> = workgroup_size
            .map(|size| ("WORKGROUP_SIZE".to_string(), size as f64))
            .into_iter()
            .collect();
        let (module, info) =
            naga::back::pipeline_constants::process_overrides(&module, &info, &constants)
                .map_err(|e| e.to_string())?;

        // Buffer slots in layout order; storage buffers get a sizes buffer.
        let mut resources = naga::back::msl::EntryPointResources::default();
        let mut slot = 0u8;
        let mut needs_sizes = false;
        for entry in entries {
            let wgpu::BindingType::Buffer { ty, .. } = entry.ty else {
                return Err("only buffer bindings are mirrored".into());
            };
            let mutable = matches!(ty, wgpu::BufferBindingType::Storage { read_only: false });
            needs_sizes |= matches!(ty, wgpu::BufferBindingType::Storage { .. });
            let target = naga::back::msl::BindTarget { buffer: Some(slot), mutable, ..Default::default() };
            resources.resources.insert(naga::ResourceBinding { group: 0, binding: entry.binding }, target);
            slot += 1;
        }
        resources.sizes_buffer = needs_sizes.then_some(slot);

        // Buffers the entry point uses but never stores to are immutable.
        let ep_info = info.get_entry_point(0);
        let mut immutable_slots = Vec::new();
        for (handle, var) in module.global_variables.iter() {
            let stores = match var.space {
                naga::AddressSpace::Uniform => false,
                naga::AddressSpace::Storage { access } => access.contains(naga::StorageAccess::STORE),
                _ => continue,
            };
            let Some(binding) = &var.binding else { continue };
            if !ep_info[handle].is_empty() && !stores {
                let target = resources.resources.get(binding).ok_or("binding missing from layout")?;
                immutable_slots.extend(target.buffer);
            }
        }

        let checks = naga::proc::BoundsCheckPolicy::Restrict;
        let options = naga::back::msl::Options {
            // wgpu-hal uses MSL 3.1 from macOS 14, the oldest we deploy to.
            lang_version: (3, 1),
            per_entry_point_map: [("main".to_string(), resources)].into(),
            bounds_check_policies: naga::proc::BoundsCheckPolicies {
                index: checks,
                buffer: checks,
                image_load: checks,
                binding_array: naga::proc::BoundsCheckPolicy::Unchecked,
            },
            zero_initialize_workgroup_memory: false, // as in `build_pipeline`
            force_loop_bounding: !super::relaxed_loop_bounding(shader_source),
            fake_missing_bindings: false,
            ..Default::default()
        };
        let pipeline_options = naga::back::msl::PipelineOptions {
            vertex_pulling_transform: true,
            ..Default::default()
        };
        let (msl, translation) =
            naga::back::msl::write_string(&module, &info, &options, &pipeline_options)
                .map_err(|e| e.to_string())?;
        let entry = translation
            .entry_point_names
            .first()
            .and_then(|name| name.as_ref().ok())
            .ok_or("no entry point")?;
        let compile = metal::CompileOptions::new();
        compile.set_language_version(metal::MTLLanguageVersion::V3_1);
        compile.set_preserve_invariance(true);
        let library = device.new_library_with_source(&msl, &compile)?;
        let function = library.get_function(entry, None)?;
        let descriptor = metal::ComputePipelineDescriptor::new();
        descriptor.set_compute_function(Some(&function));
        let buffers = descriptor.buffers().ok_or("no buffer descriptors")?;
        for slot in immutable_slots {
            let buffer = buffers.object_at(slot as u64).ok_or("no buffer descriptor")?;
            buffer.set_mutability(metal::MTLMutability::Immutable);
        }
        let pipeline = device.new_compute_pipeline_state(&descriptor)?;
        Ok(pipeline.max_total_threads_per_threadgroup() as u32)
    }
}

#[cfg(all(feature = "wgpu-backend", feature = "precision-f64"))]
#[inline]
fn scalars_to_f32(v: &[spektrafilm_math::precision::Scalar]) -> std::borrow::Cow<'static, [f32]> {
    std::borrow::Cow::Owned(v.iter().map(|&s| s as f32).collect())
}

/// Convert a `Vec<f32>` from GPU readback into `Vec<Scalar>`.
/// Zero-copy (identity) in f32 mode; allocates in f64 mode.
#[cfg(all(feature = "wgpu-backend", not(feature = "precision-f64")))]
#[inline]
fn f32_to_scalars(v: Vec<f32>) -> Vec<spektrafilm_math::precision::Scalar> {
    v
}

/// Sanitize NaN values in spectral inputs before GPU upload.
///
/// **Why this exists**: profile JSON has NaN values for `channel_density` /
/// `base_density` at UV/IR wavelengths where the dye/base density isn't
/// measured. Python's `density_to_light` zeros NaN values out (correctly
/// excluding those wavelengths from the einsum). WGSL on Metal compiles with
/// fast-math semantics and optimizes away `x != x` NaN checks, so we cannot
/// rely on in-shader NaN handling. We sanitize CPU-side instead:
///
/// - `channel_density` NaN → 0
/// - `base_density` NaN OR any of its row in channel_density is NaN → +1000
///   (so `pow(10, -1000) ≈ 0` zeros the whole wavelength's contribution)
///
/// This preserves Python's wavelength-skipping semantics exactly.
#[cfg(feature = "wgpu-backend")]
fn sanitize_spectral_inputs(
    channel_density: &[[f64; 3]],
    base_density: &[f64],
    n_wl: usize,
) -> (Vec<f32>, Vec<f32>) {
    let mut cd = Vec::with_capacity(n_wl * 3);
    let mut bd = Vec::with_capacity(n_wl);
    for wl in 0..n_wl {
        let r = channel_density[wl][0];
        let g = channel_density[wl][1];
        let b = channel_density[wl][2];
        let base = if wl < base_density.len() {
            base_density[wl]
        } else {
            f64::NAN
        };
        let row_has_nan = r.is_nan() || g.is_nan() || b.is_nan() || base.is_nan();
        cd.push(if r.is_nan() { 0.0 } else { r as f32 });
        cd.push(if g.is_nan() { 0.0 } else { g as f32 });
        cd.push(if b.is_nan() { 0.0 } else { b as f32 });
        bd.push(if row_has_nan { 1000.0 } else { base as f32 });
    }
    (cd, bd)
}

#[cfg(all(feature = "wgpu-backend", feature = "precision-f64"))]
#[inline]
fn f32_to_scalars(v: Vec<f32>) -> Vec<spektrafilm_math::precision::Scalar> {
    v.into_iter().map(|x| x as f64).collect()
}

#[cfg(feature = "wgpu-backend")]
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Cache compiled compute pipelines keyed by shader source pointer.
    /// `&'static str` is fine because all our shader sources come from `include_str!`.
    pipeline_cache: std::sync::Mutex<std::collections::HashMap<usize, CachedPipeline>>,
    /// Vulkan driver binaries reused between processes (Metal has its own cache).
    disk_pipeline_cache: Option<crate::pipeline_cache::DiskPipelineCache>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    capacity: crate::capacity::CapacityLimits,
    /// Upper bound for `WORKGROUP_SIZE` overrides: the adapter's limit, or
    /// lower with `SPEKTRAFILM_WGPU_WORKGROUP_SIZE` (a diagnostic to exercise
    /// the smaller sizes other GPUs get).
    max_workgroup_size: u32,
    /// The Metal device whose per-pipeline thread limits are queried: GPUs
    /// that are not Apple-designed, or any with
    /// `SPEKTRAFILM_WGPU_PROBE_WORKGROUPS` set.
    #[cfg(target_os = "macos")]
    threadgroup_probe: Option<metal::Device>,
}

#[cfg(feature = "wgpu-backend")]
struct CachedPipeline {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    /// `WORKGROUP_SIZE` the pipeline was built with; 0 for shaders with a
    /// fixed `@workgroup_size`.
    workgroup_size: u32,
}

#[cfg(feature = "wgpu-backend")]
impl WgpuBackend {
    pub fn new() -> Option<Self> {
        // Metal on macOS, Vulkan everywhere else. DX12 is left out on purpose:
        // every Windows GPU that can run this well has Vulkan (NVIDIA and AMD
        // since 2012, Intel since Skylake), and a second Windows backend would
        // need its own shader compiler and golden calibration. Without Vulkan
        // the CPU engine renders instead.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::METAL,
            ..Default::default()
        });

        // TODO(intel): on MacBook Pros with an Intel iGPU and an AMD dGPU this
        // picks the AMD GPU, which switches the discrete GPU on. Measure both
        // for speed and battery, and check large RAWs (e.g. 60 MP) fit the
        // 4 GB of VRAM on smaller Radeon Pro parts.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;

        // The default `Limits` cap storage buffer bindings at 128 MB,
        // which a 6-channel ≥ 14 MP image exceeds (image_bytes =
        // width × height × 3 × 4). Bump every relevant limit up to the
        // adapter's hardware ceiling so we can render arbitrary
        // megapixel counts (within RAM).
        let adapter_info = adapter.get_info();
        #[cfg(target_os = "linux")]
        if adapter_info.device_type == wgpu::DeviceType::Cpu {
            tracing::warn!(adapter = adapter_info.name, "software Vulkan adapter; using the CPU engine");
            return None;
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        let capacity = crate::capacity::vulkan_limits(&adapter);
        let adapter_limits = adapter.limits();
        let mut limits = wgpu::Limits::default();
        limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
        limits.max_buffer_size = adapter_limits.max_buffer_size;
        limits.max_compute_workgroups_per_dimension =
            adapter_limits.max_compute_workgroups_per_dimension;
        limits.max_bind_groups = adapter_limits.max_bind_groups.max(limits.max_bind_groups);
        // Our per-pixel shaders use `@workgroup_size(1024)`; the default
        // Limits cap workgroup invocations at 256, so lift that here too.
        // Large images wrap the dispatch grid into y (`linear_dispatch_grid`)
        // to stay under the 65535-workgroups-per-dimension limit.
        limits.max_compute_invocations_per_workgroup =
            adapter_limits.max_compute_invocations_per_workgroup;
        limits.max_compute_workgroup_size_x = adapter_limits.max_compute_workgroup_size_x;
        limits.max_compute_workgroup_size_y = adapter_limits.max_compute_workgroup_size_y;
        limits.max_compute_workgroup_size_z = adapter_limits.max_compute_workgroup_size_z;
        // MAPPABLE_PRIMARY_BUFFERS lets the input/output STORAGE buffers also
        // be MAP_WRITE / MAP_READ, so they map directly for a zero-copy
        // upload/readback. On unified-memory GPUs this skips the slow
        // Private↔Shared staging blits (~0.5 GB/s) that otherwise dominate the
        // per-frame cost. No-op (falls back to the blit path) if unsupported.
        // Diagnostic override to exercise the dedicated upload/readback path
        // on integrated GPUs that normally map the primary image buffers.
        // TODO(intel): Intel iGPUs also report IntegratedGpu, so they take the
        // mapped path. Check it renders the same as SPEKTRAFILM_WGPU_FORCE_STAGING
        // there and is actually faster.
        let force_staging = std::env::var_os("SPEKTRAFILM_WGPU_FORCE_STAGING").is_some();
        let mut opt_feats =
            if adapter_info.device_type == wgpu::DeviceType::IntegratedGpu && !force_staging {
                wgpu::Features::MAPPABLE_PRIMARY_BUFFERS & adapter.features()
            } else {
                wgpu::Features::empty()
            };
        let cache_path = crate::pipeline_cache::DiskPipelineCache::path(&adapter_info)
            .filter(|_| adapter.features().contains(wgpu::Features::PIPELINE_CACHE));
        if cache_path.is_some() {
            opt_feats |= wgpu::Features::PIPELINE_CACHE;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("spektrafilm"),
                required_features: opt_feats,
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .ok()?;

        let disk_pipeline_cache = cache_path
            .map(|path| crate::pipeline_cache::DiskPipelineCache::new(&device, path));

        let limits = device.limits();
        let mut max_workgroup_size = limits
            .max_compute_invocations_per_workgroup
            .min(limits.max_compute_workgroup_size_x);
        if let Some(cap) = std::env::var("SPEKTRAFILM_WGPU_WORKGROUP_SIZE")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
        {
            max_workgroup_size = max_workgroup_size.min(cap.max(1));
        }
        #[cfg(target_os = "macos")]
        let threadgroup_probe = metal_threadgroups::raw_device(&device).filter(|raw| {
            !metal_threadgroups::is_apple_gpu(raw)
                || std::env::var_os("SPEKTRAFILM_WGPU_PROBE_WORKGROUPS").is_some()
        });

        tracing::info!(
            adapter = adapter_info.name,
            backend = ?adapter_info.backend,
            max_workgroup_size,
            "wgpu backend initialized"
        );

        Some(Self {
            device,
            queue,
            pipeline_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            disk_pipeline_cache,
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            capacity,
            max_workgroup_size,
            #[cfg(target_os = "macos")]
            threadgroup_probe,
        })
    }

    /// `WORKGROUP_SIZE` for `shader_source`: its default, lowered to the
    /// largest power of two this GPU runs for the compiled pipeline. 0 for
    /// shaders with a fixed `@workgroup_size`, which are only checked.
    ///
    /// Apple GPUs run every default (grain.wgsl already asks for 256), so
    /// only other GPUs are queried, once per pipeline.
    // TODO(intel): on Intel (Iris/UHD) and AMD Radeon Macs, run with
    // RUST_LOG=spektrafilm_gpu=debug and record which shaders are lowered,
    // check none of the fixed-size shaders warn, and compare a render with
    // Apple silicon's.
    fn workgroup_size(&self, shader_source: &str, entries: &[wgpu::BindGroupLayoutEntry]) -> u32 {
        let floor_pow2 = |n: u32| 1u32 << (31 - n.max(1).leading_zeros());
        let declared = declared_workgroup_size(shader_source);
        let mut size = declared.map_or(0, |d| floor_pow2(d.min(self.max_workgroup_size)));
        #[cfg(target_os = "macos")]
        if let Some(device) = &self.threadgroup_probe {
            let query = |size: Option<u32>| {
                metal_threadgroups::max_threads(device, shader_source, entries, size)
                    .unwrap_or_else(|error| {
                        // Unexpected; a 64-thread group runs on every Mac GPU.
                        tracing::warn!(%error, "could not query the Metal threadgroup limit");
                        64
                    })
            };
            if declared.is_none() {
                let threads = fixed_workgroup_threads(shader_source).unwrap_or(1);
                let limit = query(None);
                tracing::debug!(threads, limit, "Metal threadgroup limit");
                if threads > limit {
                    tracing::warn!(
                        threads,
                        limit,
                        "fixed workgroup exceeds the GPU's limit; its dispatches may be dropped"
                    );
                }
                return 0;
            }
            // The limit is of the pipeline specialized to `size`, so step
            // down until the specialization that will be built fits.
            loop {
                let limit = query(Some(size));
                tracing::debug!(size, limit, "Metal threadgroup limit");
                if size <= limit || size == 1 {
                    break;
                }
                size = floor_pow2(limit.min(size / 2));
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = entries;
        size
    }

    /// Compile `shader_source`'s `main` with one bind group of `entries`,
    /// setting its `WORKGROUP_SIZE` override if it has one.
    fn build_pipeline(
        &self,
        shader_source: &'static str,
        entries: &[wgpu::BindGroupLayoutEntry],
    ) -> CachedPipeline {
        // SAFETY: `relaxed_loop_bounding` shaders' loops all have finite
        // bounds; buffer and array accesses stay bounds-checked.
        let shader = unsafe {
            self.device.create_shader_module_trusted(
                wgpu::ShaderModuleDescriptor {
                    label: Some("compute_shader"),
                    source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source)),
                },
                wgpu::ShaderRuntimeChecks {
                    bounds_checks: true,
                    force_loop_bounding: !relaxed_loop_bounding(shader_source),
                },
            )
        };
        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("compute_layout"),
                    entries,
                });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compute_pipeline_layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let workgroup_size = self.workgroup_size(shader_source, entries);
        let constants: std::collections::HashMap<String, f64> = if workgroup_size > 0 {
            [("WORKGROUP_SIZE".to_string(), workgroup_size as f64)].into()
        } else {
            Default::default()
        };
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("compute_pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    // naga zeroes workgroup memory from one invocation per
                    // workgroup, which made the tiled blurs several times
                    // slower. Every shader writes what it reads there first.
                    zero_initialize_workgroup_memory: false,
                },
                cache: self.disk_pipeline_cache.as_ref().map(|c| &c.cache),
            });
        if let Some(cache) = &self.disk_pipeline_cache {
            cache.mark_dirty();
        }
        CachedPipeline {
            bind_group_layout,
            pipeline,
            workgroup_size,
        }
    }

    /// Get or compile + cache a pipeline keyed by shader source pointer.
    /// All shader sources come from `include_str!` so the pointer is stable.
    fn get_or_compile_pipeline<F>(
        &self,
        shader_source: &'static str,
        layout_entries_fn: F,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<usize, CachedPipeline>>
    where
        F: FnOnce() -> Vec<wgpu::BindGroupLayoutEntry>,
    {
        let key = shader_source.as_ptr() as usize;
        let mut cache = self.pipeline_cache.lock().unwrap();
        if !cache.contains_key(&key) {
            let pipeline = self.build_pipeline(shader_source, &layout_entries_fn());
            cache.insert(key, pipeline);
        }
        cache
    }

    /// Generic GPU compute dispatch helper.
    ///
    /// `shader_source` must be a `'static str` (typically from `include_str!`) so
    /// the cache can key by pointer identity.
    fn dispatch_compute(
        &self,
        shader_source: &'static str,
        bindings: &[GpuBuffer],
        n_pixels: u32,
        output_idx: usize,
    ) -> Vec<f32> {
        let t_start = std::time::Instant::now();
        let layout_entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupLayoutEntry {
                binding: i as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: b.binding_type,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let entries_for_compile = layout_entries.clone();
        let cache = self.get_or_compile_pipeline(shader_source, || entries_for_compile);
        let key = shader_source.as_ptr() as usize;
        let cached = cache.get(&key).expect("pipeline just inserted");
        let pipeline = &cached.pipeline;
        let bind_group_layout = &cached.bind_group_layout;
        let t_compile = t_start.elapsed();

        // Create GPU buffers
        let gpu_buffers: Vec<wgpu::Buffer> = bindings
            .iter()
            .map(|b| {
                use wgpu::util::DeviceExt;
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("buffer"),
                        contents: &b.data,
                        usage: b.usage,
                    })
            })
            .collect();

        let bind_entries: Vec<wgpu::BindGroupEntry> = gpu_buffers
            .iter()
            .enumerate()
            .map(|(i, buf)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: buf.as_entire_binding(),
            })
            .collect();

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compute_bind_group"),
            layout: &bind_group_layout,
            entries: &bind_entries,
        });

        // Readback buffer
        let output_size = bindings[output_idx].data.len() as u64;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: output_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Dispatch

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("compute_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            dispatch_linear(&mut pass, n_pixels, cached.workgroup_size);
        }
        encoder.copy_buffer_to_buffer(&gpu_buffers[output_idx], 0, &readback, 0, output_size);
        self.queue.submit(Some(encoder.finish()));

        // Read back
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).unwrap();
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();

        let data = slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        readback.unmap();
        let _t_total = t_start.elapsed();
        let _ = t_compile;
        result
    }

    /// Blur `img` with every sigma in `sigmas` on the GPU, like the CPU's
    /// `gaussian_blur` (`gaussian_blur_channel` per channel: FIR with
    /// `radius = int(3σ + 0.5)` below σ = 3, the recursive filter above).
    /// One upload, one command buffer, one readback per sigma.
    pub fn gaussian_blur_multi_gpu(&self, img: &ImageBuf, sigmas: &[f32]) -> Vec<ImageBuf> {
        assert!(!sigmas.is_empty(), "gaussian_blur_multi_gpu: empty sigmas");
        let (w, h) = (img.width, img.height);
        let img_bytes = (w as u64) * (h as u64) * 3 * 4;
        let make_buf = |label: &str| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: img_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let buf_in = make_buf("blur_multi_in");
        let buf_mid = make_buf("blur_multi_mid");
        let buf_out = make_buf("blur_multi_out");
        self.queue.write_buffer(&buf_in, 0, bytemuck::cast_slice(&scalars_to_f32(&img.data)));
        // One readback per sigma: a single combined buffer could exceed the
        // per-buffer limit at large sizes.
        let readbacks: Vec<_> = (0..sigmas.len())
            .map(|i| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("blur_multi_readback_{i}")),
                    size: img_bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut blurs = Vec::with_capacity(sigmas.len());
        for (i, &sigma) in sigmas.iter().enumerate() {
            if sigma > 0.0 {
                let blur = build_ref_blur(&self.device, self, w, h, &buf_in, &buf_mid, &buf_out, [sigma; 3], [1.0; 3], false, "blur_multi");
                blur.encode(&mut encoder);
                blurs.push(blur);
                encoder.copy_buffer_to_buffer(&buf_out, 0, &readbacks[i], 0, img_bytes);
            } else {
                encoder.copy_buffer_to_buffer(&buf_in, 0, &readbacks[i], 0, img_bytes);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        for rb in &readbacks {
            rb.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        }
        self.device.poll(wgpu::Maintain::Wait);
        readbacks
            .iter()
            .map(|rb| {
                let out: Vec<f32> = bytemuck::cast_slice(&rb.slice(..).get_mapped_range()).to_vec();
                rb.unmap();
                ImageBuf::from_data(w, h, f32_to_scalars(out))
            })
            .collect()
    }

    /// The grain passes alone on a density image (the film chain's step 3c),
    /// for tests that check the GPU grain's statistics against the CPU's.
    pub fn run_grain(&self, density: &ImageBuf, gp: &crate::GrainGpuParams) -> ImageBuf {
        use wgpu::util::DeviceExt;
        let (w, h) = (density.width, density.height);
        let bytes = density.pixel_count() as u64 * 12;
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let den = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grain_density"),
            contents: bytemuck::cast_slice(&scalars_to_f32(&density.data)),
            usage,
        });
        let scratch: Vec<_> = (0..3)
            .map(|_| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("grain_scratch"),
                    size: bytes,
                    usage,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grain_readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let state = build_grain_state(&self.device, gp, w, h, &den, &scratch[0], &scratch[1], &scratch[2], self);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        state.encode_passes(&mut encoder);
        encoder.copy_buffer_to_buffer(&den, 0, &readback, 0, bytes);
        self.queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        self.device.poll(wgpu::Maintain::Wait);
        let out: Vec<f32> = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();
        ImageBuf::from_data(w, h, f32_to_scalars(out))
    }

    /// GPU-resident pipeline: runs the front pass (tc LUT lookup),
    /// highlight boost, camera diffusion/lens blur,
    /// halation, density curves, DIR, grain, print spectral, enlarger
    /// diffusion, scan spectral, glare, gamut compression, scanner lens blur,
    /// and unsharp as a single command buffer with ping-pong image storage.
    /// Only one upload at the start and one readback at the end.
    pub fn run_film_chain(&self, p: &crate::FilmChainParams<'_>) -> ImageBuf {
        use wgpu::util::DeviceExt;
        let t_start = std::time::Instant::now();
        // Pull all references into locals so the existing body below
        // doesn't need a rewrite — only the param sources change.
        let image = p.image;
        let film_log_exposure = p.film_log_exposure;
        let film_density_curves_normalized = p.film_density_curves_normalized;
        let film_channel_density = p.film_channel_density;
        let film_base_density = p.film_base_density;
        let print_illuminant = p.print_illuminant;
        let print_sensitivity = p.print_sensitivity;
        let print_normalization_factor = p.print_normalization_factor;
        let print_log_exposure = p.print_log_exposure;
        let print_density_curves = p.print_density_curves;
        let preflash = p.preflash;
        let print_channel_density = p.print_channel_density;
        let print_base_density = p.print_base_density;
        let viewing_illuminant = p.viewing_illuminant;
        let scan_normalization = p.scan_normalization;
        let scan_xyz_to_rgb = p.scan_xyz_to_rgb;

        let n_pixels = image.pixel_count() as u32;
        let img_bytes = n_pixels as usize * 3 * 4;

        // Ping-pong image buffers (each holds H*W*3 f32 values).
        let make_img_buf = |label: &str| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: img_bytes as u64,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let mappable = self
            .device
            .features()
            .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);

        // buf_a is the input/ping-pong buffer. The dominant per-frame cost was
        // the input upload: `queue.write_buffer` stages CPU→GPU through a slow
        // (~0.5 GB/s) blit on this unified-memory GPU. With
        // MAPPABLE_PRIMARY_BUFFERS we create buf_a already mapped (MAP_WRITE)
        // and memcpy the input straight into its (shared) memory — no staging.
        let input_f32 = scalars_to_f32(&image.data);
        let buf_a = if mappable {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("img_a"),
                size: img_bytes as u64,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::MAP_WRITE,
                mapped_at_creation: true,
            });
            buf.slice(..)
                .get_mapped_range_mut()
                .copy_from_slice(bytemuck::cast_slice(&input_f32));
            buf.unmap();
            buf
        } else {
            let buf = make_img_buf("img_a");
            self.queue
                .write_buffer(&buf, 0, bytemuck::cast_slice(&input_f32));
            buf
        };

        // buf_b holds the final RGB. With MAPPABLE_PRIMARY_BUFFERS we add
        // MAP_READ so it can be mapped directly for a zero-copy readback,
        // skipping the slow Private→Shared blit on unified memory.
        let buf_b = {
            let mut usage = wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC;
            if mappable {
                usage |= wgpu::BufferUsages::MAP_READ;
            }
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("img_b"),
                size: img_bytes as u64,
                usage,
                mapped_at_creation: false,
            })
        };

        // Static (LUT) buffers — uploaded once.
        let mk_storage = |label: &str, bytes: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let mk_uniform = |label: &str, bytes: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        };

        // ── Pre-compute every static GPU-side buffer ─────────────────────
        // Filming density curves — already normalized by caller.
        let film_log_exp_f32: Vec<f32> = film_log_exposure.iter().map(|&v| v as f32).collect();
        let film_curves_f32: Vec<f32> = film_density_curves_normalized
            .iter()
            .flat_map(|r| r.iter().map(|&v| if v.is_nan() { 0.0 } else { v as f32 }))
            .collect();
        let film_log_exp_buf = mk_storage("film_log_exp", bytemuck::cast_slice(&film_log_exp_f32));
        let film_curves_buf = mk_storage("film_curves", bytemuck::cast_slice(&film_curves_f32));

        // Film spectral data (for printing pass).
        let (film_cd_f32, mut film_bd_f32) = sanitize_spectral_inputs(
            film_channel_density,
            film_base_density,
            film_channel_density.len(),
        );
        film_bd_f32.resize(film_channel_density.len(), 0.0);
        let film_cd_buf = mk_storage("film_cd", bytemuck::cast_slice(&film_cd_f32));
        let film_bd_buf = mk_storage("film_bd", bytemuck::cast_slice(&film_bd_f32));

        let print_illu_f32: Vec<f32> = print_illuminant.iter().map(|&v| v as f32).collect();
        let print_sens_f32: Vec<f32> = print_sensitivity
            .iter()
            .flat_map(|r| r.iter().map(|&v| if v.is_nan() { 0.0 } else { v as f32 }))
            .collect();
        let print_illu_buf = mk_storage("print_illu", bytemuck::cast_slice(&print_illu_f32));
        let print_sens_buf = mk_storage("print_sens", bytemuck::cast_slice(&print_sens_f32));

        // Print density curves (RAW, no normalization for print path).
        let print_log_exp_f32: Vec<f32> = print_log_exposure.iter().map(|&v| v as f32).collect();
        let print_curves_f32: Vec<f32> = print_density_curves
            .iter()
            .flat_map(|r| r.iter().map(|&v| if v.is_nan() { 0.0 } else { v as f32 }))
            .collect();
        let print_log_exp_buf =
            mk_storage("print_log_exp", bytemuck::cast_slice(&print_log_exp_f32));
        let print_curves_buf = mk_storage("print_curves", bytemuck::cast_slice(&print_curves_f32));

        // Spectral dye-density data for the scanning pass. For scan_film we
        // scan the developed film directly, so feed the film's spectral
        // densities; otherwise the print's. (print_spectral uses
        // sensitivity, not these — this buffer feeds only scan_spectral.)
        let (scan_cd_src, scan_bd_src): (&[[f64; 3]], &[f64]) = if p.scan_film {
            (film_channel_density, film_base_density)
        } else {
            (print_channel_density, print_base_density)
        };
        let (scan_cd_f32, mut scan_bd_f32) =
            sanitize_spectral_inputs(scan_cd_src, scan_bd_src, scan_cd_src.len());
        scan_bd_f32.resize(scan_cd_src.len(), 0.0);
        let scan_cd_buf = mk_storage("scan_cd", bytemuck::cast_slice(&scan_cd_f32));
        let scan_bd_buf = mk_storage("scan_bd", bytemuck::cast_slice(&scan_bd_f32));

        let view_illu_f32: Vec<f32> = viewing_illuminant.iter().map(|&v| v as f32).collect();
        let view_illu_buf = mk_storage("view_illu", bytemuck::cast_slice(&view_illu_f32));
        let cmf_x_buf = mk_storage(
            "cmf_x",
            bytemuck::cast_slice(&spektrafilm_math::spectral::CMF_X_F32),
        );
        let cmf_y_buf = mk_storage(
            "cmf_y",
            bytemuck::cast_slice(&spektrafilm_math::spectral::CMF_Y_F32),
        );
        let cmf_z_buf = mk_storage(
            "cmf_z",
            bytemuck::cast_slice(&spektrafilm_math::spectral::CMF_Z_F32),
        );

        // ── Param structs ────────────────────────────────────────────────
        // Front pass (tc LUT lookup).
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct FrontParams {
            width: u32,
            height: u32,
            lut_size: u32,
            _pad: u32,
            col0: [f32; 4],
            col1: [f32; 4],
            col2: [f32; 4],
        }
        let (m, front_lut_size, guard) = match &p.front {
            crate::FrontPass::Hanatos2025 {
                tc_lut,
                rgb_to_adapted_xyz,
                guard,
            } => (rgb_to_adapted_xyz, tc_lut.size as u32, *guard),
        };
        let front_params = FrontParams {
            width: image.width,
            height: image.height,
            lut_size: front_lut_size,
            _pad: 0,
            col0: [m[0][0] as f32, m[1][0] as f32, m[2][0] as f32, 0.0],
            col1: [m[0][1] as f32, m[1][1] as f32, m[2][1] as f32, 0.0],
            col2: [m[0][2] as f32, m[1][2] as f32, m[2][2] as f32, 0.0],
        };
        let front_params_buf = mk_uniform("front_params", bytemuck::bytes_of(&front_params));
        // The spectral boundary guard rewrites non-physical input in place on
        // buf_a before the front pass, which stays upstream's arithmetic.
        let guard_params_buf = guard.then(|| {
            mk_uniform("guard_params", bytemuck::bytes_of(&SpectralGuardParams::new(n_pixels, m)))
        });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct DensityParams {
            width: u32,
            height: u32,
            k: u32,
            uniform_grid: u32,
            gamma_inv: [f32; 3],
            _pad: f32,
        }
        let film_density_params = DensityParams {
            width: image.width,
            height: image.height,
            k: film_log_exposure.len() as u32,
            uniform_grid: if is_uniform(film_log_exposure) { 1 } else { 0 },
            // Unit gamma: Film Curves is baked into the curves.
            gamma_inv: [1.0; 3],
            _pad: 0.0,
        };
        let film_density_params_buf =
            mk_uniform("film_dens_params", bytemuck::bytes_of(&film_density_params));

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct PrintParams {
            width: u32,
            height: u32,
            n_wavelengths: u32,
            normalization_factor: f32,
            preflash: [f32; 3],
            _pad: f32,
        }
        let has_enlarger_diffusion = p.enlarger_diffusion.is_some();
        let print_exposure_scale = p.print_exposure_scale as f32;
        let print_norm_for_shader = if has_enlarger_diffusion && print_exposure_scale != 0.0 {
            (print_normalization_factor / p.print_exposure_scale) as f32
        } else {
            print_normalization_factor as f32
        };
        let print_params = PrintParams {
            width: image.width,
            height: image.height,
            n_wavelengths: film_channel_density.len() as u32,
            normalization_factor: print_norm_for_shader,
            preflash: [preflash[0] as f32, preflash[1] as f32, preflash[2] as f32],
            _pad: 0.0,
        };
        let print_params_buf = mk_uniform("print_params", bytemuck::bytes_of(&print_params));

        let print_density_params = DensityParams {
            width: image.width,
            height: image.height,
            k: print_log_exposure.len() as u32,
            uniform_grid: if is_uniform(print_log_exposure) { 1 } else { 0 },
            gamma_inv: [1.0; 3],
            _pad: 0.0,
        };
        let print_density_params_buf = mk_uniform(
            "print_dens_params",
            bytemuck::bytes_of(&print_density_params),
        );

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct ScanParams {
            width: u32,
            height: u32,
            n_wavelengths: u32,
            normalization: f32,
            col0: [f32; 4],
            col1: [f32; 4],
            col2: [f32; 4],
            bw: [f32; 4],
        }
        let s = scan_xyz_to_rgb;
        // B&W/slide luminance remap (m, q); z=1 enables it in the shader.
        // w=1 skips the shader's [0,1] clamp: glare, gamut compression and
        // the scanner unsharp mask see the unclamped scan RGB, as on the CPU
        // and upstream, and the output stays unclamped like theirs.
        let bw = match p.bw_xyz_remap {
            Some((m, q)) => [m as f32, q as f32, 1.0, 1.0],
            None => [1.0, 0.0, 0.0, 1.0],
        };
        let scan_params = ScanParams {
            width: image.width,
            height: image.height,
            n_wavelengths: scan_cd_src.len() as u32,
            normalization: scan_normalization as f32,
            col0: [s[0][0] as f32, s[1][0] as f32, s[2][0] as f32, 0.0],
            col1: [s[0][1] as f32, s[1][1] as f32, s[2][1] as f32, 0.0],
            col2: [s[0][2] as f32, s[1][2] as f32, s[2][2] as f32, 0.0],
            bw,
        };
        let scan_params_buf = mk_uniform("scan_params", bytemuck::bytes_of(&scan_params));

        // ── Pre-compile pipelines (cached after first call) ──────────────
        // Each shader's bindings layout is fixed and known here.
        let density_pipe = self.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/density_curve_interp.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
            ],
        );
        let print_pipe = self.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/print_spectral.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
            ],
        );
        let scan_pipe = self.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/scan_spectral.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
            ],
        );

        // ── Build bind groups (per dispatch, but no buffer creation) ─────

        // Front pass: params + rgb_in + tc_lut + raw_out. The tc_lut buffer
        // rides along in the tuple to outlive the encoder.
        let (front_pipe, bg_front, _front_tc_lut) = match &p.front {
            crate::FrontPass::Hanatos2025 { tc_lut, .. } => {
                let tc_lut_f32: Vec<f32> = tc_lut.data.iter().map(|&v| v as f32).collect();
                let tc_lut_buf = mk_storage("tc_lut", bytemuck::cast_slice(&tc_lut_f32));
                let pipe = self.cached_pipeline(
                    include_str!("../../spektrafilm-shaders/wgsl/hanatos2025_rgb_to_raw.wgsl"),
                    &[
                        wgpu::BufferBindingType::Uniform,
                        wgpu::BufferBindingType::Storage { read_only: true },
                        wgpu::BufferBindingType::Storage { read_only: true },
                        wgpu::BufferBindingType::Storage { read_only: false },
                    ],
                );
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bg_hanatos"),
                    layout: &pipe.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: front_params_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: buf_a.as_entire_binding(),
                        }, // rgb_in
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: tc_lut_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: buf_b.as_entire_binding(),
                        }, // raw_out
                    ],
                });
                (pipe, bg, tc_lut_buf)
            }
        };
        let guard_pass = guard_params_buf.as_ref().map(|params| {
            let pipe = self.cached_pipeline(
                SPECTRAL_GUARD_WGSL,
                &[wgpu::BufferBindingType::Uniform, wgpu::BufferBindingType::Storage { read_only: false }],
            );
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bg_spectral_guard"),
                layout: &pipe.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: buf_a.as_entire_binding() },
                ],
            });
            (pipe, bg)
        });
        // After the front pass: log10 + density curve interp into normalized
        // film curves. The front pass outputs raw (not log_raw), so a small
        // log10 shader transforms buf_b in-place, then density_curve_interp
        // reads buf_b → buf_a.

        let bg_log10 = {
            let pipe = self.cached_pipeline(
                include_str!("../../spektrafilm-shaders/wgsl/log10_inplace.wgsl"),
                &[
                    wgpu::BufferBindingType::Uniform,
                    wgpu::BufferBindingType::Storage { read_only: false },
                ],
            );
            #[repr(C)]
            #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
            struct Log10Params {
                // WGSL struct: `n: u32 + _pad: vec3<u32>`. vec3 has 16-byte alignment,
                // so the struct is 32 bytes total. We pad on the Rust side accordingly.
                n_pixels: u32,
                _pad: [u32; 7],
            }
            let log10_params_buf = mk_uniform(
                "log10_params",
                bytemuck::bytes_of(&Log10Params {
                    n_pixels,
                    _pad: [0; 7],
                }),
            );
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bg_log10"),
                layout: &pipe.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: log10_params_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buf_b.as_entire_binding(),
                    },
                ],
            });
            // Return both the buffer (to keep it alive) and the bind group.
            (pipe, bg, log10_params_buf)
        };

        let bg_density_film = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_density_film"),
            layout: &density_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: film_density_params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buf_b.as_entire_binding(),
                }, // log_raw
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: film_log_exp_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: film_curves_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: buf_a.as_entire_binding(),
                }, // density_cmy
            ],
        });
        let bg_print = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_print"),
            layout: &print_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: print_params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buf_a.as_entire_binding(),
                }, // density_cmy
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: film_cd_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: film_bd_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: print_illu_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: print_sens_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: buf_b.as_entire_binding(),
                }, // log_raw_print
            ],
        });
        let bg_density_print = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_density_print"),
            layout: &density_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: print_density_params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buf_b.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: print_log_exp_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: print_curves_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: buf_a.as_entire_binding(),
                },
            ],
        });
        let bg_scan = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_scan"),
            layout: &scan_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scan_params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buf_a.as_entire_binding(),
                }, // density_print
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: scan_cd_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: scan_bd_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: view_illu_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: cmf_x_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: cmf_y_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: cmf_z_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: buf_b.as_entire_binding(),
                }, // final rgb
            ],
        });

        // Readback buffer (for the final image only) — only needed when we
        // can't map buf_b directly. Skipping it on the mappable path also
        // avoids allocating a second full-image buffer per frame.
        let readback = (!mappable).then(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: img_bytes as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        // Effects execute sequentially in separate compute passes. Share their
        // full-image storage, but keep every input/output of a dispatch distinct.
        // See docs/memory-scratch-reuse.md for the pass/lifetime table.
        // This pool is per-render: no full images enter the backend cache.
        let scratch_count = if p.dir_couplers.is_some()
            || p.halation.is_some()
            || p.unsharp.is_some()
            || p.grain.is_some()
        {
            2
        } else if p.glare.is_some()
            || p.diffusion.is_some()
            || (!p.scan_film && p.enlarger_diffusion.is_some())
        {
            1
        } else {
            0
        };
        let scratch: Vec<_> = (0..scratch_count)
            .map(|i| make_img_buf(&format!("effect_scratch_{i}")))
            .collect();

        // ── Halation auxiliary buffers + bind groups (only if active) ────
        // The two ping-pong buffers are reused as blur input/mid; shared
        // `buf_c` and `buf_d` hold the scatter outputs and the halation
        // accumulator. None of this is touched when `p.halation` is `None`.
        let halation_state = p.halation.as_ref().map(|hp| {
            build_halation_state(
                &self.device,
                hp,
                image.width,
                image.height,
                &buf_a,
                &buf_b,
                &scratch,
                self,
            )
        });

        // ── Highlight boost state ────────────────────────────────────────
        // Runs on raw film exposure immediately after the front pass.
        let highlight_state = p
            .highlight_boost
            .as_ref()
            .map(|hp| build_highlight_boost_state(&self.device, hp, n_pixels, &buf_b, self));

        // ── Camera diffusion filter state ────────────────────────────────
        // Applied on the raw film exposure (buf_b) right after hanatos and
        // before halation, matching the CPU filming order.
        let diffusion_state = p.diffusion.as_ref().map(|plan| {
            build_diffusion_state(
                &self.device,
                plan,
                image.width,
                image.height,
                &buf_b,
                &scratch,
                self,
            )
        });

        let camera_lens_blur_state = p.camera_lens_blur_px.and_then(|sigma| {
            (sigma > 0.0).then(|| {
                build_simple_blur_state(
                    &self.device,
                    sigma,
                    image.width,
                    image.height,
                    &buf_b,
                    &buf_a,
                    "camera_lens",
                    self,
                )
            })
        });

        // ── Unsharp mask state ───────────────────────────────────────────
        // Last pass before readback. Blurs buf_b → buf_c (via buf_a mid),
        // then combines: buf_b_out = (1+amount)*buf_b - amount*buf_c.
        // Since the combine writes back to buf_b in-place we need to
        // route via a temporary buffer to satisfy wgpu aliasing rules.
        let unsharp_state = p.unsharp.as_ref().map(|up| {
            build_unsharp_state(
                &self.device,
                up,
                image.width,
                image.height,
                &buf_a,
                &buf_b,
                &scratch,
                self,
            )
        });

        // ── Output gamut compression state ───────────────────────────────
        // Single per-pixel dispatch in place on buf_b, after glare and
        // before unsharp (the CPU scanning order). The C_max table is
        // baked once at pipeline construction; only the upload happens here.
        let gamut_state = p
            .gamut
            .as_ref()
            .map(|gp| build_gamut_state(&self.device, gp, &buf_b, n_pixels, self));

        // ── Glare state ──────────────────────────────────────────────────
        // Applied in place on buf_b (the scan_spectral output) just before
        // readback. Generates per-pixel lognormal noise into a scratch
        // buffer, optionally blurs it, then adds `g * rgb_offset[c]` to
        // the image. Uses buf_a (free after scan_spectral consumed
        // density_cmy) and one shared scratch buffer.
        let glare_state = p.glare.as_ref().map(|gp| {
            build_glare_state(
                &self.device,
                gp,
                image.width,
                image.height,
                &buf_a,
                &buf_b,
                &scratch,
                self,
            )
        });

        let scanner_lens_blur_state = p.scanner_lens_blur_px.and_then(|sigma| {
            (sigma > 0.0).then(|| {
                build_simple_blur_state(
                    &self.device,
                    sigma,
                    image.width,
                    image.height,
                    &buf_b,
                    &buf_a,
                    "scanner_lens",
                    self,
                )
            })
        });

        // ── Grain state ──────────────────────────────────────────────────
        // Applied to buf_a (density_cmy) after DIR couplers, before
        // print_spectral. Uses buf_b (log_raw_corrected is no longer needed
        // after DIR's final density_curve_0) and both scratch buffers.
        let grain_state = p.grain.as_ref().map(|gp| {
            build_grain_state(
                &self.device,
                gp,
                image.width,
                image.height,
                &buf_a,
                &buf_b,
                &scratch[0],
                &scratch[1],
                self,
            )
        });

        // ── DIR couplers state ────────────────────────────────────────────
        // Allocated lazily when the DIR stage is active. Reads buf_a
        // (density_cmy from film density curve) and buf_b (log_raw),
        // produces a corrected buf_a via re-interpolation of
        // `density_curves_0` against `log_raw - correction`. Uses two
        // shared scratch buffers (correction, gaussian). After matmul
        // consumes density_cmy, buf_a serves as blur mid and then as the
        // weighted mix destination. The final density curve overwrites it.
        let dir_state = p.dir_couplers.as_ref().map(|dp| {
            build_dir_state(
                &self.device,
                dp,
                image.width,
                image.height,
                &buf_a,
                &buf_b,
                &scratch,
                self,
            )
        });

        let enlarger_log_to_linear_state = p.enlarger_diffusion.as_ref().map(|_| {
            build_log_to_linear_state(&self.device, n_pixels, print_exposure_scale, &buf_b, self)
        });
        let enlarger_diffusion_state =
            p.enlarger_diffusion
                .as_ref()
                .filter(|_| !p.scan_film)
                .map(|plan| {
                    build_diffusion_state(
                        &self.device,
                        plan,
                        image.width,
                        image.height,
                        &buf_b,
                        &scratch,
                        self,
                    )
                });
        let enlarger_linear_to_log_state = p
            .enlarger_diffusion
            .as_ref()
            .map(|_| build_linear_to_log_state(&self.device, n_pixels, &buf_b, self));
        let post_scan_state = p.output_cctf_encoding.then(|| {
            build_post_scan_state(
                &self.device,
                n_pixels,
                p.output_cctf_encoding,
                p.output_encode_delta(),
                &buf_b,
                self,
            )
        });

        // ── Single command buffer chaining everything ────────────────────
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let dispatch = |encoder: &mut wgpu::CommandEncoder,
                        pipe: &CachedPipelineRef,
                        bg: &wgpu::BindGroup,
                        n: u32| {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipe.pipeline);
            pass.set_bind_group(0, bg, &[]);
            dispatch_linear(&mut pass, n, pipe.workgroup_size);
        };

        let mut profiler = GpuStageProfiler::new(&self.device, &self.queue);
        profiler.mark("upload", &mut encoder);
        // 1. Front pass (tc LUT lookup): buf_a (rgb in) → buf_b (raw), after
        // the spectral boundary guard in place on buf_a.
        if let Some((pipe, bg)) = guard_pass.as_ref() {
            dispatch(&mut encoder, pipe, bg, n_pixels);
        }
        dispatch(&mut encoder, &front_pipe, &bg_front, n_pixels);
        profiler.mark("front", &mut encoder);
        // 1a. Highlight boost on raw, before optical scatter.
        if let Some(hs) = highlight_state.as_ref() {
            hs.encode_passes(&mut encoder);
            profiler.mark("highlight_boost", &mut encoder);
        }
        // 1a. Camera diffusion filter in-place on buf_b (raw), before
        //     halation — mirrors the CPU filming order.
        if let Some(ds) = diffusion_state.as_ref() {
            ds.encode_passes(&mut encoder, &buf_b);
            profiler.mark("camera_diffusion", &mut encoder);
        }
        // 1b. Camera lens blur on raw, after camera diffusion and before halation.
        if let Some(bs) = camera_lens_blur_state.as_ref() {
            bs.encode_passes(&mut encoder);
            profiler.mark("camera_lens_blur", &mut encoder);
        }
        // 1b. Halation in-place on buf_b. Uses buf_a as blur scratch (the
        //     input RGB image is no longer needed), buf_c / buf_d for the
        //     scatter outputs and halation accumulator.
        if let Some(hs) = halation_state.as_ref() {
            hs.encode_passes(&mut encoder, n_pixels);
            profiler.mark("halation", &mut encoder);
        }
        // 2. log10 in-place on buf_b: raw → log_raw (3 channels per thread).
        let (log10_pipe, log10_bg, _keepalive) = &bg_log10;
        dispatch(&mut encoder, &log10_pipe, log10_bg, n_pixels);
        // 3. Density curve (film, normalized): buf_b (log_raw) → buf_a (density_cmy)
        dispatch(
            &mut encoder,
            &density_pipe,
            &bg_density_film,
            n_pixels,
        );
        profiler.mark("log10_density", &mut encoder);
        // 3b. DIR couplers (operates on buf_a, mutates buf_b → log_raw_corrected,
        //     re-interps density curve back into buf_a).
        if let Some(ds) = dir_state.as_ref() {
            ds.encode_passes(&mut encoder, n_pixels);
            profiler.mark("dir_couplers", &mut encoder);
        }
        // 3c. Grain (buf_a in and out, via buf_b and both scratch buffers).
        if let Some(gs) = grain_state.as_ref() {
            gs.encode_passes(&mut encoder);
            profiler.mark("grain", &mut encoder);
        }
        // 4 + 5. Printing: print_spectral (buf_a → buf_b) then the print
        //     density curve (buf_b → buf_a). Skipped for scan_film — the
        //     scan pass consumes the film density already in buf_a.
        if !p.scan_film {
            // 4. Print spectral: buf_a → buf_b (log_raw_print)
            dispatch(&mut encoder, &print_pipe, &bg_print, n_pixels);
            profiler.mark("print_spectral", &mut encoder);
            if let (Some(l2l), Some(ds), Some(l2g)) = (
                enlarger_log_to_linear_state.as_ref(),
                enlarger_diffusion_state.as_ref(),
                enlarger_linear_to_log_state.as_ref(),
            ) {
                l2l.encode_passes(&mut encoder, n_pixels);
                ds.encode_passes(&mut encoder, &buf_b);
                l2g.encode_passes(&mut encoder, n_pixels);
                profiler.mark("enlarger_diffusion", &mut encoder);
            }
            // 5. Density curve (print, raw curves): buf_b → buf_a (density_print)
            dispatch(
                &mut encoder,
                &density_pipe,
                &bg_density_print,
                n_pixels,
            );
            profiler.mark("print_density", &mut encoder);
        }
        // 6. Scan spectral: buf_a → buf_b (linear rgb, unclamped)
        dispatch(&mut encoder, &scan_pipe, &bg_scan, n_pixels);
        profiler.mark("scan_spectral", &mut encoder);
        // 6b. Glare (in place on buf_b).
        if let Some(gs) = glare_state.as_ref() {
            gs.encode_passes(&mut encoder, n_pixels);
            profiler.mark("glare", &mut encoder);
        }
        // 6c. Output gamut compression (in place on buf_b) — after glare,
        //     before unsharp, mirroring the CPU scanning order.
        if let Some(gs) = gamut_state.as_ref() {
            gs.encode_passes(&mut encoder, n_pixels);
            profiler.mark("gamut", &mut encoder);
        }
        // 6d. Scanner lens blur — after glare/gamut, before unsharp.
        if let Some(bs) = scanner_lens_blur_state.as_ref() {
            bs.encode_passes(&mut encoder);
            profiler.mark("scanner_lens_blur", &mut encoder);
        }
        // 6d. Unsharp mask — last in-flight pass. Writes the final image
        //     back to buf_b so the readback path below is unchanged.
        if let Some(us) = unsharp_state.as_ref() {
            us.encode_passes(&mut encoder, n_pixels, &buf_b, img_bytes as u64);
            profiler.mark("unsharp", &mut encoder);
        }
        // 6e. The sRGB encoding, unclipped, when the output is encoded. This
        // mirrors Pipeline::apply_post_scan so the resident WGSL path needs no
        // CPU full-frame post pass.
        if let Some(ps) = post_scan_state.as_ref() {
            ps.encode_passes(&mut encoder, n_pixels);
            profiler.mark("post_scan", &mut encoder);
        }

        // Zero-copy path: when buf_b is mappable, skip the blit and map it
        // directly below. Otherwise stage it into the MAP_READ readback buffer.
        if let Some(rb) = readback.as_ref() {
            encoder.copy_buffer_to_buffer(&buf_b, 0, rb, 0, img_bytes as u64);
        }
        // Everything since function entry: CPU-side param prep, buffer
        // creation/uploads, and command encoding.
        let cpu_setup_ms = t_start.elapsed().as_secs_f32() * 1000.0;
        self.queue.submit(Some(encoder.finish()));

        // Single sync point at the end. Map buf_b directly on the zero-copy
        // path, or the staging buffer otherwise.
        let map_target = readback.as_ref().unwrap_or(&buf_b);
        let slice = map_target.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).unwrap();
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let gpu_wait_ms = t_start.elapsed().as_secs_f32() * 1000.0 - cpu_setup_ms;
        let data = slice.get_mapped_range();
        let out_f32: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        map_target.unmap();

        profiler.report();
        // Persist only when new pipelines were compiled. The helper can be
        // terminated by its UI, so also save after a successful resident render
        // rather than relying solely on Drop at a graceful process exit.
        if let Some(cache) = &self.disk_pipeline_cache {
            cache.flush();
        }
        let out = ImageBuf::from_data(image.width, image.height, f32_to_scalars(out_f32));
        tracing::debug!(
            cpu_setup_ms = format!("{cpu_setup_ms:.1}"),
            gpu_wait_ms = format!("{gpu_wait_ms:.1}"),
            readback_ms = format!(
                "{:.1}",
                t_start.elapsed().as_secs_f32() * 1000.0 - cpu_setup_ms - gpu_wait_ms
            ),
            "film chain timings"
        );
        out
    }

    /// Get-or-compile a pipeline by shader source + binding layout. Cached by
    /// shader source pointer.
    fn cached_pipeline(
        &self,
        shader_source: &'static str,
        binding_types: &[wgpu::BufferBindingType],
    ) -> CachedPipelineRef {
        let entries: Vec<wgpu::BindGroupLayoutEntry> = binding_types
            .iter()
            .enumerate()
            .map(|(i, &ty)| wgpu::BindGroupLayoutEntry {
                binding: i as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let entries_for_compile = entries;
        let mut cache = self.pipeline_cache.lock().unwrap();
        let key = shader_source.as_ptr() as usize;
        if !cache.contains_key(&key) {
            let pipeline = self.build_pipeline(shader_source, &entries_for_compile);
            cache.insert(key, pipeline);
        }
        // Drop the guard but the underlying Arc keeps the entries alive.
        // Return cloned handles.
        let cached = cache.get(&key).unwrap();
        CachedPipelineRef {
            pipeline: cached.pipeline.clone(),
            layout: cached.bind_group_layout.clone(),
            workgroup_size: cached.workgroup_size,
        }
    }
}

/// Per-stage GPU timing for the resident chain, enabled with
/// `SPEKTRAFILM_GPU_PROFILE=1`: each `mark` submits the work encoded since
/// the previous mark and waits for it, so the stage's wall time is mostly GPU
/// time (plus ~0.1 ms of submit overhead). Changes nothing when disabled.
#[cfg(feature = "wgpu-backend")]
struct GpuStageProfiler<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    enabled: bool,
    last: std::time::Instant,
    rows: Vec<(&'static str, f64)>,
}

#[cfg(feature = "wgpu-backend")]
impl<'a> GpuStageProfiler<'a> {
    fn new(device: &'a wgpu::Device, queue: &'a wgpu::Queue) -> Self {
        let enabled = std::env::var_os("SPEKTRAFILM_GPU_PROFILE").is_some();
        Self { device, queue, enabled, last: std::time::Instant::now(), rows: Vec::new() }
    }

    fn mark(&mut self, name: &'static str, encoder: &mut wgpu::CommandEncoder) {
        if !self.enabled {
            return;
        }
        let done = std::mem::replace(encoder, self.device.create_command_encoder(&Default::default()));
        self.queue.submit(Some(done.finish()));
        self.device.poll(wgpu::Maintain::Wait);
        let now = std::time::Instant::now();
        self.rows.push((name, (now - self.last).as_secs_f64() * 1e3));
        self.last = now;
    }

    fn report(&self) {
        if !self.enabled {
            return;
        }
        let total: f64 = self.rows.iter().skip(1).map(|r| r.1).sum();
        for (name, ms) in &self.rows {
            eprintln!("gpu stage {name:<20} {ms:8.1} ms");
        }
        eprintln!("gpu stages total (without upload) {total:.1} ms");
    }
}

#[cfg(feature = "wgpu-backend")]
#[derive(Clone)]
struct CachedPipelineRef {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// See [`CachedPipeline::workgroup_size`].
    workgroup_size: u32,
}

#[cfg(feature = "wgpu-backend")]
struct GpuBuffer {
    data: Vec<u8>,
    binding_type: wgpu::BufferBindingType,
    usage: wgpu::BufferUsages,
}

#[cfg(feature = "wgpu-backend")]
impl GpuBuffer {
    fn uniform(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            binding_type: wgpu::BufferBindingType::Uniform,
            usage: wgpu::BufferUsages::UNIFORM,
        }
    }
    fn storage_ro(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            binding_type: wgpu::BufferBindingType::Storage { read_only: true },
            usage: wgpu::BufferUsages::STORAGE,
        }
    }
    fn storage_rw(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            binding_type: wgpu::BufferBindingType::Storage { read_only: false },
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        }
    }
}

#[cfg(feature = "wgpu-backend")]
impl ComputeBackend for WgpuBackend {
    fn colorspace_convert(&self, img: &ImageBuf, matrix: &[[f32; 3]; 3]) -> ImageBuf {
        cpu_backend::CpuBackend.colorspace_convert(img, matrix)
    }
    fn cctf_encode_srgb(&self, img: &ImageBuf) -> ImageBuf {
        cpu_backend::CpuBackend.cctf_encode_srgb(img)
    }
    fn cctf_decode_srgb(&self, img: &ImageBuf) -> ImageBuf {
        cpu_backend::CpuBackend.cctf_decode_srgb(img)
    }
    fn gaussian_blur(&self, img: &ImageBuf, sigma: f32) -> ImageBuf {
        if sigma <= 0.0 {
            return img.clone();
        }
        self.gaussian_blur_multi_gpu(img, &[sigma]).pop().unwrap()
    }
    fn gaussian_blur_multi(&self, img: &ImageBuf, sigmas: &[f32]) -> Vec<ImageBuf> {
        if sigmas.is_empty() {
            return Vec::new();
        }
        self.gaussian_blur_multi_gpu(img, sigmas)
    }
    fn table_lookup(&self, img: &ImageBuf, table_x: &[f32], table_y: &[[f32; 3]]) -> ImageBuf {
        cpu_backend::CpuBackend.table_lookup(img, table_x, table_y)
    }
    fn lut3d_interp(&self, img: &ImageBuf, lut: &Lut3D) -> ImageBuf {
        cpu_backend::CpuBackend.lut3d_interp(img, lut)
    }

    fn scan_spectral(
        &self,
        density_cmy: &ImageBuf,
        channel_density: &[[f64; 3]],
        base_density: &[f64],
        illuminant: &[f64],
        normalization: f64,
        cat: &[[f64; 3]; 3],
        xyz_to_rgb: &[[f64; 3]; 3],
    ) -> ImageBuf {
        // GPU live-preview path collapses CAT and XYZ→RGB into a single
        // matrix — small precision drop acceptable for preview, matches
        // the same trade-off as `hanatos2025_rgb_to_raw`.
        let combined: [[f64; 3]; 3] = {
            let mut out = [[0.0f64; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    out[i][j] = xyz_to_rgb[i][0] * cat[0][j]
                        + xyz_to_rgb[i][1] * cat[1][j]
                        + xyz_to_rgb[i][2] * cat[2][j];
                }
            }
            out
        };
        let xyz_to_rgb = &combined;
        let n_wl = channel_density.len();
        let n_pixels = density_cmy.pixel_count() as u32;

        // Pack uniform params — mat3x3 in WGSL std140 is 3 columns of vec4.
        // GPU shaders are f32-only: cast precision-preserved inputs at the shader boundary.
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            n_wavelengths: u32,
            normalization: f32,
            col0: [f32; 4],
            col1: [f32; 4],
            col2: [f32; 4],
            bw: [f32; 4],
        }

        let params = Params {
            width: density_cmy.width,
            height: density_cmy.height,
            n_wavelengths: n_wl as u32,
            normalization: normalization as f32,
            col0: [
                xyz_to_rgb[0][0] as f32,
                xyz_to_rgb[1][0] as f32,
                xyz_to_rgb[2][0] as f32,
                0.0,
            ],
            col1: [
                xyz_to_rgb[0][1] as f32,
                xyz_to_rgb[1][1] as f32,
                xyz_to_rgb[2][1] as f32,
                0.0,
            ],
            col2: [
                xyz_to_rgb[0][2] as f32,
                xyz_to_rgb[1][2] as f32,
                xyz_to_rgb[2][2] as f32,
                0.0,
            ],
            // Standalone scan path carries no B&W remap (the resident chain
            // is the only caller that sets it). Identity / disabled.
            // w=1: skip the shader clamp — the per-stage CPU callers
            // (glare, gamut compression, blur/unsharp) expect the
            // unclamped scan RGB, exactly like `scan_spectral_cpu` returns;
            // the final encoding happens at the end of the scanning stage.
            bw: [1.0, 0.0, 0.0, 1.0],
        };

        // GPU shaders are f32-only — narrow f64 inputs at the shader boundary.
        // Metal's compiler runs fast-math (no-NaN assumption), so we sanitize CPU-side:
        // any wavelength with a NaN input has its `base_density` bumped to +1000 so
        // `pow(10, -d) ≈ 0`, zeroing that wavelength's contribution exactly the same
        // way Python's `density_to_light` zeros NaN-bearing wavelengths.
        let (cd_flat, mut bd) = sanitize_spectral_inputs(channel_density, base_density, n_wl);
        let illu_f32: Vec<f32> = illuminant.iter().map(|&v| v as f32).collect();
        bd.resize(n_wl, 0.0);
        let input_f32 = scalars_to_f32(&density_cmy.data);
        let output_bytes = vec![0u8; n_pixels as usize * 3 * 4];

        let result = self.dispatch_compute(
            include_str!("../../spektrafilm-shaders/wgsl/scan_spectral.wgsl"),
            &[
                GpuBuffer::uniform(bytemuck::bytes_of(&params)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&input_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&cd_flat)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&bd)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&illu_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&spektrafilm_math::spectral::CMF_X_F32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&spektrafilm_math::spectral::CMF_Y_F32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&spektrafilm_math::spectral::CMF_Z_F32)),
                GpuBuffer::storage_rw(&output_bytes),
            ],
            n_pixels,
            8, // output buffer index
        );

        ImageBuf::from_data(
            density_cmy.width,
            density_cmy.height,
            f32_to_scalars(result),
        )
    }

    fn print_spectral(
        &self,
        density_cmy: &ImageBuf,
        channel_density: &[[f64; 3]],
        base_density: &[f64],
        illuminant: &[f64],
        sensitivity: &[[f64; 3]],
        normalization_factor: f64,
        preflash: [f64; 3],
    ) -> ImageBuf {
        let n_wl = channel_density.len();
        let n_pixels = density_cmy.pixel_count() as u32;

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            n_wavelengths: u32,
            normalization_factor: f32,
            preflash: [f32; 3],
            _pad: f32,
        }

        let params = Params {
            width: density_cmy.width,
            height: density_cmy.height,
            n_wavelengths: n_wl as u32,
            normalization_factor: normalization_factor as f32,
            preflash: [preflash[0] as f32, preflash[1] as f32, preflash[2] as f32],
            _pad: 0.0,
        };

        // GPU shaders are f32-only — narrow f64 inputs at the shader boundary.
        // See `sanitize_spectral_inputs` for the NaN-handling story (Metal fast-math).
        let (cd_flat, mut bd) = sanitize_spectral_inputs(channel_density, base_density, n_wl);
        let sens_flat: Vec<f32> = sensitivity
            .iter()
            .flat_map(|r| r.iter().map(|&v| if v.is_nan() { 0.0 } else { v as f32 }))
            .collect();
        bd.resize(n_wl, 0.0);
        let illu_f32: Vec<f32> = illuminant.iter().map(|&v| v as f32).collect();
        let input_f32 = scalars_to_f32(&density_cmy.data);
        let output_bytes = vec![0u8; n_pixels as usize * 3 * 4];

        let result = self.dispatch_compute(
            include_str!("../../spektrafilm-shaders/wgsl/print_spectral.wgsl"),
            &[
                GpuBuffer::uniform(bytemuck::bytes_of(&params)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&input_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&cd_flat)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&bd)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&illu_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&sens_flat)),
                GpuBuffer::storage_rw(&output_bytes),
            ],
            n_pixels,
            6, // output buffer index
        );

        ImageBuf::from_data(
            density_cmy.width,
            density_cmy.height,
            f32_to_scalars(result),
        )
    }

    fn hanatos2025_rgb_to_raw(
        &self,
        image: &ImageBuf,
        tc_lut: &spektrafilm_math::spectral::TcLut,
        color_space: &str,
        ref_illuminant: &[f32],
        cat16: bool,
        guard: bool,
    ) -> ImageBuf {
        // GPU live-preview path collapses the two-step CAT02 adaptation
        // into a single matmul — small visible-spectrum precision drop
        // that's acceptable for preview. The CPU path keeps the two-step
        // for export bit-parity with Python.
        let rgb_to_adapted_xyz = spektrafilm_math::spectral::build_rgb_to_adapted_xyz(
            color_space,
            ref_illuminant,
            cat16,
        );
        let rgb_to_adapted_xyz = &rgb_to_adapted_xyz;
        let n_pixels = image.pixel_count() as u32;
        let lut_size = tc_lut.size as u32;

        // Pack uniform: mat3x3 in std140 = 3 vec4 columns. f32 only on GPU.
        // WGSL columns are read as columns of the matrix; storing row-major as
        // [col[0], col[1], col[2]] where col[j][i] = mat[i][j].
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            lut_size: u32,
            _pad: u32,
            col0: [f32; 4],
            col1: [f32; 4],
            col2: [f32; 4],
        }

        let m = rgb_to_adapted_xyz;
        let params = Params {
            width: image.width,
            height: image.height,
            lut_size,
            _pad: 0,
            col0: [m[0][0] as f32, m[1][0] as f32, m[2][0] as f32, 0.0],
            col1: [m[0][1] as f32, m[1][1] as f32, m[2][1] as f32, 0.0],
            col2: [m[0][2] as f32, m[1][2] as f32, m[2][2] as f32, 0.0],
        };

        // TC LUT data is f64 on disk; narrow to f32 at the GPU boundary.
        let tc_lut_f32: Vec<f32> = tc_lut.data.iter().map(|&v| v as f32).collect();
        let mut input_f32 = scalars_to_f32(&image.data);
        if guard {
            let guard_params = SpectralGuardParams::new(n_pixels, m);
            input_f32 = std::borrow::Cow::Owned(self.dispatch_compute(
                SPECTRAL_GUARD_WGSL,
                &[
                    GpuBuffer::uniform(bytemuck::bytes_of(&guard_params)),
                    GpuBuffer::storage_rw(bytemuck::cast_slice(&input_f32)),
                ],
                n_pixels,
                1,
            ));
        }
        let output_bytes = vec![0u8; n_pixels as usize * 3 * 4];

        let result = self.dispatch_compute(
            include_str!("../../spektrafilm-shaders/wgsl/hanatos2025_rgb_to_raw.wgsl"),
            &[
                GpuBuffer::uniform(bytemuck::bytes_of(&params)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&input_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&tc_lut_f32)),
                GpuBuffer::storage_rw(&output_bytes),
            ],
            n_pixels,
            3, // output buffer index
        );

        ImageBuf::from_data(image.width, image.height, f32_to_scalars(result))
    }

    fn density_curve_interp(
        &self,
        log_raw: &ImageBuf,
        log_exposure: &[f64],
        density_curves: &[[f64; 3]],
        gamma_factor: f64,
    ) -> ImageBuf {
        let n_pixels = log_raw.pixel_count() as u32;
        let k = log_exposure.len() as u32;

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            k: u32,
            uniform_grid: u32,
            gamma_inv: [f32; 3],
            _pad: f32,
        }
        // Detect uniformly-spaced log_exposure (typical case) to use the fast path.
        let uniform = is_uniform(log_exposure);
        let gamma_inv = if gamma_factor.abs() > 1e-12 {
            (1.0 / gamma_factor) as f32
        } else {
            1.0
        };
        let params = Params {
            width: log_raw.width,
            height: log_raw.height,
            k,
            uniform_grid: if uniform { 1 } else { 0 },
            gamma_inv: [gamma_inv; 3],
            _pad: 0.0,
        };

        let log_exp_f32: Vec<f32> = log_exposure.iter().map(|&v| v as f32).collect();
        let curves_f32: Vec<f32> = density_curves
            .iter()
            .flat_map(|r| r.iter().map(|&v| if v.is_nan() { 0.0 } else { v as f32 }))
            .collect();
        let input_f32 = scalars_to_f32(&log_raw.data);
        let output_bytes = vec![0u8; n_pixels as usize * 3 * 4];

        let result = self.dispatch_compute(
            include_str!("../../spektrafilm-shaders/wgsl/density_curve_interp.wgsl"),
            &[
                GpuBuffer::uniform(bytemuck::bytes_of(&params)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&input_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&log_exp_f32)),
                GpuBuffer::storage_ro(bytemuck::cast_slice(&curves_f32)),
                GpuBuffer::storage_rw(&output_bytes),
            ],
            n_pixels,
            4, // output buffer index
        );

        ImageBuf::from_data(log_raw.width, log_raw.height, f32_to_scalars(result))
    }

    fn try_run_film_chain(&self, params: &crate::FilmChainParams<'_>) -> Option<ImageBuf> {
        self.film_chain_capacity(params).ok()?;
        Some(self.run_film_chain(params))
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn image_capacity(&self, width: u32, height: u32) -> Result<(), String> {
        self.capacity.image(width, height).map(|_| ())
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn film_chain_capacity(&self, params: &crate::FilmChainParams<'_>) -> Result<(), String> {
        let mut bytes = self.capacity.image(params.image.width, params.image.height)?;
        // Camera and enlarger FFT states are prepared together and remain
        // alive through readback, even though their passes run sequentially.
        for plan in params.diffusion.iter().chain(
            params.enlarger_diffusion.iter().filter(|_| !params.scan_film),
        ) {
            let radius2 = plan.psf.radius.checked_mul(2).ok_or("FFT radius overflow")?;
            let padded = |dimension: u32| -> Result<u32, String> {
                let target = dimension.checked_add(radius2).ok_or("FFT dimension overflow")?;
                // Check before searching for a smooth length; this also keeps
                // fft_size's u32 increments far away from overflow.
                self.capacity.buffer(u64::from(target) * 8, "FFT dimension")?;
                Ok(fft_size(target))
            };
            let grid = u64::from(padded(params.image.width)?)
                * u64::from(padded(params.image.height)?);
            self.capacity.buffer(grid.checked_mul(8).ok_or("FFT buffer overflow")?, "FFT grid")?;
            bytes = grid.checked_mul(20).and_then(|n| bytes.checked_add(n))
                .ok_or("FFT working-set overflow")?;
        }
        self.capacity.working_set(bytes)
    }

    fn resident_chain_applies_post_scan(&self) -> bool {
        true
    }

    fn is_gpu(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "wgpu (GPU)"
    }
}

/// Pre-built halation passes — owns the per-sigma kernel buffers, params
/// buffers, and bind groups so they live long enough for the encoder.
///
/// The CPU equivalent is `spektrafilm_model::diffusion::apply_halation_um`.
/// All passes operate on the resident image buffer (passed in as `buf_b`),
/// using `buf_a` as blur intermediate and two shared scratch buffers
/// `buf_c` / `buf_d` for scatter outputs and the halation accumulator.
#[cfg(feature = "wgpu-backend")]
struct HalationState {
    // Owns all the auxiliary buffers + bind groups; only `encode_passes` is
    // called from the hot path.
    buf_c: wgpu::Buffer,
    buf_d: wgpu::Buffer,
    scatter_blurs: Vec<RefBlur>, // [core, tail × 3]
    scatter_mix: DispatchJob,
    halation_blurs: Vec<RefBlur>, // one per bounce, accumulating into buf_d
    halation_final_add: DispatchJob,
    halation_renormalize: Option<DispatchJob>,
}

#[cfg(feature = "wgpu-backend")]
struct DispatchJob {
    _params_buf: wgpu::Buffer,
    pipeline: CachedPipelineRef,
    bg: wgpu::BindGroup,
}

/// One dispatch of a [`RefBlur`].
#[cfg(feature = "wgpu-backend")]
struct RefBlurPass {
    _params_buf: wgpu::Buffer,
    _kernel_buf: Option<wgpu::Buffer>,
    pipeline: CachedPipelineRef,
    bg: wgpu::BindGroup,
    workgroups: (u32, u32),
}

/// Per-channel separable Gaussian that reproduces the CPU reference
/// `spektrafilm_math::gaussian::gaussian_blur_channel`: FIR with
/// `radius = int(3σ + 0.5)` below σ = 3, Young–van Vliet IIR from σ = 3 on.
/// Writes `out[c] = scale[c] * blur(src[c], σ[c])`, or adds it to `out` when
/// `accumulate` is set. `mid` must differ from `src` and `out`; `out` may be
/// `src` (an in-place blur), since each dispatch binds only the buffers it
/// uses and every pass reads `src` before the last one writes `out`.
#[cfg(feature = "wgpu-backend")]
struct RefBlur {
    passes: Vec<RefBlurPass>,
}

/// Amplitude and σ/λ ratio of the three Gaussians that approximate an
/// exponential PSF. Same table as `exponential_filter_channel` (Python
/// `_EXPONENTIAL_GAUSSIAN_FITS[3]`).
#[cfg(feature = "wgpu-backend")]
#[cfg(feature = "wgpu-backend")]
const EXPONENTIAL_GAUSSIAN_FIT: [(f64, f64); 3] = [(0.1633, 0.5360), (0.6496, 1.5236), (0.1870, 2.7684)];

#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_ref_blur(
    device: &wgpu::Device,
    backend: &WgpuBackend,
    width: u32,
    height: u32,
    src: &wgpu::Buffer,
    mid: &wgpu::Buffer,
    out: &wgpu::Buffer,
    sigma: [f32; 3],
    scale: [f32; 3],
    accumulate: bool,
    label: &str,
) -> RefBlur {
    build_ref_blur_channels(device, backend, width, height, src, mid, out, sigma, scale, accumulate, [true; 3], label)
}

/// [`build_ref_blur`] for the channels set in `active` only; the others are
/// left as they are in `out`.
#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_ref_blur_channels(
    device: &wgpu::Device,
    backend: &WgpuBackend,
    width: u32,
    height: u32,
    src: &wgpu::Buffer,
    mid: &wgpu::Buffer,
    out: &wgpu::Buffer,
    sigma: [f32; 3],
    scale: [f32; 3],
    accumulate: bool,
    active: [bool; 3],
    label: &str,
) -> RefBlur {
    use wgpu::util::DeviceExt;
    let fir_layout = [
        wgpu::BufferBindingType::Uniform,
        wgpu::BufferBindingType::Storage { read_only: true },
        wgpu::BufferBindingType::Storage { read_only: true },
        wgpu::BufferBindingType::Storage { read_only: false },
    ];
    // Separate shaders per direction, so the row pass reserves only the
    // workgroup memory it needs.
    let fir_rows_pipe = backend.cached_pipeline(include_str!("../../spektrafilm-shaders/wgsl/ref_blur_fir_rows.wgsl"), &fir_layout);
    let fir_cols_pipe = backend.cached_pipeline(include_str!("../../spektrafilm-shaders/wgsl/ref_blur_fir_cols.wgsl"), &fir_layout);
    let iir_layout = [
        wgpu::BufferBindingType::Uniform,
        wgpu::BufferBindingType::Storage { read_only: true },
        wgpu::BufferBindingType::Storage { read_only: false },
        wgpu::BufferBindingType::Storage { read_only: false },
    ];
    // Rows: segmented (ref_blur_iir_rows.wgsl). Columns: one invocation per
    // channel of a column.
    let iir_rows_pipe = backend.cached_pipeline(include_str!("../../spektrafilm-shaders/wgsl/ref_blur_iir_rows.wgsl"), &iir_layout);
    let iir_cols_pipe = backend.cached_pipeline(include_str!("../../spektrafilm-shaders/wgsl/ref_blur_iir_cols.wgsl"), &iir_layout);
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct FirParams {
        width: u32,
        height: u32,
        accumulate: u32,
        radius: u32,
        mask: [u32; 4],
        scale: [f32; 4],
    }
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct IirParams {
        width: u32,
        height: u32,
        vertical: u32,
        accumulate: u32,
        mask: [u32; 4],
        scale: [f32; 4],
        g1: [f32; 4],
        g2: [f32; 4],
        c2: [f32; 4],
    }
    // Channels at σ ≥ 3 share one IIR job (one thread per line filters all of
    // them); the rest share one FIR job per direction.
    let mut iir = IirParams {
        width,
        height,
        vertical: 0,
        accumulate: accumulate as u32,
        mask: [0; 4],
        scale: [scale[0], scale[1], scale[2], 0.0],
        g1: [0.0; 4],
        g2: [0.0; 4],
        c2: [0.0; 4],
    };
    let mut passes = Vec::with_capacity(4);
    // Channels below σ = 3 share one FIR job per direction, each with its
    // own kernel (`fir_kernels`, padded to the largest radius below).
    let mut fir = FirParams {
        width,
        height,
        accumulate: 0,
        radius: 0,
        mask: [0; 4],
        scale: [1.0; 4],
    };
    let mut fir_kernels: [Vec<f32>; 3] = Default::default();
    for c in 0..3u32 {
        if !active[c as usize] {
            continue;
        }
        let s = sigma[c as usize] as f64;
        if s < 3.0 {
            // Same kernel as `make_gaussian_kernel`, built in f64.
            // σ ≤ 0 is the identity (the CPU's `gaussian_blur_channel`
            // copies), so one channel can stay unblurred among others.
            let radius = if s > 0.0 { (3.0 * s + 0.5) as u32 } else { 0 };
            let s2 = 2.0 * s * s;
            let kernel: Vec<f64> = (0..=2 * radius)
                .map(|i| {
                    let x = i as f64 - radius as f64;
                    if s > 0.0 { (-x * x / s2).exp() } else { 1.0 }
                })
                .collect();
            let sum: f64 = kernel.iter().sum();
            let ci = c as usize;
            fir.mask[ci] = 1;
            fir.radius = fir.radius.max(radius);
            fir.scale[ci] = scale[ci];
            fir_kernels[ci] = kernel.iter().map(|v| (v / sum) as f32).collect();
        } else {
            // The same filter as `iir_blur_1d`, factored into a first- and a
            // second-order section (see `IirCascade`).
            let k = IirCascade::young_van_vliet(s);
            let ci = c as usize;
            iir.mask[ci] = 1;
            iir.g1[ci] = k.g1 as f32;
            iir.g2[ci] = k.g2 as f32;
            iir.c2[ci] = k.c2 as f32;
        }
    }
    if fir.mask.iter().any(|&m| m != 0) {
        // Rows of 2·r_max + 1 taps, each kernel centred, zeros around it.
        let taps = 2 * fir.radius as usize + 1;
        let mut padded = vec![0.0f32; 3 * taps];
        for (c, k) in fir_kernels.iter().enumerate() {
            let start = c * taps + (taps - k.len()) / 2;
            padded[start..start + k.len()].copy_from_slice(k);
        }
        let kernel_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}_fir_kernels")),
            contents: bytemuck::cast_slice(&padded),
            usage: wgpu::BufferUsages::STORAGE,
        });
        for vertical in [0u32, 1] {
            let (input, output) = if vertical == 0 { (src, mid) } else { (mid, out) };
            let params = FirParams {
                accumulate: if vertical == 0 { 0 } else { accumulate as u32 },
                scale: if vertical == 0 { [1.0; 4] } else { fir.scale },
                ..fir
            };
            let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{label}_fir_params_{vertical}")),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let pipeline = if vertical == 0 { &fir_rows_pipe } else { &fir_cols_pipe };
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{label}_fir_bg_{vertical}")),
                layout: &pipeline.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: kernel_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: output.as_entire_binding() },
                ],
            });
            // 256-pixel row segments (numbered row by row, wrapped into
            // y), or 16 × 32 blocks.
            let workgroups = if vertical == 0 {
                linear_dispatch_grid(width.div_ceil(256) * height, 1)
            } else {
                (width.div_ceil(16), height.div_ceil(32))
            };
            passes.push(RefBlurPass {
                _params_buf: params_buf,
                _kernel_buf: Some(kernel_buf.clone()),
                pipeline: pipeline.clone(),
                bg,
                workgroups,
            });
        }
    }
    if iir.mask.iter().any(|&m| m != 0) {
        // The row passes read src and write mid; the column pass filters mid
        // and writes out. The column pass binds a placeholder for src, so out
        // may alias src.
        let placeholder = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label}_iir_unused")),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        // Rows: six dispatches of the segmented recursion (zero-state runs,
        // scan over segments, final runs; forward then backward).
        let segments = width.div_ceil(IIR_ROW_SEGMENT).clamp(1, 64);
        let seg_len = width.div_ceil(segments);
        let segments = width.div_ceil(seg_len);
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct RowParams {
            width: u32,
            height: u32,
            segments: u32,
            seg_len: u32,
            mode: u32,
            _pad: [u32; 3],
            mask: [u32; 4],
            g1: [f32; 4],
            g2: [f32; 4],
            c2: [f32; 4],
            a: [[f32; 4]; 9],
        }
        let mut a = [[0.0f32; 4]; 9];
        for c in 0..3 {
            if iir.mask[c] != 0 {
                let m = iir_step_matrix(iir.g1[c], iir.g2[c], iir.c2[c]);
                let p = mat3_pow(m, seg_len);
                for r in 0..3 {
                    a[3 * c + r] = [p[r][0] as f32, p[r][1] as f32, p[r][2] as f32, 0.0];
                }
            }
        }
        let state_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label}_iir_row_state")),
            size: (height as u64) * (segments as u64) * 9 * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        for mode in 0..6u32 {
            let params = RowParams {
                width,
                height,
                segments,
                seg_len,
                mode,
                _pad: [0; 3],
                mask: iir.mask,
                g1: iir.g1,
                g2: iir.g2,
                c2: iir.c2,
                a,
            };
            let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{label}_iir_row_params_{mode}")),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{label}_iir_row_bg_{mode}")),
                layout: &iir_rows_pipe.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: mid.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: state_buf.as_entire_binding() },
                ],
            });
            // Phase 1: one invocation per row and channel; phases 0 and 2:
            // one workgroup per segment of 32 rows.
            let workgroups = if mode % 3 == 1 {
                linear_dispatch_grid(3 * height, 96)
            } else {
                linear_dispatch_grid(segments * height.div_ceil(32), 1)
            };
            passes.push(RefBlurPass {
                _params_buf: params_buf,
                _kernel_buf: Some(state_buf.clone()),
                pipeline: iir_rows_pipe.clone(),
                bg,
                workgroups,
            });
        }
        // Columns: mid filtered in place, written to out.
        let params = IirParams { vertical: 1, ..iir };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}_iir_params_1")),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{label}_iir_bg_1")),
            layout: &iir_cols_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: placeholder.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: mid.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: out.as_entire_binding() },
            ],
        });
        passes.push(RefBlurPass {
            _params_buf: params_buf,
            _kernel_buf: Some(placeholder.clone()),
            pipeline: iir_cols_pipe.clone(),
            bg,
            workgroups: linear_dispatch_grid(3 * width, iir_cols_pipe.workgroup_size),
        });
    }
    RefBlur { passes }
}

/// The third-order Young–van Vliet recursion of `iir_blur_1d`,
/// `w = a·x + b1·w₁ + b2·w₂ + b3·w₃`, factored into a first-order section
/// (its real pole p) and a second-order section (its complex pole pair),
/// each normalized to unit DC gain:
///
/// ```text
/// u  = u₁ + g1·(x − u₁)                       g1 = 1 − p
/// d  = −c2·d₁ + g2·(u − y₁),  y = y₁ + d      g2 = 1 − c1 − c2
/// ```
///
/// The direct form is ill-conditioned in f32: its poles crowd towards 1 as σ
/// grows (a = 1 − Σb is 1.3e-6 at σ = 150), so each step's rounding error is
/// amplified by up to 1/a. In a numpy simulation on image rows its f32 error
/// relative to f64 was 4e-3 at σ = 20 and 0.3 at σ = 150; the cascade stays
/// at 7e-6 for every σ. With every state set to the first sample, the cascade
/// starts exactly like the CPU's direct form does with w₁ = w₂ = w₃ = x₀.
#[cfg(feature = "wgpu-backend")]
#[derive(Clone, Copy, Debug)]
struct IirCascade {
    g1: f64,
    g2: f64,
    c2: f64,
    /// For tests: the second section's other coefficient and the real pole.
    #[cfg_attr(not(test), allow(dead_code))]
    c1: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    p: f64,
}

#[cfg(feature = "wgpu-backend")]
impl IirCascade {
    /// Coefficients for σ ≥ 3, from the same b's as `iir_blur_1d`, in f64.
    fn young_van_vliet(sigma: f64) -> Self {
        let (b1, b2, b3) = young_van_vliet_b(sigma);
        // f(z) = z³ − b1·z² − b2·z − b3 has f(0) = −b3 < 0 and
        // f(1) = 1 − Σb = a > 0, and its one real root lies in between.
        let f = |z: f64| ((z - b1) * z - b2) * z - b3;
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if f(mid) < 0.0 { lo = mid } else { hi = mid }
        }
        let p = 0.5 * (lo + hi);
        // (z − p)(z² − c1·z − c2) = z³ − b1·z² − b2·z − b3
        let c1 = b1 - p;
        let c2 = -b3 / p;
        assert!(c1 * c1 + 4.0 * c2 < 0.0, "σ = {sigma}: expected one real pole and a complex pair");
        Self { g1: 1.0 - p, g2: 1.0 - c1 - c2, c2, c1, p }
    }
}

/// b1, b2, b3 of `iir_blur_1d` (σ ≥ 2.5 branch), in f64.
#[cfg(feature = "wgpu-backend")]
fn young_van_vliet_b(sigma: f64) -> (f64, f64, f64) {
    let q = 0.98711 * sigma - 0.96330;
    let (q2, q3) = (q * q, q * q * q);
    let b0 = 1.57825 + 2.44413 * q + 1.4281 * q2 + 0.422205 * q3;
    (
        (2.44413 * q + 2.85619 * q2 + 1.26661 * q3) / b0,
        -(1.4281 * q2 + 1.26661 * q3) / b0,
        (0.422205 * q3) / b0,
    )
}

/// Target samples per segment of the segmented row recursion.
#[cfg(feature = "wgpu-backend")]
const IIR_ROW_SEGMENT: u32 = 256;

/// One step of the cascade recursion as a matrix on the state (u, y, d)
/// without the input term, in f64 for the f32 coefficients the shader uses:
/// u' = (1−g1)·u, d' = g2·u' − g2·y − c2·d, y' = y + d'.
#[cfg(feature = "wgpu-backend")]
fn iir_step_matrix(g1: f32, g2: f32, c2: f32) -> [[f64; 3]; 3] {
    let (g1, g2, c2) = (g1 as f64, g2 as f64, c2 as f64);
    [
        [1.0 - g1, 0.0, 0.0],
        [g2 * (1.0 - g1), 1.0 - g2, -c2],
        [g2 * (1.0 - g1), -g2, -c2],
    ]
}

#[cfg(feature = "wgpu-backend")]
fn mat3_pow(m: [[f64; 3]; 3], mut e: u32) -> [[f64; 3]; 3] {
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
    };
    let mut result = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut base = m;
    while e > 0 {
        if e & 1 == 1 {
            result = mul(result, base);
        }
        base = mul(base, base);
        e >>= 1;
    }
    result
}

/// Blurs for `scale · exponential_filter(src, λ)` per channel: the three
/// Gaussians of [`EXPONENTIAL_GAUSSIAN_FIT`] accumulated into `out`.
#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_ref_exponential_blur(
    device: &wgpu::Device,
    backend: &WgpuBackend,
    width: u32,
    height: u32,
    src: &wgpu::Buffer,
    mid: &wgpu::Buffer,
    out: &wgpu::Buffer,
    lambda: [f32; 3],
    scale: [f32; 3],
    accumulate: bool,
    label: &str,
) -> Vec<RefBlur> {
    EXPONENTIAL_GAUSSIAN_FIT
        .iter()
        .enumerate()
        .map(|(k, &(amp, ratio))| {
            let sigma = lambda.map(|l| (ratio * l as f64) as f32);
            let s = [0, 1, 2].map(|c| (amp * scale[c] as f64) as f32);
            build_ref_blur(
                device,
                backend,
                width,
                height,
                src,
                mid,
                out,
                sigma,
                s,
                accumulate || k > 0,
                &format!("{label}_exp{k}"),
            )
        })
        .collect()
}

#[cfg(feature = "wgpu-backend")]
impl RefBlur {
    fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for p in &self.passes {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ref_blur"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&p.pipeline.pipeline);
            pass.set_bind_group(0, &p.bg, &[]);
            pass.dispatch_workgroups(p.workgroups.0, p.workgroups.1, 1);
        }
    }
}

#[cfg(feature = "wgpu-backend")]
fn build_halation_state(
    device: &wgpu::Device,
    hp: &crate::HalationGpuParams,
    width: u32,
    height: u32,
    buf_a: &wgpu::Buffer,
    buf_b: &wgpu::Buffer,
    scratch: &[wgpu::Buffer],
    backend: &WgpuBackend,
) -> HalationState {
    use wgpu::util::DeviceExt;
    let n_pixels = (width as usize) * (height as usize);
    let buf_c = scratch[0].clone();
    let buf_d = scratch[1].clone();

    // ── Scatter: core → buf_c, exponential tail → buf_d, per channel ──
    // CPU: `scattered = (1-w)·G(σ_core) + w·exp_filter(λ_tail)`, skipped
    // entirely when scatter_amount is 0.
    let scatter_blurs = if hp.scatter_amount > 0.0 {
        let mut blurs = vec![build_ref_blur(
            device,
            backend,
            width,
            height,
            buf_b,
            buf_a,
            &buf_c,
            hp.scatter_core_px,
            [1.0; 3],
            false,
            "scatter_core",
        )];
        blurs.extend(build_ref_exponential_blur(
            device,
            backend,
            width,
            height,
            buf_b,
            buf_a,
            &buf_d,
            hp.scatter_tail_px,
            [1.0; 3],
            false,
            "scatter_tail",
        ));
        blurs
    } else {
        Vec::new()
    };

    // ── Scatter mix: result = (1-sa)*result + sa*((1-atw)*core + atw*tail)
    let scatter_mix_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/scatter_mix.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct ScatterMixParams {
        n_pixels: u32,
        scatter_amount: f32,
        _pad0: u32,
        _pad1: u32,
        // vec4<f32>: per-channel tail weights, .w padding for 16-byte alignment
        tail_weight: [f32; 4],
    }
    let scatter_mix_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("halation_scatter_mix_params"),
        contents: bytemuck::bytes_of(&ScatterMixParams {
            n_pixels: n_pixels as u32,
            scatter_amount: hp.scatter_amount,
            _pad0: 0,
            _pad1: 0,
            tail_weight: [
                hp.scatter_tail_weight[0],
                hp.scatter_tail_weight[1],
                hp.scatter_tail_weight[2],
                0.0,
            ],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let scatter_mix_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("halation_scatter_mix_bg"),
        layout: &scatter_mix_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: scatter_mix_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buf_c.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buf_d.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: buf_b.as_entire_binding(),
            },
        ],
    });
    let scatter_mix_job = DispatchJob {
        _params_buf: scatter_mix_params,
        pipeline: scatter_mix_pipe,
        bg: scatter_mix_bg,
    };

    // ── Halation bounces ──────────────────────────────────────────────
    // CPU: `hb = Σ_k decay_k · G(σ_first·√(k+1))` per channel, with decay
    // normalized to sum to 1, computed in f64. Each bounce blur writes its
    // weighted result straight into the accumulator buf_d. Skipped when no
    // channel has any strength.
    let n = hp.halation_n_bounces as usize;
    let active = n >= 1 && hp.halation_a_tot.iter().any(|&a| a > 0.0);
    let mut decay: Vec<f64> = (0..n).map(|k| (hp.halation_bounce_decay as f64).powi(k as i32)).collect();
    let decay_sum: f64 = decay.iter().sum();
    for d in &mut decay {
        *d /= decay_sum;
    }
    let halation_blurs: Vec<RefBlur> = if active {
        decay
            .iter()
            .enumerate()
            .map(|(k, &wk)| {
                let sigma = hp
                    .halation_first_sigma_px
                    .map(|s| (s * ((k as f64) + 1.0).sqrt()).max(1e-6) as f32);
                build_ref_blur(
                    device,
                    backend,
                    width,
                    height,
                    buf_b,
                    buf_a,
                    &buf_d,
                    sigma,
                    [wk as f32; 3],
                    k > 0,
                    &format!("halation_bounce_{k}"),
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    // Final add: result[c] += a_tot[c] * accumulator[c]. Per-channel
    // because halation_strength varies dramatically across channels
    // (Portra zeros out blue entirely). Uses the per-channel variant
    // of add_scaled.
    let final_add_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/add_scaled_per_channel.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct AddScaledPerChannelParams {
        n_pixels: u32,
        _pad0: u32,
        _pad1: u32,
        _pad2: u32,
        scale: [f32; 4],
    }
    let final_add_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("halation_final_add_params"),
        contents: bytemuck::bytes_of(&AddScaledPerChannelParams {
            n_pixels: n_pixels as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
            scale: [
                hp.halation_a_tot[0],
                hp.halation_a_tot[1],
                hp.halation_a_tot[2],
                0.0,
            ],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let final_add_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("halation_final_add_bg"),
        layout: &final_add_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: final_add_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buf_d.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buf_b.as_entire_binding(),
            },
        ],
    });
    let halation_final_add = DispatchJob {
        _params_buf: final_add_params,
        pipeline: final_add_pipe,
        bg: final_add_bg,
    };

    // Per-channel renormalize: `result[c] /= 1 + a_tot[c]`. Uses the
    // pre-computed inverse factor passed in `inv_factor.xyz`.
    let halation_renormalize = if hp.halation_renormalize {
        let renorm_pipe = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/halation_renormalize.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: false },
            ],
        );
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct RenormParams {
            n_pixels: u32,
            _pad: u32,
            // f32x4: .xyz used, .w padding to align next vec4. Two u32
            // header fields above push this to offset 16, matching the
            // WGSL std140 layout.
            _gap: [u32; 2],
            inv_factor: [f32; 4],
        }
        let inv = [
            1.0 / (1.0 + hp.halation_a_tot[0]),
            1.0 / (1.0 + hp.halation_a_tot[1]),
            1.0 / (1.0 + hp.halation_a_tot[2]),
            0.0,
        ];
        let renorm_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("halation_renorm_params"),
            contents: bytemuck::bytes_of(&RenormParams {
                n_pixels: n_pixels as u32,
                _pad: 0,
                _gap: [0, 0],
                inv_factor: inv,
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let renorm_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("halation_renorm_bg"),
            layout: &renorm_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: renorm_params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buf_b.as_entire_binding(),
                },
            ],
        });
        Some(DispatchJob {
            _params_buf: renorm_params,
            pipeline: renorm_pipe,
            bg: renorm_bg,
        })
    } else {
        None
    };

    HalationState {
        buf_c,
        buf_d,
        scatter_blurs,
        scatter_mix: scatter_mix_job,
        halation_blurs,
        halation_final_add,
        halation_renormalize,
    }
}

#[cfg(feature = "wgpu-backend")]
impl HalationState {
    fn encode_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        n_pixels: u32,
    ) {
        let _ = (&self.buf_c, &self.buf_d); // owned, just keepalive
        let dispatch_linear = |enc: &mut wgpu::CommandEncoder, job: &DispatchJob| {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("halation_linear"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&job.pipeline.pipeline);
            pass.set_bind_group(0, &job.bg, &[]);
            dispatch_linear(&mut pass, n_pixels, job.pipeline.workgroup_size);
        };

        // Scatter
        if !self.scatter_blurs.is_empty() {
            for blur in &self.scatter_blurs {
                blur.encode(encoder);
            }
            dispatch_linear(encoder, &self.scatter_mix);
        }

        // Halation bounces
        for blur in &self.halation_blurs {
            blur.encode(encoder);
        }
        if !self.halation_blurs.is_empty() {
            dispatch_linear(encoder, &self.halation_final_add);
            if let Some(rn) = &self.halation_renormalize {
                dispatch_linear(encoder, rn);
            }
        }
    }
}

/// Smallest n ≥ `target` whose only prime factors are 2, 3 and 5 (the
/// radices `fft_stage.wgsl` implements).
#[cfg(feature = "wgpu-backend")]
fn fft_size(target: u32) -> u32 {
    let mut n = target.max(1);
    loop {
        let mut m = n;
        for p in [2, 3, 5] {
            while m % p == 0 {
                m /= p;
            }
        }
        if m == 1 {
            return n;
        }
        n += 1;
    }
}

/// Radices for an n-point Stockham FFT, 4 first.
#[cfg(feature = "wgpu-backend")]
fn fft_radices(mut n: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for p in [4, 2, 3, 5] {
        while n % p == 0 {
            out.push(p);
            n /= p;
        }
    }
    assert_eq!(n, 1, "FFT length must be 2·3·5-smooth");
    out
}

/// A 2D complex FFT over an `n1 × n2` grid held in one of two ping-pong
/// buffers: rows (length n2) then columns (length n1), all stages
/// pre-built. [`Self::encode`] returns which buffer holds the result.
#[cfg(feature = "wgpu-backend")]
struct Fft2d {
    /// (pipeline bind group, workgroups) per stage; `from_a` stages read A.
    stages: Vec<(wgpu::BindGroup, (u32, u32), bool)>,
    pipeline: CachedPipelineRef,
    _owned: Vec<wgpu::Buffer>,
}

#[cfg(feature = "wgpu-backend")]
impl Fft2d {
    /// Stages for a transform starting in A (`start_in_a`) or B.
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        backend: &WgpuBackend,
        n1: u32,
        n2: u32,
        a: &wgpu::Buffer,
        b: &wgpu::Buffer,
        tw1: &wgpu::Buffer,
        tw2: &wgpu::Buffer,
        inverse: bool,
        start_in_a: bool,
    ) -> Self {
        use wgpu::util::DeviceExt;
        let pipeline = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/fft_stage.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
                wgpu::BufferBindingType::Storage { read_only: true },
            ],
        );
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            n: u32,
            radix: u32,
            ns: u32,
            lines: u32,
            elem_stride: u32,
            line_stride: u32,
            inverse: u32,
            line_fastest: u32,
        }
        let mut stages = Vec::new();
        let mut owned = Vec::new();
        let mut in_a = start_in_a;
        // Rows: n2-point transforms, n1 lines. Columns: n1-point, n2 lines.
        for (n, lines, elem_stride, line_stride, line_fastest, tw) in
            [(n2, n1, 1u32, n2, 0u32, tw2), (n1, n2, n2, 1u32, 1u32, tw1)]
        {
            let mut ns = 1;
            for radix in fft_radices(n) {
                let params = Params { n, radix, ns, lines, elem_stride, line_stride, inverse: inverse as u32, line_fastest };
                let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("fft_stage_params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let (src, dst) = if in_a { (a, b) } else { (b, a) };
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fft_stage_bg"),
                    layout: &pipeline.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: tw.as_entire_binding() },
                    ],
                });
                stages.push((bg, linear_dispatch_grid((n / radix) * lines, pipeline.workgroup_size), in_a));
                owned.push(params_buf);
                in_a = !in_a;
                ns *= radix;
            }
        }
        Self { stages, pipeline, _owned: owned }
    }

    /// Encodes the stages; true if the result is in A.
    fn encode(&self, encoder: &mut wgpu::CommandEncoder) -> bool {
        for (bg, (x, y), _) in &self.stages {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fft_stage"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.dispatch_workgroups(*x, *y, 1);
        }
        // The last stage read from `from_a` and wrote the other buffer.
        !self.stages.last().map_or(false, |s| s.2)
    }
}

/// Pre-built diffusion-filter passes (camera or enlarger) for the resident
/// chain, in place on `buf_b`. Reproduces `apply_diffusion_filter_um`: each
/// channel is reflect-padded by the PSF radius r, convolved with the exact
/// exponential PSF (normalized to unit sum over its (2r+1)² square) by FFT,
/// cropped, and mixed `(1 − p_s)·x + p_s·blurred`. The FFT runs in f32 on an
/// n1 × n2 grid (2·3·5-smooth, ≥ h + 2r by ≥ w + 2r), so the circular
/// convolution equals the linear one on the output window. An earlier
/// version blurred a copy downsampled by up to 10× with three Gaussians per
/// exponential (worst pixel ΔE00 7–16 against the reference).
#[cfg(feature = "wgpu-backend")]
struct DiffusionState {
    /// Per channel: PSF fill, kernel FFT, spectrum, pad, image FFT,
    /// multiply, inverse FFT, extract. Built for the buffer each lands in.
    channels: Vec<DiffusionChannel>,
    pipeline: CachedPipelineRef,
    n_full: u32,
    buf_mix: wgpu::Buffer,
    _owned: Vec<wgpu::Buffer>,
}

#[cfg(feature = "wgpu-backend")]
struct DiffusionChannel {
    psf: (wgpu::BindGroup, (u32, u32)),
    kernel_fft: Fft2d,
    kspec: (wgpu::BindGroup, (u32, u32)),
    pad: (wgpu::BindGroup, (u32, u32)),
    image_fft: Fft2d,
    mul: (wgpu::BindGroup, (u32, u32)),
    inverse_fft: Fft2d,
    extract: (wgpu::BindGroup, (u32, u32)),
}

#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_diffusion_state(
    device: &wgpu::Device,
    plan: &crate::DiffusionGpuPlan,
    width: u32,
    height: u32,
    buf_b: &wgpu::Buffer,
    scratch: &[wgpu::Buffer],
    backend: &WgpuBackend,
) -> DiffusionState {
    use wgpu::util::DeviceExt;
    let psf = &plan.psf;
    let r = psf.radius;
    let n1 = fft_size(height + 2 * r);
    let n2 = fft_size(width + 2 * r);
    let n_grid = n1 as u64 * n2 as u64;
    let buf_mix = scratch[0].clone();
    let mk = |label: &str, bytes: u64| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        })
    };
    let grid_a = mk("diffusion_fft_a", n_grid * 8);
    let grid_b = mk("diffusion_fft_b", n_grid * 8);
    let kspec = mk("diffusion_kspec", n_grid * 4);
    // exp(−2πi·m/n), m < n, in f64 then rounded.
    let twiddles = |n: u32| -> Vec<f32> {
        (0..n)
            .flat_map(|m| {
                let a = -2.0 * std::f64::consts::PI * m as f64 / n as f64;
                [a.cos() as f32, a.sin() as f32]
            })
            .collect()
    };
    let storage = |label: &str, data: &[f32]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(data),
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let tw1 = storage("diffusion_tw1", &twiddles(n1));
    let tw2 = storage("diffusion_tw2", &twiddles(n2));
    let mut comps = vec![0.0f32; 3 * 16 * 2];
    for c in 0..3 {
        assert!(psf.components[c].len() <= 16, "diffusion PSF has more than 16 exponentials");
        for (k, &(lambda, amp)) in psf.components[c].iter().enumerate() {
            comps[(c * 16 + k) * 2] = lambda as f32;
            comps[(c * 16 + k) * 2 + 1] = amp as f32;
        }
    }
    let comps_buf = storage("diffusion_psf_components", &comps);

    let pipeline = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/fft_conv.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: false },
            wgpu::BufferBindingType::Storage { read_only: false },
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
            wgpu::BufferBindingType::Storage { read_only: true },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params {
        mode: u32,
        n1: u32,
        n2: u32,
        h: u32,
        w: u32,
        r: u32,
        channel: u32,
        n_comp: u32,
        p_s: f32,
        _pad: [u32; 3],
    }
    let mut owned = vec![grid_a.clone(), grid_b.clone(), kspec.clone(), tw1.clone(), tw2.clone(), comps_buf.clone()];
    let mut job = |mode: u32, channel: u32, grid: &wgpu::Buffer| {
        let params = Params {
            mode,
            n1,
            n2,
            h: height,
            w: width,
            r,
            channel,
            n_comp: psf.components[channel as usize].len() as u32,
            p_s: plan.p_s,
            _pad: [0; 3],
        };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("diffusion_fft_conv_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diffusion_fft_conv_bg"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: grid.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: kspec.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: buf_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: buf_mix.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: comps_buf.as_entire_binding() },
            ],
        });
        owned.push(params_buf);
        let threads = if mode == 4 { width * height } else { (n_grid) as u32 };
        (bg, linear_dispatch_grid(threads, pipeline.workgroup_size))
    };
    let grid = |in_a: bool| if in_a { &grid_a } else { &grid_b };
    let mut channels = Vec::new();
    for c in 0..3u32 {
        // Each FFT starts in A; where its result lands is fixed by the
        // number of stages.
        let kernel_fft = Fft2d::new(device, backend, n1, n2, &grid_a, &grid_b, &tw1, &tw2, false, true);
        let k_in_a = kernel_fft.stages.len() % 2 == 0;
        let image_fft = Fft2d::new(device, backend, n1, n2, &grid_a, &grid_b, &tw1, &tw2, false, true);
        let x_in_a = image_fft.stages.len() % 2 == 0;
        let inverse_fft = Fft2d::new(device, backend, n1, n2, &grid_a, &grid_b, &tw1, &tw2, true, x_in_a);
        let y_in_a = x_in_a == (inverse_fft.stages.len() % 2 == 0);
        channels.push(DiffusionChannel {
            psf: job(0, c, &grid_a),
            kernel_fft,
            kspec: job(1, c, grid(k_in_a)),
            pad: job(2, c, &grid_a),
            image_fft,
            mul: job(3, c, grid(x_in_a)),
            inverse_fft,
            extract: job(4, c, grid(y_in_a)),
        });
    }
    DiffusionState {
        channels,
        pipeline,
        n_full: width * height,
        buf_mix,
        _owned: owned,
    }
}

#[cfg(feature = "wgpu-backend")]
impl DiffusionState {
    /// Encode the diffusion onto `buf_b` (in place: result copied back).
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder, buf_b: &wgpu::Buffer) {
        let run = |encoder: &mut wgpu::CommandEncoder, job: &(wgpu::BindGroup, (u32, u32))| {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("diffusion_fft_conv"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &job.0, &[]);
            pass.dispatch_workgroups(job.1 .0, job.1 .1, 1);
        };
        for ch in &self.channels {
            run(encoder, &ch.psf);
            ch.kernel_fft.encode(encoder);
            run(encoder, &ch.kspec);
            run(encoder, &ch.pad);
            ch.image_fft.encode(encoder);
            run(encoder, &ch.mul);
            ch.inverse_fft.encode(encoder);
            run(encoder, &ch.extract);
        }
        encoder.copy_buffer_to_buffer(&self.buf_mix, 0, buf_b, 0, (self.n_full as u64) * 3 * 4);
    }
}

/// Camera or scanner lens blur, in place on the image buffer. CPU
/// equivalent: `backend.gaussian_blur` (`gaussian_blur_channel` per channel).
#[cfg(feature = "wgpu-backend")]
struct SimpleBlurState {
    blur: RefBlur,
}

#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_simple_blur_state(
    device: &wgpu::Device,
    sigma: f32,
    width: u32,
    height: u32,
    img: &wgpu::Buffer,
    mid: &wgpu::Buffer,
    label: &str,
    backend: &WgpuBackend,
) -> SimpleBlurState {
    SimpleBlurState {
        blur: build_ref_blur(device, backend, width, height, img, mid, img, [sigma; 3], [1.0; 3], false, label),
    }
}

#[cfg(feature = "wgpu-backend")]
impl SimpleBlurState {
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        self.blur.encode(encoder);
    }
}

#[cfg(feature = "wgpu-backend")]
struct HighlightBoostState {
    _buffers: Vec<wgpu::Buffer>,
    reductions: Vec<(DispatchJob, u32)>,
    boost: DispatchJob,
    n_values: u32,
}

#[cfg(feature = "wgpu-backend")]
fn build_highlight_boost_state(
    device: &wgpu::Device,
    hp: &crate::HighlightBoostGpuParams,
    n_pixels: u32,
    img: &wgpu::Buffer,
    backend: &WgpuBackend,
) -> HighlightBoostState {
    use wgpu::util::DeviceExt;
    let n_values = n_pixels * 3;
    let mut counts = Vec::new();
    let mut values = n_values;
    loop {
        let blocks = values.div_ceil(2048).max(1);
        counts.push(blocks);
        if blocks == 1 {
            break;
        }
        values = blocks;
    }

    let reduce_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/max_reduce.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    let boost_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/highlight_boost.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );

    let buffers: Vec<wgpu::Buffer> = counts
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("highlight_reduce_{i}")),
                size: (c as u64) * 4,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        })
        .collect();

    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct ReduceParams {
        n_values: u32,
        _pad: [u32; 7],
    }

    let mut reductions = Vec::with_capacity(counts.len());
    for (i, &blocks) in counts.iter().enumerate() {
        let pass_values = if i == 0 { n_values } else { counts[i - 1] };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("highlight_reduce_params_{i}")),
            contents: bytemuck::bytes_of(&ReduceParams {
                n_values: pass_values,
                _pad: [0; 7],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let input = if i == 0 { img } else { &buffers[i - 1] };
        let output = &buffers[i];
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("highlight_reduce_bg_{i}")),
            layout: &reduce_pipe.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        reductions.push((
            DispatchJob {
                _params_buf: params_buf,
                pipeline: reduce_pipe.clone(),
                bg,
            },
            blocks,
        ));
    }

    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct BoostParams {
        n_values: u32,
        boost_ev: f32,
        boost_range: f32,
        protect_ev: f32,
    }
    let boost_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("highlight_boost_params"),
        contents: bytemuck::bytes_of(&BoostParams {
            n_values,
            boost_ev: hp.boost_ev,
            boost_range: hp.boost_range,
            protect_ev: hp.protect_ev,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let final_max = buffers.last().expect("highlight reduction has output");
    let boost_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("highlight_boost_bg"),
        layout: &boost_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: boost_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: final_max.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: img.as_entire_binding(),
            },
        ],
    });

    HighlightBoostState {
        _buffers: buffers,
        reductions,
        boost: DispatchJob {
            _params_buf: boost_params,
            pipeline: boost_pipe,
            bg: boost_bg,
        },
        n_values,
    }
}

#[cfg(feature = "wgpu-backend")]
impl HighlightBoostState {
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        for (job, blocks) in &self.reductions {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("highlight_reduce"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&job.pipeline.pipeline);
            pass.set_bind_group(0, &job.bg, &[]);
            // One 256-thread workgroup per 2048-value block.
            let (x, y) = linear_dispatch_grid(*blocks, 1);
            pass.dispatch_workgroups(x, y, 1);
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("highlight_boost"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.boost.pipeline.pipeline);
        pass.set_bind_group(0, &self.boost.bg, &[]);
        dispatch_linear(&mut pass, self.n_values, self.boost.pipeline.workgroup_size);
    }
}

#[cfg(feature = "wgpu-backend")]
struct InplaceUnaryState {
    job: DispatchJob,
}

#[cfg(feature = "wgpu-backend")]
impl InplaceUnaryState {
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder, n_pixels: u32) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("inplace_unary"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.job.pipeline.pipeline);
        pass.set_bind_group(0, &self.job.bg, &[]);
        dispatch_linear(&mut pass, n_pixels, self.job.pipeline.workgroup_size);
    }
}

#[cfg(feature = "wgpu-backend")]
fn build_log_to_linear_state(
    device: &wgpu::Device,
    n_pixels: u32,
    scale: f32,
    img: &wgpu::Buffer,
    backend: &WgpuBackend,
) -> InplaceUnaryState {
    use wgpu::util::DeviceExt;
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params {
        n_pixels: u32,
        scale: f32,
        _pad: [u32; 2],
    }
    let pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/log_to_linear_scaled.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("print_log_to_linear_params"),
        contents: bytemuck::bytes_of(&Params {
            n_pixels,
            scale,
            _pad: [0; 2],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("print_log_to_linear_bg"),
        layout: &pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: img.as_entire_binding(),
            },
        ],
    });
    InplaceUnaryState {
        job: DispatchJob {
            _params_buf: params_buf,
            pipeline: pipe,
            bg,
        },
    }
}

#[cfg(feature = "wgpu-backend")]
fn build_linear_to_log_state(
    device: &wgpu::Device,
    n_pixels: u32,
    img: &wgpu::Buffer,
    backend: &WgpuBackend,
) -> InplaceUnaryState {
    use wgpu::util::DeviceExt;
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params {
        n_pixels: u32,
        _pad: [u32; 7],
    }
    let pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/linear_to_log10_outer.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("print_linear_to_log_params"),
        contents: bytemuck::bytes_of(&Params {
            n_pixels,
            _pad: [0; 7],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("print_linear_to_log_bg"),
        layout: &pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: img.as_entire_binding(),
            },
        ],
    });
    InplaceUnaryState {
        job: DispatchJob {
            _params_buf: params_buf,
            pipeline: pipe,
            bg,
        },
    }
}

#[cfg(feature = "wgpu-backend")]
fn build_post_scan_state(
    device: &wgpu::Device,
    n_pixels: u32,
    output_cctf_encoding: bool,
    delta: [[f32; 4]; 3],
    img: &wgpu::Buffer,
    backend: &WgpuBackend,
) -> InplaceUnaryState {
    use wgpu::util::DeviceExt;
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params {
        n_pixels: u32,
        output_cctf_encoding: u32,
        _pad: [u32; 2],
        delta: [[f32; 4]; 3],
    }
    let pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/post_scan.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("post_scan_params"),
        contents: bytemuck::bytes_of(&Params {
            n_pixels,
            output_cctf_encoding: u32::from(output_cctf_encoding),
            _pad: [0; 2],
            delta,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("post_scan_bg"),
        layout: &pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: img.as_entire_binding(),
            },
        ],
    });
    InplaceUnaryState {
        job: DispatchJob {
            _params_buf: params_buf,
            pipeline: pipe,
            bg,
        },
    }
}

/// Pre-built DIR coupler passes — owns scratch buffers, kernel buffers,
/// and bind groups so they live for the encoder.
///
/// CPU equivalent: `spektrafilm_model::couplers::apply_density_correction`.
/// Encodes per-pixel donor saturation and matmul → the Gaussian and
/// exponential blurs of the correction, weighted → in-place subtract from
/// log_raw via `add_scaled(scale=-1)`, or with the receiver saturation via
/// `dir_receiver_subtract` (positives) → re-interpolation of
/// `density_curves_0`.
#[cfg(feature = "wgpu-backend")]
struct DirState {
    _buf_correction: wgpu::Buffer, // matmul output (S0)
    _buf_diffused: wgpu::Buffer,   // diffused correction (S1)
    matmul: DispatchJob,
    blurs: Vec<RefBlur>,          // S0 → S1: (1-w)·Gaussian + w·exponential
    subtract: DispatchJob,        // buf_b -= correction
    density_curve_0: DispatchJob, // re-interp density curves
    /// Buffers we own that the bind groups reference (log_exp, curves) —
    /// kept alive for the encoder's lifetime.
    _owned: Vec<wgpu::Buffer>,
}

#[cfg(feature = "wgpu-backend")]
fn build_dir_state(
    device: &wgpu::Device,
    dp: &crate::DirCouplersGpuParams<'_>,
    width: u32,
    height: u32,
    buf_a: &wgpu::Buffer,
    buf_b: &wgpu::Buffer,
    scratch: &[wgpu::Buffer],
    backend: &WgpuBackend,
) -> DirState {
    use wgpu::util::DeviceExt;
    let n_pixels = (width as usize) * (height as usize);
    let buf_correction = scratch[0].clone();
    let buf_diffused = scratch[1].clone();

    // ── 1. dir_matmul ─────────────────────────────────────────────────
    let matmul_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/dir_matmul.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct DirMatmulParams {
        n_pixels: u32,
        positive: u32,
        _pad0: u32,
        _pad1: u32,
        density_max: [f32; 4],
        m_row0: [f32; 4],
        m_row1: [f32; 4],
        m_row2: [f32; 4],
        donor: LangmuirUniform,
    }
    let m = &dp.couplers_matrix_scaled;
    let matmul_params = DirMatmulParams {
        n_pixels: n_pixels as u32,
        positive: if dp.is_positive { 1 } else { 0 },
        _pad0: 0,
        _pad1: 0,
        density_max: [dp.density_max[0], dp.density_max[1], dp.density_max[2], 0.0],
        m_row0: [m[0][0], m[0][1], m[0][2], 0.0],
        m_row1: [m[1][0], m[1][1], m[1][2], 0.0],
        m_row2: [m[2][0], m[2][1], m[2][2], 0.0],
        donor: LangmuirUniform::new(dp.donor.as_ref()),
    };
    let matmul_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("dir_matmul_params"),
        contents: bytemuck::bytes_of(&matmul_params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let matmul_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("dir_matmul_bg"),
        layout: &matmul_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: matmul_params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buf_a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buf_correction.as_entire_binding(),
            },
        ],
    });
    let matmul = DispatchJob {
        _params_buf: matmul_params_buf,
        pipeline: matmul_pipe,
        bg: matmul_bg,
    };

    // ── 2. Diffuse the correction (S0) into S1 ───────────────────────
    // CPU: `(1-w)·G(σ_size) + w·exp_filter(λ_tail)` per channel, only when
    // σ_size > 0; otherwise the correction is used undiffused. A is the
    // blur intermediate now that the matmul has consumed the density.
    let w = dp.diffusion_tail_weight;
    let diffuse = dp.diffusion_size_px > 0.0;
    let blurs = if diffuse {
        let mut blurs = vec![build_ref_blur(
            device,
            backend,
            width,
            height,
            &buf_correction,
            buf_a,
            &buf_diffused,
            [dp.diffusion_size_px; 3],
            [1.0 - w; 3],
            false,
            "dir_size",
        )];
        blurs.extend(build_ref_exponential_blur(
            device,
            backend,
            width,
            height,
            &buf_correction,
            buf_a,
            &buf_diffused,
            [dp.diffusion_tail_px; 3],
            [w; 3],
            true,
            "dir_tail",
        ));
        blurs
    } else {
        Vec::new()
    };
    let correction = if diffuse { &buf_diffused } else { &buf_correction };

    let add_scaled_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/add_scaled.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct AddScaledParams {
        n_pixels: u32,
        scale: f32,
        clear_first: u32,
        _pad: u32,
    }

    // ── 3. Subtract: buf_b -= correction, or the receiver-saturated
    //    correction for positives ─────────────────────────────────────
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct ReceiverSubtractParams {
        n_pixels: u32,
        _pad: [u32; 3],
        receiver: LangmuirUniform,
    }
    let (subtract_pipe, subtract_params) = match dp.receiver.as_ref() {
        Some(receiver) => (
            backend.cached_pipeline(
                include_str!("../../spektrafilm-shaders/wgsl/dir_receiver_subtract.wgsl"),
                &[
                    wgpu::BufferBindingType::Uniform,
                    wgpu::BufferBindingType::Storage { read_only: true },
                    wgpu::BufferBindingType::Storage { read_only: false },
                ],
            ),
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dir_receiver_subtract_params"),
                contents: bytemuck::bytes_of(&ReceiverSubtractParams {
                    n_pixels: n_pixels as u32,
                    _pad: [0; 3],
                    receiver: LangmuirUniform::new(Some(receiver)),
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            }),
        ),
        None => (
            add_scaled_pipe,
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dir_subtract_params"),
                contents: bytemuck::bytes_of(&AddScaledParams {
                    n_pixels: n_pixels as u32,
                    scale: -1.0,
                    clear_first: 0,
                    _pad: 0,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            }),
        ),
    };
    let subtract_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("dir_subtract_bg"),
        layout: &subtract_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: subtract_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: correction.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buf_b.as_entire_binding(),
            },
        ],
    });
    let subtract = DispatchJob {
        _params_buf: subtract_params,
        pipeline: subtract_pipe,
        bg: subtract_bg,
    };

    // ── 4. Final density curve re-interp using density_curves_0 ───────
    let density_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/density_curve_interp.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    let log_exp_f32: Vec<f32> = dp.log_exposure.iter().map(|&v| v as f32).collect();
    let curves_f32: Vec<f32> = dp
        .density_curves_0
        .iter()
        .flat_map(|r| r.iter().map(|&v| if v.is_nan() { 0.0 } else { v as f32 }))
        .collect();
    let log_exp_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("dir_density_log_exp"),
        contents: bytemuck::cast_slice(&log_exp_f32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let curves_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("dir_density_curves_0"),
        contents: bytemuck::cast_slice(&curves_f32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct DensityParams {
        width: u32,
        height: u32,
        k: u32,
        uniform_grid: u32,
        gamma_inv: [f32; 3],
        _pad: f32,
    }
    let density_params = DensityParams {
        width,
        height,
        k: dp.log_exposure.len() as u32,
        uniform_grid: if is_uniform(dp.log_exposure) { 1 } else { 0 },
        gamma_inv: [1.0; 3],
        _pad: 0.0,
    };
    let density_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("dir_density_params"),
        contents: bytemuck::bytes_of(&density_params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let density_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("dir_density_bg"),
        layout: &density_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: density_params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buf_b.as_entire_binding(),
            }, // input: corrected log_raw
            wgpu::BindGroupEntry {
                binding: 2,
                resource: log_exp_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: curves_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: buf_a.as_entire_binding(),
            }, // output: corrected density_cmy
        ],
    });
    // Keep log_exp_buf / curves_buf alive via the DispatchJob's
    // _params_buf isn't ideal — pack them into the state instead.
    let density_curve_0 = DispatchJob {
        _params_buf: density_params_buf,
        pipeline: density_pipe,
        bg: density_bg,
    };

    // The two storage buffers (log_exp, curves) need to outlive the
    // encoder. Stuff them somewhere — extend DirState with a small
    // owner Vec.
    DirState {
        _buf_correction: buf_correction,
        _buf_diffused: buf_diffused,
        matmul,
        blurs,
        subtract,
        density_curve_0,
        _owned: vec![log_exp_buf, curves_buf],
    }
}

/// A Langmuir term as `dir_matmul.wgsl` and `dir_receiver_subtract.wgsl`
/// read it: `k`, `x_ref` and the per-channel on/off flag, each a vec4.
/// Linear channels get `k = 1` so the unused saturated value stays finite.
#[cfg(feature = "wgpu-backend")]
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LangmuirUniform {
    k: [f32; 4],
    x_ref: [f32; 4],
    saturating: [u32; 4],
}

#[cfg(feature = "wgpu-backend")]
impl LangmuirUniform {
    fn new(langmuir: Option<&crate::LangmuirGpuParams>) -> Self {
        let mut u = Self { k: [1.0; 4], x_ref: [0.0; 4], saturating: [0; 4] };
        if let Some(l) = langmuir {
            for c in 0..3 {
                if l.saturating[c] {
                    u.k[c] = l.k[c];
                    u.x_ref[c] = l.x_ref[c];
                    u.saturating[c] = 1;
                }
            }
        }
        u
    }
}

#[cfg(feature = "wgpu-backend")]
impl DirState {
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder, n_pixels: u32) {
        let dispatch_linear = |enc: &mut wgpu::CommandEncoder, job: &DispatchJob| {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("dir_linear"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&job.pipeline.pipeline);
            pass.set_bind_group(0, &job.bg, &[]);
            dispatch_linear(&mut pass, n_pixels, job.pipeline.workgroup_size);
        };

        dispatch_linear(encoder, &self.matmul); // buf_a → S0
        for blur in &self.blurs {
            blur.encode(encoder); // S0 → S1
        }
        dispatch_linear(encoder, &self.subtract); // buf_b -= correction
        dispatch_linear(encoder, &self.density_curve_0); // buf_b → buf_a (corrected)
    }
}

/// Pre-built unsharp mask pass — owns the blur and the combine dispatch.
///
/// CPU equivalent: `spektrafilm_model::diffusion::apply_unsharp_mask`.
/// The combine shader writes to a temp buffer (avoids same-pass
/// aliasing); `encode_passes` then `copy_buffer_to_buffer`s the result
/// back into buf_b so the rest of the pipeline doesn't need to know
/// about the temporary.
#[cfg(feature = "wgpu-backend")]
struct UnsharpState {
    blur: RefBlur,
    combine: DispatchJob,
    out_buf: wgpu::Buffer, // combine target; copied back to buf_b at the end
}

#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_unsharp_state(
    device: &wgpu::Device,
    up: &crate::UnsharpGpuParams,
    width: u32,
    height: u32,
    buf_a: &wgpu::Buffer,
    buf_b: &wgpu::Buffer,
    scratch: &[wgpu::Buffer],
    backend: &WgpuBackend,
) -> UnsharpState {
    use wgpu::util::DeviceExt;
    let n_pixels = (width as usize) * (height as usize);
    let out_buf = scratch[0].clone();
    let blur_dst = &scratch[1];

    // Blur B → S1 (A as intermediate), then combine B + S1 → S0 and copy
    // S0 → B. CPU: `backend.gaussian_blur` = `gaussian_blur_channel`.
    let blur = build_ref_blur(device, backend, width, height, buf_b, buf_a, blur_dst, [up.sigma_px; 3], [1.0; 3], false, "unsharp");

    // ── Combine: out = (1+amount)*orig - amount*blurred
    let combine_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/unsharp_combine.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct CombineParams {
        n_pixels: u32,
        amount: f32,
        _pad0: u32,
        _pad1: u32,
    }
    let combine_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("unsharp_combine_params"),
        contents: bytemuck::bytes_of(&CombineParams {
            n_pixels: n_pixels as u32,
            amount: up.amount,
            _pad0: 0,
            _pad1: 0,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let combine_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("unsharp_combine_bg"),
        layout: &combine_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: combine_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buf_b.as_entire_binding(),
            }, // a = original
            wgpu::BindGroupEntry {
                binding: 2,
                resource: blur_dst.as_entire_binding(),
            }, // b = blurred
            wgpu::BindGroupEntry {
                binding: 3,
                resource: out_buf.as_entire_binding(),
            }, // out = sharpened
        ],
    });
    let combine = DispatchJob {
        _params_buf: combine_params,
        pipeline: combine_pipe,
        bg: combine_bg,
    };

    UnsharpState {
        blur,
        combine,
        out_buf,
    }
}

#[cfg(feature = "wgpu-backend")]
impl UnsharpState {
    fn encode_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        n_pixels: u32,
        buf_b: &wgpu::Buffer,
        img_bytes: u64,
    ) {
        self.blur.encode(encoder);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("unsharp_combine"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.combine.pipeline.pipeline);
            pass.set_bind_group(0, &self.combine.bg, &[]);
            dispatch_linear(&mut pass, n_pixels, self.combine.pipeline.workgroup_size);
        }
        // Copy sharpened result back into buf_b so the downstream
        // readback sees it without needing to know we used a temp.
        encoder.copy_buffer_to_buffer(&self.out_buf, 0, buf_b, 0, img_bytes);
    }
}

/// Pre-built glare pass — owns the noise-gen dispatch, optional blur,
/// and the additive RGB apply.
///
/// CPU equivalent:
/// `spektrafilm_model::glare::{compute_random_glare_amount,
/// add_glare_with_amount}`. Operates on buf_b (the RGB output of
/// scan_spectral) in place; the lognormal noise is held in a shared
/// scratch buffer, blurred via the existing 3-channel separable
/// Gaussian.
#[cfg(feature = "wgpu-backend")]
struct GlareState {
    _scratch: wgpu::Buffer,
    gen_dispatch: DispatchJob,
    blur: Option<RefBlur>,
    apply_dispatch: DispatchJob,
}

#[cfg(feature = "wgpu-backend")]
fn build_glare_state(
    device: &wgpu::Device,
    gp: &crate::GlareGpuParams,
    width: u32,
    height: u32,
    buf_a: &wgpu::Buffer,
    buf_b: &wgpu::Buffer,
    scratch: &[wgpu::Buffer],
    backend: &WgpuBackend,
) -> GlareState {
    use wgpu::util::DeviceExt;
    let n_pixels = (width as usize) * (height as usize);
    let scratch = scratch[0].clone();

    // ── Noise generation ──────────────────────────────────────────────
    let gen_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/glare_gen.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct GenParams {
        n_pixels: u32,
        base_seed: u32,
        mu: f32,
        sigma: f32,
    }
    let gen_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("glare_gen_params"),
        contents: bytemuck::bytes_of(&GenParams {
            n_pixels: n_pixels as u32,
            base_seed: gp.base_seed,
            mu: gp.mu,
            sigma: gp.sigma,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let gen_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glare_gen_bg"),
        layout: &gen_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: gen_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: scratch.as_entire_binding(),
            },
        ],
    });
    let gen_dispatch = DispatchJob {
        _params_buf: gen_params,
        pipeline: gen_pipe,
        bg: gen_bg,
    };

    // ── Optional blur, in place on scratch via buf_a ──────────────────
    // CPU: `gaussian_blur_channel` on the single glare field. The field is
    // broadcast to all three channels but the apply pass reads channel 0
    // only, so only that channel is blurred.
    let blur = (gp.blur_px > 0.0).then(|| {
        build_ref_blur_channels(
            device,
            backend,
            width,
            height,
            &scratch,
            buf_a,
            &scratch,
            [gp.blur_px; 3],
            [1.0; 3],
            false,
            [true, false, false],
            "glare",
        )
    });

    // ── Apply: image[c] += glare_amount * rgb_offset[c] ───────────────
    let apply_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/glare_apply.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct ApplyParams {
        n_pixels: u32,
        _pad0: u32,
        _pad1: u32,
        _pad2: u32,
        offset: [f32; 4],
    }
    let apply_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("glare_apply_params"),
        contents: bytemuck::bytes_of(&ApplyParams {
            n_pixels: n_pixels as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
            offset: [gp.rgb_offset[0], gp.rgb_offset[1], gp.rgb_offset[2], 0.0],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let apply_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glare_apply_bg"),
        layout: &apply_pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: apply_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: scratch.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buf_b.as_entire_binding(),
            },
        ],
    });
    let apply_dispatch = DispatchJob {
        _params_buf: apply_params,
        pipeline: apply_pipe,
        bg: apply_bg,
    };

    GlareState {
        _scratch: scratch,
        gen_dispatch,
        blur,
        apply_dispatch,
    }
}

#[cfg(feature = "wgpu-backend")]
impl GlareState {
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder, n_pixels: u32) {
        // 1. Noise generation.
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("glare_gen"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.gen_dispatch.pipeline.pipeline);
            pass.set_bind_group(0, &self.gen_dispatch.bg, &[]);
            dispatch_linear(&mut pass, n_pixels, self.gen_dispatch.pipeline.workgroup_size);
        }
        // 2. Optional blur.
        if let Some(b) = &self.blur {
            b.encode(encoder);
        }
        // 3. Apply onto buf_b.
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("glare_apply"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.apply_dispatch.pipeline.pipeline);
            pass.set_bind_group(0, &self.apply_dispatch.bg, &[]);
            dispatch_linear(&mut pass, n_pixels, self.apply_dispatch.pipeline.workgroup_size);
        }
    }
}

/// Pre-built output gamut compression pass — a single per-pixel dispatch
/// in place on `buf_b` (the scan output), between glare and unsharp,
/// matching the CPU scanning order. Owns the uniform + `C_max` table
/// buffers so they outlive the encoder.
///
/// CPU equivalent: `OutputGamutCompress::compress`.
#[cfg(feature = "wgpu-backend")]
struct GamutState {
    _cmax_buf: wgpu::Buffer,
    dispatch: DispatchJob,
}

#[cfg(feature = "wgpu-backend")]
fn build_gamut_state(
    device: &wgpu::Device,
    gp: &crate::GamutGpuParams<'_>,
    buf_b: &wgpu::Buffer,
    n_pixels: u32,
    backend: &WgpuBackend,
) -> GamutState {
    use wgpu::util::DeviceExt;
    let pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/gamut_compress.wgsl"),
        &[
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferBindingType::Storage { read_only: true },
            wgpu::BufferBindingType::Storage { read_only: false },
        ],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct GamutParams {
        n_pixels: u32,
        mode: u32,
        n_l: u32,
        n_h: u32,
        knee: [f32; 4],
        lightness: [f32; 4],
        lspace: [f32; 4],
        rgb_to_xyz: [[f32; 4]; 3],
        xyz_to_rgb: [[f32; 4]; 3],
        cam16_white: [f32; 4],
    }
    let rows = |m: [[f32; 3]; 3]| m.map(|r| [r[0], r[1], r[2], 0.0]);
    let lightness = match gp.lightness {
        Some([t, l, p]) => [t, l, p, 1.0],
        None => [0.0, 1.0, 1.0, 0.0],
    };
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gamut_params"),
        contents: bytemuck::bytes_of(&GamutParams {
            n_pixels,
            mode: gp.mode,
            n_l: gp.n_l,
            n_h: gp.n_h,
            knee: [gp.knee[0], gp.knee[1], gp.knee[2], 0.0],
            lightness,
            lspace: [gp.l_white, gp.l_min, gp.l_max, 0.0],
            rgb_to_xyz: rows(gp.rgb_to_xyz),
            xyz_to_rgb: rows(gp.xyz_to_rgb),
            cam16_white: [gp.cam16_d_rgb[0], gp.cam16_d_rgb[1], gp.cam16_d_rgb[2], gp.cam16_a_w],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    // aces_rgc carries no table — bind a 1-element placeholder (the shader
    // never reads it on that mode; wgpu requires a non-empty binding).
    let cmax_f32: Vec<f32> = if gp.cmax.is_empty() {
        vec![0.0]
    } else {
        gp.cmax.iter().map(|&v| v as f32).collect()
    };
    let cmax_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gamut_cmax"),
        contents: bytemuck::cast_slice(&cmax_f32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gamut_bg"),
        layout: &pipe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: cmax_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buf_b.as_entire_binding(),
            },
        ],
    });
    GamutState {
        _cmax_buf: cmax_buf,
        dispatch: DispatchJob {
            _params_buf: params_buf,
            pipeline: pipe,
            bg,
        },
    }
}

#[cfg(feature = "wgpu-backend")]
impl GamutState {
    fn encode_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        n_pixels: u32,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gamut_compress"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.dispatch.pipeline.pipeline);
        pass.set_bind_group(0, &self.dispatch.bg, &[]);
        dispatch_linear(&mut pass, n_pixels, self.dispatch.pipeline.workgroup_size);
    }
}

/// Pre-built grain passes: upstream experimental's multilayer model.
///
/// CPU equivalent: `spektrafilm_model::grain::apply_grain`. Buffers: `den`
/// (buf_a) holds the density in and out, `layer` (buf_b, free after DIR's
/// final density curve) one sub-layer's draws, `mid` (scratch 0) the blurs'
/// intermediate and `acc` (scratch 1) the sum of the sub-layers. Passes:
///
/// 1. per sub-layer: draw (`grain.wgsl`), then the dye-cloud blur into `acc`
///    (accumulating after the first); without a blur the draw adds itself;
/// 2. micro-structure (`grain_micro.wgsl`), only at very small pixels;
/// 3. the pixel blur `acc` → `layer`;
/// 4. the multiplicative unsharp mask: clamp, blur, combine
///    (`grain_usm.wgsl`), the per-channel sums (`grain_sum_reduce.wgsl`);
/// 5. the renormalization and floor removal into `den` (`grain_finalize.wgsl`).
///
/// The renormalization sums the whole image, as upstream's does.
#[cfg(feature = "wgpu-backend")]
struct GrainState {
    _buffers: Vec<wgpu::Buffer>,
    passes: Vec<GrainPass>,
}

#[cfg(feature = "wgpu-backend")]
enum GrainPass {
    /// A per-pixel dispatch over `n` threads at the pipeline's group size.
    Linear(DispatchJob, u32),
    /// One 256-thread workgroup per 2048-value block.
    Blocks(DispatchJob, u32),
    Blur(RefBlur),
}

#[cfg(feature = "wgpu-backend")]
#[allow(clippy::too_many_arguments)]
fn build_grain_state(
    device: &wgpu::Device,
    gp: &crate::GrainGpuParams,
    width: u32,
    height: u32,
    den: &wgpu::Buffer,
    layer_buf: &wgpu::Buffer,
    mid: &wgpu::Buffer,
    acc: &wgpu::Buffer,
    backend: &WgpuBackend,
) -> GrainState {
    use wgpu::util::DeviceExt;
    let n_pixels = width * height;
    let n_layers = gp.n_layers();
    let n_samples = gp.interp_x.len();
    let storage = |read_only| wgpu::BufferBindingType::Storage { read_only };
    let uniform = wgpu::BufferBindingType::Uniform;
    let job = |pipe: &CachedPipelineRef, label: &str, params: &[u8], buffers: &[&wgpu::Buffer]| {
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() }];
        for (i, b) in buffers.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: b.as_entire_binding() });
        }
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &pipe.layout,
            entries: &entries,
        });
        DispatchJob { _params_buf: params_buf, pipeline: pipe.clone(), bg }
    };
    let vec4 = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
    let mut buffers = Vec::new();
    let mut passes = Vec::new();

    // 1. Sub-layers.
    let draw_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/grain.wgsl"),
        &[uniform, storage(true), storage(true), storage(false)],
    );
    let mut curves: Vec<f32> = Vec::with_capacity(n_samples * (6 + 3 * n_layers));
    curves.extend(gp.interp_x.iter().flatten());
    curves.extend(gp.interp_inv_dx.iter().flatten());
    curves.extend(gp.curves_layers.iter().flatten());
    let curves_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("grain_curves"),
        contents: bytemuck::cast_slice(&curves),
        usage: wgpu::BufferUsages::STORAGE,
    });
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct DrawParams {
        n_pixels: u32,
        n_samples: u32,
        n_layers: u32,
        layer: u32,
        base_seed: u32,
        monochrome: u32,
        accumulate: u32,
        positive: u32,
        density_min: [f32; 4],
        density_max: [f32; 4],
        n_particles_per_pixel: [f32; 4],
        grain_uniformity: [f32; 4],
    }
    for l in 0..n_layers {
        let sigma = gp.dye_cloud_sigma_px[l];
        let blurred = sigma.iter().any(|&s| s > 0.0);
        let params = DrawParams {
            n_pixels,
            n_samples: n_samples as u32,
            n_layers: n_layers as u32,
            layer: l as u32,
            base_seed: gp.base_seed,
            monochrome: gp.monochrome as u32,
            accumulate: (!blurred && l > 0) as u32,
            positive: gp.positive as u32,
            density_min: vec4(gp.density_min_layers[l]),
            density_max: vec4(gp.density_max_layers[l]),
            n_particles_per_pixel: vec4(gp.n_particles_per_pixel[l]),
            grain_uniformity: vec4(gp.grain_uniformity),
        };
        let out = if blurred { layer_buf } else { acc };
        passes.push(GrainPass::Linear(
            job(&draw_pipe, "grain_draw", bytemuck::bytes_of(&params), &[den, &curves_buf, out]),
            n_pixels,
        ));
        if blurred {
            passes.push(GrainPass::Blur(build_ref_blur(
                device, backend, width, height, layer_buf, mid, acc, sigma, [1.0; 3], l > 0, "grain_dye",
            )));
        }
    }
    buffers.push(curves_buf);

    // 2. Micro-structure.
    if let Some([sigma2, blur_px]) = gp.micro_structure {
        let pipe = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/grain_micro.wgsl"),
            &[uniform, storage(false), storage(false)],
        );
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct MicroParams {
            n_pixels: u32,
            base_seed: u32,
            mode: u32,
            monochrome: u32,
            sigma2: f32,
            _pad: [f32; 3],
        }
        let params = |mode| MicroParams {
            n_pixels,
            base_seed: gp.base_seed,
            mode,
            monochrome: gp.monochrome as u32,
            sigma2,
            _pad: [0.0; 3],
        };
        if blur_px > 0.0 {
            passes.push(GrainPass::Linear(job(&pipe, "grain_micro", bytemuck::bytes_of(&params(0)), &[acc, layer_buf]), n_pixels));
            passes.push(GrainPass::Blur(build_ref_blur(
                device, backend, width, height, layer_buf, mid, layer_buf, [blur_px; 3], [1.0; 3], false, "grain_micro",
            )));
            passes.push(GrainPass::Linear(job(&pipe, "grain_micro", bytemuck::bytes_of(&params(1)), &[acc, layer_buf]), n_pixels));
        } else {
            passes.push(GrainPass::Linear(job(&pipe, "grain_micro", bytemuck::bytes_of(&params(2)), &[acc, layer_buf]), n_pixels));
        }
    }

    // 3. Pixel blur.
    let (cur, other) = if gp.blur_px > 0.0 {
        passes.push(GrainPass::Blur(build_ref_blur(
            device, backend, width, height, acc, mid, layer_buf, [gp.blur_px; 3], [1.0; 3], false, "grain",
        )));
        (layer_buf, acc)
    } else {
        (acc, layer_buf)
    };

    // 4. Multiplicative unsharp mask.
    let finalize_pipe = backend.cached_pipeline(
        include_str!("../../spektrafilm-shaders/wgsl/grain_finalize.wgsl"),
        &[uniform, storage(true), storage(true), storage(false)],
    );
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct FinalizeParams {
        n_pixels: u32,
        use_ratio: u32,
        _pad: [u32; 2],
        density_min: [f32; 4],
    }
    let finalize = |use_ratio: bool, src: &wgpu::Buffer, sums: &wgpu::Buffer| {
        let params = FinalizeParams {
            n_pixels,
            use_ratio: use_ratio as u32,
            _pad: [0; 2],
            density_min: vec4(gp.density_min),
        };
        GrainPass::Linear(job(&finalize_pipe, "grain_finalize", bytemuck::bytes_of(&params), &[src, sums, den]), n_pixels)
    };
    if let Some([sigma, amount]) = gp.mult_usm {
        let usm_pipe = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/grain_usm.wgsl"),
            &[uniform, storage(false), storage(false)],
        );
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct UsmParams {
            n_pixels: u32,
            mode: u32,
            amount: f32,
            _pad: u32,
        }
        let usm = |mode| UsmParams { n_pixels, mode, amount, _pad: 0 };
        passes.push(GrainPass::Linear(job(&usm_pipe, "grain_usm_clamp", bytemuck::bytes_of(&usm(0)), &[cur, other]), n_pixels));
        passes.push(GrainPass::Blur(build_ref_blur(
            device, backend, width, height, cur, mid, other, [sigma; 3], [1.0; 3], false, "grain_usm",
        )));
        passes.push(GrainPass::Linear(job(&usm_pipe, "grain_usm_combine", bytemuck::bytes_of(&usm(1)), &[cur, other]), n_pixels));

        // Sums of `cur` (before) and `other` (after), level by level.
        let reduce_pipe = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/grain_sum_reduce.wgsl"),
            &[uniform, storage(true), storage(true), storage(false)],
        );
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct ReduceParams {
            n_values: u32,
            mode: u32,
            _pad: [u32; 2],
        }
        let mut values = n_pixels;
        let mut level = 0;
        loop {
            let blocks = values.div_ceil(2048).max(1);
            let out = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("grain_sums_{level}")),
                size: blocks as u64 * 6 * 4,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let params = ReduceParams { n_values: values, mode: (level > 0) as u32, _pad: [0; 2] };
            let reduce = if level == 0 {
                job(&reduce_pipe, "grain_sum_reduce", bytemuck::bytes_of(&params), &[cur, other, &out])
            } else {
                let prev = buffers.last().expect("previous level");
                job(&reduce_pipe, "grain_sum_reduce", bytemuck::bytes_of(&params), &[prev, prev, &out])
            };
            passes.push(GrainPass::Blocks(reduce, blocks));
            buffers.push(out);
            if blocks == 1 {
                break;
            }
            values = blocks;
            level += 1;
        }
        // 5. Renormalize and remove the floor.
        passes.push(finalize(true, other, buffers.last().expect("sums")));
    } else {
        let unused = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grain_sums_unused"),
            size: 32,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        passes.push(finalize(false, cur, &unused));
        buffers.push(unused);
    }

    GrainState { _buffers: buffers, passes }
}

#[cfg(feature = "wgpu-backend")]
impl GrainState {
    fn encode_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        for p in &self.passes {
            match p {
                GrainPass::Blur(b) => b.encode(encoder),
                GrainPass::Linear(job, n) | GrainPass::Blocks(job, n) => {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("grain"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&job.pipeline.pipeline);
                    pass.set_bind_group(0, &job.bg, &[]);
                    let wg = if matches!(p, GrainPass::Blocks(..)) { 1 } else { job.pipeline.workgroup_size };
                    let (x, y) = linear_dispatch_grid(*n, wg);
                    pass.dispatch_workgroups(x, y, 1);
                }
            }
        }
    }
}

/// Detects whether a 1D sequence is uniformly spaced (within tight tolerance).
#[cfg(feature = "wgpu-backend")]
fn is_uniform(xs: &[f64]) -> bool {
    if xs.len() < 3 {
        return true;
    }
    let step = (xs[xs.len() - 1] - xs[0]) / (xs.len() as f64 - 1.0);
    let tol = step.abs() * 1e-9 + 1e-12;
    for i in 1..xs.len() {
        let expected = xs[0] + (i as f64) * step;
        if (xs[i] - expected).abs() > tol {
            return false;
        }
    }
    true
}

#[cfg(all(test, feature = "wgpu-backend"))]
mod grain_tests {
    use super::*;
    use wgpu::util::DeviceExt;

    #[test]
    fn linear_dispatch_grid_covers_every_thread_within_limits() {
        // Up to 150 MP, and the 3-values-per-pixel highlight-boost pass at
        // 100 MP. The overshoot is less than one row of workgroups.
        for wg in [1, GRAIN_WORKGROUP_SIZE, LINEAR_WORKGROUP_SIZE] {
            for n in [0, 1, 255, 256, 257, 24_337_152, 60_318_720, 67_107_840, 67_108_865, 100_663_296, 150_000_000, 301_989_888] {
                let (x, y) = linear_dispatch_grid(n, wg);
                assert!((1..=65535).contains(&x) && (1..=65535).contains(&y), "{n}/{wg}: {x} × {y}");
                let threads = x as u64 * y as u64 * wg as u64;
                assert!(threads >= n as u64, "{n}/{wg}: {x} × {y}");
                assert!(threads < n as u64 + x as u64 * wg as u64 + wg as u64, "{n}/{wg}: {x} × {y}");
            }
        }
    }

    /// The Metal threadgroup query must agree with what Apple GPUs are known
    /// to run: every shader's default size. (The earlier single-pass grain shader
    /// could not run 1024 threads, hence grain's default of 256; the
    /// per-sub-layer shader that replaced it can, and keeps 256.)
    #[cfg(target_os = "macos")]
    #[test]
    fn metal_threadgroup_limits_match_apple_gpus() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let Some(device) = metal_threadgroups::raw_device(&backend.device) else { return };
        if !metal_threadgroups::is_apple_gpu(&device) {
            eprintln!("not an Apple GPU; skipping");
            return;
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../spektrafilm-shaders/wgsl");
        let mut paths: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        for path in paths {
            let source = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let entries = layout_from_wgsl(&source);
            let declared = declared_workgroup_size(&source);
            let limit = metal_threadgroups::max_threads(&device, &source, &entries, declared)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let threads = declared.or_else(|| fixed_workgroup_threads(&source)).unwrap();
            eprintln!("{name:32} {threads:5} threads, pipeline limit {limit}");
            assert!(threads <= limit, "{name}: {threads} threads > limit {limit}");
        }
    }

    /// One bind group of buffers as the shader declares them.
    #[cfg(target_os = "macos")]
    fn layout_from_wgsl(source: &str) -> Vec<wgpu::BindGroupLayoutEntry> {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(source).unwrap();
        let mut bindings: Vec<_> = module
            .global_variables
            .iter()
            .filter_map(|(_, var)| {
                let ty = match var.space {
                    naga::AddressSpace::Uniform => wgpu::BufferBindingType::Uniform,
                    naga::AddressSpace::Storage { access } => wgpu::BufferBindingType::Storage {
                        read_only: !access.contains(naga::StorageAccess::STORE),
                    },
                    _ => return None,
                };
                Some((var.binding.as_ref()?.binding, ty))
            })
            .collect();
        bindings.sort_by_key(|&(binding, _)| binding);
        bindings
            .into_iter()
            .map(|(binding, ty)| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            })
            .collect()
    }

    /// The front pass agrees with the CPU at the spectral boundary: negative
    /// coordinates, x + y > 1, x > 1, no light and above-one values.
    #[test]
    fn front_pass_boundary_matches_cpu() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        use spektrafilm_math::precision::{from_f64, to_f64};
        // A smooth synthetic table, so the comparison tests the guard and not
        // the LUT's contents.
        let size = 33;
        let mut data = Vec::with_capacity(size * size * 3);
        for i in 0..size {
            for j in 0..size {
                let (u, v) = (i as f64 / 32.0, j as f64 / 32.0);
                data.extend([0.2 + 0.5 * u, 0.3 + 0.4 * v * u, 0.6 - 0.3 * v]);
            }
        }
        let lut = spektrafilm_math::spectral::TcLut { size, channels: 3, data };
        let pixels: [[f64; 3]; 9] = [
            [0.18, 0.18, 0.18],
            [4.0, 3.5, 2.0],
            [-0.2247, 1.0420, -0.0786],
            [1.0, -0.3, -0.4],
            [-0.05, -0.02, 0.9],
            [0.0, 0.0, 0.0],
            [-0.1, -0.1, -0.1],
            [0.9, 0.0, -0.5],
            [0.02, -0.01, 0.0],
        ];
        let image = ImageBuf::from_data(pixels.len() as u32, 1, pixels.iter().flatten().map(|&v| from_f64(v)).collect());
        let illuminant = spektrafilm_math::spectral::ILLUMINANT_D55.to_vec();
        for guard in [true, false] {
            let cpu = spektrafilm_math::spectral::hanatos2025_rgb_to_raw(&image, &lut, "sRGB", &illuminant, false, guard);
            let gpu = backend.hanatos2025_rgb_to_raw(&image, &lut, "sRGB", &illuminant, false, guard);
            for (i, (c, g)) in cpu.data.iter().zip(&gpu.data).enumerate() {
                let (c, g) = (to_f64(*c), to_f64(*g));
                assert!(g.is_finite(), "guard {guard}, pixel {}: {g}", i / 3);
                // f32 maths and the GPU's fused matrix against the CPU's f64.
                assert!((c - g).abs() <= 2e-4 * c.abs().max(1.0), "guard {guard}, pixel {}: cpu {c} gpu {g}", i / 3);
            }
        }
    }

    /// Render a cheap flat shader over a grid that is forced to wrap into y
    /// (as every pass does above 67 MP) and check each pixel is written
    /// exactly once: `dst += 1 · src` with src = 1 leaves 0 where a thread
    /// was missed and 2 where one ran twice.
    #[test]
    fn wrapped_linear_dispatch_writes_every_pixel_once() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let pipe = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/add_scaled.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
            ],
        );
        let n_pixels = 10 * pipe.workgroup_size + 5; // 11 groups
        let (x, y) = linear_dispatch_grid_capped(n_pixels, pipe.workgroup_size, 4);
        assert_eq!((x, y), (4, 3), "test must exercise an overshooting 2D grid");
        let n_values = n_pixels as usize * 3;
        // One spare pixel past the end must stay untouched too.
        let ones = vec![1.0f32; n_values + 3];
        let src = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wrap_src"),
            contents: bytemuck::cast_slice(&ones),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dst = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wrap_dst"),
            contents: bytemuck::cast_slice(&vec![0.0f32; n_values + 3]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let params: [u32; 4] = [n_pixels, 1.0f32.to_bits(), 0, 0];
        let params = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wrap_params"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bg = backend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wrap_bg"),
            layout: &pipe.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
            ],
        });
        let out = run_and_read(&backend, &dst, (n_values + 3) * 4, |encoder| {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipe.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(x, y, 1);
        });
        let wrong: Vec<usize> = (0..n_values).filter(|&i| out[i] != 1.0).collect();
        assert!(wrong.is_empty(), "{} values not written exactly once, first at {:?}", wrong.len(), wrong.first());
        assert_eq!(&out[n_values..], &[0.0; 3], "wrote past n_pixels");
    }

    /// The highlight-boost max reduction over a wrapped grid of blocks (it
    /// wraps above 134 M values, i.e. 45 MP) must still find the maximum,
    /// wherever it sits.
    #[test]
    fn wrapped_max_reduce_finds_the_maximum() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let n_values: u32 = 2048 * 10 + 7; // 11 blocks
        let blocks = n_values.div_ceil(2048);
        let (x, y) = linear_dispatch_grid_capped(blocks, 1, 4);
        assert_eq!((x, y), (4, 3));
        let pipe = backend.cached_pipeline(
            include_str!("../../spektrafilm-shaders/wgsl/max_reduce.wgsl"),
            &[
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
            ],
        );
        for peak_at in [0, 2048 * 5 + 3, n_values - 1] {
            let data: Vec<f32> = (0..n_values).map(|i| if i == peak_at { 7.5 } else { (i % 97) as f32 * 0.01 }).collect();
            let input = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("reduce_in"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::STORAGE,
            });
            // One sentinel slot past the last block must survive.
            let output = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("reduce_out"),
                contents: bytemuck::cast_slice(&vec![-1.0f32; blocks as usize + 1]),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
            let params: [u32; 8] = [n_values, 0, 0, 0, 0, 0, 0, 0];
            let params = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("reduce_params"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bg = backend.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("reduce_bg"),
                layout: &pipe.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: output.as_entire_binding() },
                ],
            });
            let out = run_and_read(&backend, &output, (blocks as usize + 1) * 4, |encoder| {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipe.pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(x, y, 1);
            });
            for b in 0..blocks as usize {
                let lo = b * 2048;
                let hi = (lo + 2048).min(n_values as usize);
                let expected = data[lo..hi].iter().cloned().fold(f32::MIN, f32::max);
                assert_eq!(out[b], expected, "block {b}, peak at {peak_at}");
            }
            assert_eq!(out[blocks as usize], -1.0, "wrote past the last block");
        }
    }

    /// The cascade multiplies back out to the direct form's polynomial for
    /// every σ the reference filters recursively.
    #[test]
    fn iir_cascade_factors_the_young_van_vliet_filter() {
        let mut sigma = 3.0f64;
        while sigma < 2000.0 {
            let (b1, b2, b3) = young_van_vliet_b(sigma);
            let k = IirCascade::young_van_vliet(sigma);
            // (z − p)(z² − c1·z − c2) = z³ − (p + c1)·z² + (p·c1 − c2)·z + p·c2
            assert!((k.p + k.c1 - b1).abs() < 1e-12, "σ {sigma}");
            assert!((-(k.p * k.c1 - k.c2) - b2).abs() < 1e-12, "σ {sigma}");
            assert!((-k.p * k.c2 - b3).abs() < 1e-12, "σ {sigma}");
            assert!((k.g1 * k.g2 - (1.0 - b1 - b2 - b3)).abs() < 1e-12, "σ {sigma}: gain");
            assert!(k.p > 0.0 && k.p < 1.0 && k.c2 < 0.0 && k.c2 > -1.0, "σ {sigma}: stable");
            sigma *= 1.07;
        }
    }

    /// `RefBlur` on the GPU against the CPU's recursion (`iir_filter_row`,
    /// rows then columns) run in f64, at a small, a medium and the largest
    /// σ the pipeline uses (the DIR tail on a 100 MP frame). Simulated in
    /// numpy, the f32 direct form drifted by up to 40% of the mean on image
    /// columns at σ = 190; the cascade measures about 1.5e-6 here.
    #[test]
    fn ref_blur_iir_matches_f64_recursion() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let (w, h) = (2048usize, 700usize);
        let sigma = [3.0f32, 40.0, 190.0];
        // A smooth random walk plus edges, values 0.05..~2 like raw exposure.
        let mut state = 0x9e3779b97f4a7c15u64;
        let mut rnd = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut src = vec![0.0f64; w * h * 3];
        for c in 0..3 {
            for y in 0..h {
                let mut v = 0.5 + 0.3 * rnd();
                for x in 0..w {
                    v = (v + 0.02 * (rnd() - 0.5)).clamp(0.05, 1.5);
                    let edge = if (x / 300 + y / 200) % 2 == 0 { 0.4 } else { 0.0 };
                    src[(y * w + x) * 3 + c] = v + edge;
                }
            }
        }
        // f64 reference: direct form, exactly as iir_filter_row.
        let filter = |line: &mut Vec<f64>, b: (f64, f64, f64)| {
            let (b1, b2, b3) = b;
            let a = 1.0 - b1 - b2 - b3;
            let n = line.len();
            let (mut w1, mut w2, mut w3) = (line[0], line[0], line[0]);
            for j in 0..n {
                let v = a * line[j] + b1 * w1 + b2 * w2 + b3 * w3;
                line[j] = v;
                (w3, w2, w1) = (w2, w1, v);
            }
            let xn = line[n - 1];
            (w1, w2, w3) = (xn, xn, xn);
            for j in (0..n).rev() {
                let v = a * line[j] + b1 * w1 + b2 * w2 + b3 * w3;
                line[j] = v;
                (w3, w2, w1) = (w2, w1, v);
            }
        };
        let mut expected = src.clone();
        for c in 0..3 {
            let b = young_van_vliet_b(sigma[c] as f64);
            for y in 0..h {
                let mut line: Vec<f64> = (0..w).map(|x| expected[(y * w + x) * 3 + c]).collect();
                filter(&mut line, b);
                for x in 0..w {
                    expected[(y * w + x) * 3 + c] = line[x];
                }
            }
            for x in 0..w {
                let mut line: Vec<f64> = (0..h).map(|y| expected[(y * w + x) * 3 + c]).collect();
                filter(&mut line, b);
                for y in 0..h {
                    expected[(y * w + x) * 3 + c] = line[y];
                }
            }
        }
        let src_f32: Vec<f32> = src.iter().map(|&v| v as f32).collect();
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let src_buf = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("iir_test_src"),
            contents: bytemuck::cast_slice(&src_f32),
            usage,
        });
        let mk = |label| {
            backend.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (w * h * 3 * 4) as u64,
                usage,
                mapped_at_creation: false,
            })
        };
        let (mid, out) = (mk("iir_test_mid"), mk("iir_test_out"));
        let blur = build_ref_blur(&backend.device, &backend, w as u32, h as u32, &src_buf, &mid, &out, sigma, [1.0; 3], false, "iir_test");
        let got = run_and_read(&backend, &out, w * h * 3 * 4, |encoder| blur.encode(encoder));
        for c in 0..3 {
            let err = (0..w * h).map(|i| (got[i * 3 + c] as f64 - expected[i * 3 + c]).abs()).fold(0.0, f64::max);
            eprintln!("σ {}: max |GPU − f64| = {err:.2e}", sigma[c]);
            assert!(err < 2e-5, "σ {}: max error {err}", sigma[c]);
        }
    }

    /// In place (`out` = `src`) gives the same result as into a separate
    /// buffer, for FIR and IIR channels together, and inactive channels are
    /// left alone.
    #[test]
    fn ref_blur_in_place_and_channel_mask() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let (w, h) = (300usize, 200usize);
        let data: Vec<f32> = (0..w * h * 3).map(|i| ((i * 7919) % 1000) as f32 / 1000.0).collect();
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let mk = |label: &str, init: &[f32]| {
            backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(init), usage })
        };
        let bytes = w * h * 3 * 4;
        let sigma = [1.3f32, 4.0, 0.7];
        let (src, mid, out) = (mk("src", &data), mk("mid", &data), mk("out", &data));
        let separate = build_ref_blur(&backend.device, &backend, w as u32, h as u32, &src, &mid, &out, sigma, [1.0; 3], false, "sep");
        let expected = run_and_read(&backend, &out, bytes, |e| separate.encode(e));
        let inplace_buf = mk("inplace", &data);
        let inplace = build_ref_blur(&backend.device, &backend, w as u32, h as u32, &inplace_buf, &mid, &inplace_buf, sigma, [1.0; 3], false, "inplace");
        let got = run_and_read(&backend, &inplace_buf, bytes, |e| inplace.encode(e));
        assert_eq!(got, expected);
        let masked_buf = mk("masked", &data);
        let masked = build_ref_blur_channels(&backend.device, &backend, w as u32, h as u32, &masked_buf, &mid, &masked_buf, sigma, [1.0; 3], false, [true, false, true], "masked");
        let got = run_and_read(&backend, &masked_buf, bytes, |e| masked.encode(e));
        for i in 0..w * h {
            assert_eq!(got[i * 3], expected[i * 3]);
            assert_eq!(got[i * 3 + 1], data[i * 3 + 1], "inactive channel changed");
            assert_eq!(got[i * 3 + 2], expected[i * 3 + 2]);
        }
    }

    /// `RefBlur` at the image borders against an f64 reference of the CPU
    /// blur: the FIR (σ < 3) reflected with the edge sample repeated (Python's
    /// `_reflect`), the IIR from the edge samples. Bright pixels sit on every
    /// edge and in the corners; the small fields are narrower than the FIR
    /// kernels, and 300 px spans two row segments of the row pass.
    #[test]
    fn ref_blur_borders_match_f64_reference() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        // Python `_reflect`, written out independently of `reflect_index`.
        let reflect = |i: isize, n: usize| -> usize {
            let n = n as isize;
            let mut i = i;
            while i < 0 || i >= n {
                i = if i < 0 { -i - 1 } else { 2 * n - 1 - i };
            }
            i as usize
        };
        let fir = |line: &mut Vec<f64>, s: f64| {
            let r = (3.0 * s + 0.5) as isize;
            let k: Vec<f64> = (-r..=r).map(|x| (-(x * x) as f64 / (2.0 * s * s)).exp()).collect();
            let sum: f64 = k.iter().sum();
            let src = line.clone();
            for (j, out) in line.iter_mut().enumerate() {
                *out = (-r..=r).map(|d| src[reflect(j as isize + d, src.len())] * k[(d + r) as usize]).sum::<f64>() / sum;
            }
        };
        let iir = |line: &mut Vec<f64>, s: f64| {
            let (b1, b2, b3) = young_van_vliet_b(s);
            let a = 1.0 - b1 - b2 - b3;
            let n = line.len();
            let (mut w1, mut w2, mut w3) = (line[0], line[0], line[0]);
            for j in 0..n {
                let v = a * line[j] + b1 * w1 + b2 * w2 + b3 * w3;
                line[j] = v;
                (w3, w2, w1) = (w2, w1, v);
            }
            let xn = line[n - 1];
            (w1, w2, w3) = (xn, xn, xn);
            for j in (0..n).rev() {
                let v = a * line[j] + b1 * w1 + b2 * w2 + b3 * w3;
                line[j] = v;
                (w3, w2, w1) = (w2, w1, v);
            }
        };
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        // FIR and IIR channels mixed in one blur, as halation and DIR do.
        let sigmas = [[0.75f32, 1.7, 2.99], [0.3, 2.5, 3.0], [4.5, 9.0, 1.0]];
        for (w, h) in [(300usize, 37usize), (23, 17), (5, 3), (1, 9), (9, 1)] {
            let mut src = vec![0.0f64; w * h * 3];
            for y in 0..h {
                for x in 0..w {
                    for c in 0..3 {
                        let v = 0.2 + 0.1 * (0.7 * x as f64 + 0.3 * y as f64 + c as f64).sin();
                        let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                        let corner = (x == 0 || x == w - 1) && (y == 0 || y == h - 1);
                        let bright = if corner { 6.0 } else if edge && (x + y) % 3 == 0 { 3.0 } else { 0.0 };
                        src[(y * w + x) * 3 + c] = v + bright;
                    }
                }
            }
            let src_f32: Vec<f32> = src.iter().map(|&v| v as f32).collect();
            let mk = |label: &str| {
                backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(&src_f32), usage })
            };
            for sigma in sigmas {
                let mut expected = src.clone();
                for c in 0..3 {
                    let s = sigma[c] as f64;
                    let blur = |line: &mut Vec<f64>| if s < 3.0 { fir(line, s) } else { iir(line, s) };
                    for y in 0..h {
                        let mut line: Vec<f64> = (0..w).map(|x| expected[(y * w + x) * 3 + c]).collect();
                        blur(&mut line);
                        for x in 0..w {
                            expected[(y * w + x) * 3 + c] = line[x];
                        }
                    }
                    for x in 0..w {
                        let mut line: Vec<f64> = (0..h).map(|y| expected[(y * w + x) * 3 + c]).collect();
                        blur(&mut line);
                        for y in 0..h {
                            expected[(y * w + x) * 3 + c] = line[y];
                        }
                    }
                }
                let (src_buf, mid, out) = (mk("border_src"), mk("border_mid"), mk("border_out"));
                let blur = build_ref_blur(&backend.device, &backend, w as u32, h as u32, &src_buf, &mid, &out, sigma, [1.0; 3], false, "border_test");
                let got = run_and_read(&backend, &out, w * h * 3 * 4, |encoder| blur.encode(encoder));
                for c in 0..3 {
                    let err = (0..w * h).map(|i| (got[i * 3 + c] as f64 - expected[i * 3 + c]).abs()).fold(0.0, f64::max);
                    eprintln!("{w}×{h} σ {}: max |GPU − f64| = {err:.1e}", sigma[c]);
                    assert!(err < 5e-6, "{w}×{h} σ {}: max error {err}", sigma[c]);
                }
            }
        }
    }

    /// Time of each blur pass at 24 MP with the default halation and DIR
    /// sizes; run with `cargo test --release -p spektrafilm-gpu
    /// bench_ref_blur_passes -- --ignored --nocapture`. Each pass is submitted
    /// ten times and waited for; the median of a few runs is printed. This is
    /// amortized wall time, and a pass repeated alone works on its own output
    /// rather than on what the passes before it would leave, so compare
    /// whole renders (`spektrafilm-native`) before trusting a gain.
    #[test]
    #[ignore]
    fn bench_ref_blur_passes() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let (w, h) = (6048u32, 4024u32);
        let n = (w * h * 3) as usize;
        let data: Vec<f32> = (0..n).map(|i| ((i as u64 * 7919) % 1000) as f32 / 1000.0).collect();
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let mk = |label: &str| {
            backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(&data), usage })
        };
        let (src, mid, out) = (mk("src"), mk("mid"), mk("out"));
        let time = |encode: &dyn Fn(&mut wgpu::CommandEncoder)| {
            let mut ms = Vec::new();
            for i in 0..7 {
                // Ten in one submission: waiting is only accurate to ~1.3 ms.
                let mut encoder = backend.device.create_command_encoder(&Default::default());
                for _ in 0..10 {
                    encode(&mut encoder);
                }
                let t = std::time::Instant::now();
                backend.queue.submit(Some(encoder.finish()));
                backend.device.poll(wgpu::Maintain::Wait);
                if i >= 2 {
                    ms.push(t.elapsed().as_secs_f64() * 1e2);
                }
            }
            ms.sort_by(f64::total_cmp);
            ms[ms.len() / 2]
        };
        // BLUR_BENCH_PASS=name:i runs only pass i of that blur, for about a
        // second (to sample GPU counters with Instruments).
        let only = std::env::var("BLUR_BENCH_PASS").ok();
        // The bandwidth floor: one read and one write of the image.
        const COPY: &str = "@group(0) @binding(0) var<storage, read> a: array<vec4<f32>>;
            @group(0) @binding(1) var<storage, read_write> b: array<vec4<f32>>;
            @compute @workgroup_size(256) fn main(@builtin(global_invocation_id) g: vec3<u32>, @builtin(num_workgroups) n: vec3<u32>) {
                let i = g.y * n.x * 256u + g.x;
                if i < arrayLength(&a) { b[i] = a[i]; }
            }";
        let copy = backend.cached_pipeline(COPY, &[wgpu::BufferBindingType::Storage { read_only: true }, wgpu::BufferBindingType::Storage { read_only: false }]);
        let copy_bg = backend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &copy.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: out.as_entire_binding() },
            ],
        });
        let ms = if only.is_some() { 0.0 } else { time(&|e| {
            let mut pass = e.begin_compute_pass(&Default::default());
            pass.set_pipeline(&copy.pipeline);
            pass.set_bind_group(0, &copy_bg, &[]);
            dispatch_linear(&mut pass, (n / 4) as u32, 256);
        }) };
        eprintln!("copy {:.0} MB: {ms:.1} ms", (n * 4) as f64 / 1e6);
        let configs: [(&str, [f32; 3], bool); 7] = [
            ("scatter core", [0.37, 0.34, 0.27], false),
            ("scatter tail 0", [0.84, 0.87, 0.82], false),
            ("scatter tail 1", [2.4, 2.5, 2.3], true),
            ("scatter tail 2", [4.3, 4.5, 4.2], true),
            ("bounce 0", [10.9; 3], false),
            ("bounce 2", [18.9; 3], true),
            ("dir tail 2", [93.0; 3], true),
        ];
        for (name, sigma, accumulate) in configs {
            let blur = build_ref_blur(&backend.device, &backend, w, h, &src, &mid, &out, sigma, [0.5; 3], accumulate, name);
            if let Some((only_name, i)) = only.as_deref().and_then(|o| o.split_once(':')) {
                if only_name != name {
                    continue;
                }
                let p = &blur.passes[i.parse::<usize>().unwrap()];
                for _ in 0..10 {
                    let mut e = backend.device.create_command_encoder(&Default::default());
                    for _ in 0..10 {
                        let mut pass = e.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&p.pipeline.pipeline);
                        pass.set_bind_group(0, &p.bg, &[]);
                        pass.dispatch_workgroups(p.workgroups.0, p.workgroups.1, 1);
                    }
                    backend.queue.submit(Some(e.finish()));
                    backend.device.poll(wgpu::Maintain::Wait);
                }
                continue;
            }
            let total = time(&|e| blur.encode(e));
            let passes: Vec<String> = (0..blur.passes.len())
                .map(|i| {
                    let p = &blur.passes[i];
                    let ms = time(&|e| {
                        let mut pass = e.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&p.pipeline.pipeline);
                        pass.set_bind_group(0, &p.bg, &[]);
                        pass.dispatch_workgroups(p.workgroups.0, p.workgroups.1, 1);
                    });
                    format!("{ms:.1}")
                })
                .collect();
            eprintln!("{name:<16} σ {:>5.2}: {total:6.1} ms, passes {} (wg {:?})", sigma[0], passes.join(" "), blur.passes.iter().map(|p| p.pipeline.workgroup_size).collect::<Vec<_>>());
        }
    }



    #[test]
    fn fft_sizes_are_smooth() {
        assert_eq!(fft_size(1), 1);
        assert_eq!(fft_size(7), 8);
        assert_eq!(fft_size(6624), 6750);
        for n in [97u32, 1000, 6061, 13540] {
            let m = fft_size(n);
            assert!(m >= n && fft_radices(m).iter().product::<u32>() == m);
        }
    }

    /// The GPU diffusion filter against the CPU's reflect-padded FFT
    /// convolution in f64 (`convolve2d_reflect`) and its mix, on odd sizes
    /// that need radix 2, 3, 4 and 5 stages, for each channel's own PSF.
    #[test]
    fn diffusion_fft_matches_cpu_reflect_convolution() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        for (w, h, r) in [(97usize, 61usize, 12usize), (230, 150, 70)] {
            let p_s = 0.37f32;
            let comps: [Vec<(f64, f64)>; 3] = std::array::from_fn(|c| {
                vec![(1.7, 0.05), (9.0 + c as f64, 0.002), (40.0, 0.0001 * (c + 1) as f64)]
            });
            let plan = crate::DiffusionGpuPlan {
                d: 1,
                small_w: w as u32,
                small_h: h as u32,
                p_s,
                sigmas: vec![],
                coeffs: vec![],
                psf: crate::DiffusionPsf { radius: r as u32, components: comps.clone() },
            };
            let data: Vec<f32> = (0..w * h * 3).map(|i| 0.05 + ((i * 7919 + i / 3 * 31) % 997) as f32 / 500.0).collect();
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
            let img = backend.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("img"), contents: bytemuck::cast_slice(&data), usage });
            let scratch = vec![backend.device.create_buffer(&wgpu::BufferDescriptor { label: Some("s"), size: (w * h * 12) as u64, usage, mapped_at_creation: false })];
            let state = build_diffusion_state(&backend.device, &plan, w as u32, h as u32, &img, &scratch, &backend);
            let got = run_and_read(&backend, &img, w * h * 12, |e| state.encode_passes(e, &img));
            let k = 2 * r + 1;
            let mut worst = 0.0f64;
            for c in 0..3 {
                let mut psf: Vec<f64> = (0..k * k)
                    .map(|p| {
                        let (dx, dy) = ((p % k) as f64 - r as f64, (p / k) as f64 - r as f64);
                        let rr = (dx * dx + dy * dy).sqrt();
                        comps[c].iter().map(|&(l, a)| a * (-rr / l).exp()).sum::<f64>()
                    })
                    .collect();
                let sum: f64 = psf.iter().sum();
                psf.iter_mut().for_each(|v| *v /= sum);
                let chan: Vec<f64> = (0..w * h).map(|i| data[i * 3 + c] as f64).collect();
                let blurred = spektrafilm_math::fft_conv::convolve2d_reflect(&chan, h, w, &psf, k);
                for i in 0..w * h {
                    let expected = (1.0 - p_s as f64) * chan[i] + p_s as f64 * blurred[i];
                    worst = worst.max((got[i * 3 + c] as f64 - expected).abs());
                }
            }
            eprintln!("{w}x{h}, r {r}: max |GPU − f64| = {worst:.2e}");
            assert!(worst < 2e-6, "{w}x{h}: {worst}");
        }
    }

    /// A 100 MP frame needs 1.2 GB per 3-channel f32 image buffer, bound
    /// whole. Fails if the device can't bind that, rather than letting a
    /// large render fail later.
    #[test]
    fn device_can_bind_a_100_mp_image() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let limits = backend.device.limits();
        let bytes = 12_288u64 * 8_192 * 3 * 4;
        eprintln!(
            "max_buffer_size {} B, max_storage_buffer_binding_size {} B, 100 MP image {} B",
            limits.max_buffer_size, limits.max_storage_buffer_binding_size, bytes
        );
        assert!(limits.max_buffer_size >= bytes);
        assert!(limits.max_storage_buffer_binding_size as u64 >= bytes);
    }

    fn run_and_read(
        backend: &WgpuBackend,
        buf: &wgpu::Buffer,
        bytes: usize,
        encode: impl FnOnce(&mut wgpu::CommandEncoder),
    ) -> Vec<f32> {
        let readback = backend.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_readback"),
            size: bytes as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = backend.device.create_command_encoder(&Default::default());
        encode(&mut encoder);
        encoder.copy_buffer_to_buffer(buf, 0, &readback, 0, bytes as u64);
        backend.queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        backend.device.poll(wgpu::Maintain::Wait);
        let out: Vec<f32> = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();
        out
    }

    /// Poisson(λ) thinned by Binomial(·, p) is Poisson(λp), so on a flat
    /// density the GPU grain has a known mean and variance. The three
    /// channels exercise every sampler branch: Poisson multiplication (λ < 10)
    /// and PTRS, binomial inversion, BTPE, and the p > 0.5 mirror. Catches
    /// samplers that are wrong or a dispatch the driver silently drops.
    #[test]
    fn grain_matches_poisson_binomial_moments() {
        let Some(backend) = WgpuBackend::new() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let (width, height) = (1024u32, 1024u32);
        let n = (width * height) as usize;
        let density = [0.3f32, 1.8, 1.0];
        let density_min = [0.03f32; 3];
        let density_max = [2.5f32 + 0.03; 3];
        let npp = [4.0f32, 40.0, 400.0];
        let uniformity = [0.97f32, 0.99, 0.97];
        // One sub-layer whose curve is the total, so it sees the density as is.
        let gp = crate::GrainGpuParams {
            interp_x: vec![[0.0; 3], [3.0; 3]],
            positive: false,
            interp_inv_dx: vec![[1.0 / 3.0; 3], [0.0; 3]],
            curves_layers: vec![[0.0; 3], [3.0; 3]],
            density_min_layers: vec![density_min],
            density_max_layers: vec![density_max],
            n_particles_per_pixel: vec![npp],
            dye_cloud_sigma_px: vec![[0.0; 3]],
            grain_uniformity: uniformity,
            density_min,
            micro_structure: None,
            blur_px: 0.0,
            mult_usm: None,
            base_seed: 0,
            monochrome: false,
        };
        let data: Vec<f32> = (0..n).flat_map(|_| density).collect();
        let out = backend.run_grain(&ImageBuf::from_data(width, height, f32_to_scalars(data)), &gp);
        let out: Vec<f32> = out.data.iter().map(|&v| v as f32).collect();

        for c in 0..3 {
            let d_in = (density[c] + density_min[c]) as f64;
            let dmax = density_max[c] as f64;
            let p = (d_in / dmax).clamp(1e-6, 1.0 - 1e-6);
            let sat = 1.0 - p * uniformity[c] as f64 * (1.0 - 1e-6);
            let lambda = npp[c] as f64 / sat;
            let od = dmax / npp[c] as f64;
            let expected_mean = lambda * p * od * sat - density_min[c] as f64;
            let expected_var = lambda * p * (od * sat).powi(2);
            let values: Vec<f64> = out.iter().skip(c).step_by(3).map(|&v| v as f64).collect();
            let mean = values.iter().sum::<f64>() / n as f64;
            let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
            let mean_tol = 5.0 * (expected_var / n as f64).sqrt() + 1e-5;
            assert!(
                (mean - expected_mean).abs() < mean_tol,
                "channel {c}: mean {mean} vs expected {expected_mean}"
            );
            assert!(
                (var / expected_var - 1.0).abs() < 0.02,
                "channel {c}: variance {var} vs expected {expected_var}"
            );
        }
    }
}
