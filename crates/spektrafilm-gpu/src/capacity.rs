//! Admission checks for Windows/Linux Vulkan, before full-image GPU allocation.
//! This is a conservative memory estimate, not device-loss recovery or a
//! guarantee against long-running shaders. Metal keeps its existing path.

const MIB: u64 = 1024 * 1024;
const STATIC_RESERVE: u64 = 128 * MIB;

#[derive(Clone, Copy)]
pub(crate) struct CapacityLimits {
    pub(crate) max_binding: u64,
    pub(crate) max_buffer: u64,
    pub(crate) memory_budget: Option<u64>,
}

impl CapacityLimits {
    pub(crate) fn image(&self, width: u32, height: u32) -> Result<u64, String> {
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(12))
            .ok_or("image size overflow")?;
        self.buffer(bytes, "RGB image")?;
        // Input/output ping-pong plus the two shared effect buffers. Staging
        // and readback are host-visible; reserve space for LUTs, parameters
        // and the small segmented-blur state allocations separately.
        let estimate = bytes
            .checked_mul(4)
            .and_then(|n| n.checked_add(STATIC_RESERVE))
            .ok_or("image working-set overflow")?;
        self.working_set(estimate)?;
        Ok(estimate)
    }

    pub(crate) fn buffer(&self, bytes: u64, label: &str) -> Result<(), String> {
        if bytes > self.max_binding.min(self.max_buffer) {
            return Err(format!(
                "{label} needs {bytes} bytes; GPU buffer/binding limit is {}",
                self.max_binding.min(self.max_buffer)
            ));
        }
        Ok(())
    }

    pub(crate) fn working_set(&self, bytes: u64) -> Result<(), String> {
        if let Some(budget) = self.memory_budget {
            if bytes > budget {
                return Err(format!(
                    "estimated GPU working set {} MiB exceeds admission budget {} MiB",
                    bytes.div_ceil(MIB),
                    budget / MIB
                ));
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) fn vulkan_limits(adapter: &wgpu::Adapter) -> CapacityLimits {
    let limits = adapter.limits();
    // A static heap-size estimate deliberately leaves 20% for the desktop,
    // other applications and driver allocations. It is not a live VRAM budget.
    // Shared-memory adapters have no separate VRAM pool to reserve this way.
    let memory_budget = if adapter.get_info().device_type == wgpu::DeviceType::DiscreteGpu {
        // SAFETY: read-only Vulkan query under wgpu's adapter lock. No raw
        // handle is retained or destroyed, and no wgpu calls occur inside it.
        unsafe {
            adapter.as_hal::<wgpu::hal::api::Vulkan, _, _>(|raw| {
                let raw = raw?;
                let properties = raw
                    .shared_instance()
                    .raw_instance()
                    .get_physical_device_memory_properties(raw.raw_physical_device());
                // VK_MEMORY_HEAP_DEVICE_LOCAL_BIT = 0x1. Using as_raw avoids
                // depending directly on ash just for this one constant.
                properties.memory_heaps[..properties.memory_heap_count as usize]
                    .iter()
                    .filter(|heap| heap.flags.as_raw() & 1 != 0)
                    .map(|heap| heap.size)
                    .max()
                    .map(|bytes| bytes / 5 * 4)
            })
        }
    } else {
        None
    };
    // A diagnostic can only LOWER the normal admission budget, never bypass
    // it. This lets small fixtures exercise CPU fallback without allocating
    // multi-gigabyte images or intentionally resetting the display driver.
    let override_budget = std::env::var("SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_mul(MIB));
    let memory_budget = match (memory_budget, override_budget) {
        (Some(normal), Some(cap)) => Some(normal.min(cap)),
        (normal, None) => normal,
        (None, cap) => cap,
    };
    CapacityLimits {
        max_binding: u64::from(limits.max_storage_buffer_binding_size),
        max_buffer: limits.max_buffer_size,
        memory_budget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn six_gib() -> CapacityLimits {
        CapacityLimits {
            max_binding: 2_147_483_647,
            max_buffer: 4 * 1024 * MIB,
            memory_budget: Some(6 * 1024 * MIB / 5 * 4),
        }
    }

    #[test]
    fn admits_measured_safe_sizes_and_rejects_reset_sizes() {
        let limits = six_gib();
        assert!(limits.image(6048, 4024).is_ok());
        assert!(limits.image(12000, 8000).is_ok());
        assert!(
            limits
                .image(14400, 10000)
                .unwrap_err()
                .contains("working set")
        );
        assert!(
            limits
                .image(15000, 11930)
                .unwrap_err()
                .contains("working set")
        );
        assert!(
            limits
                .image(15000, 12000)
                .unwrap_err()
                .contains("binding limit")
        );
    }

    #[test]
    fn binding_and_allocation_limits_apply_without_a_vram_estimate() {
        let limits = CapacityLimits {
            memory_budget: None,
            max_buffer: 1024,
            ..six_gib()
        };
        assert!(limits.image(8, 8).is_ok());
        assert!(limits.image(16, 16).is_err());
        assert!(limits.image(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn extra_effect_buffers_must_fit_as_well() {
        let limits = six_gib();
        let base = limits.image(12000, 8000).unwrap();
        assert!(limits.working_set(base + 1024 * MIB).is_err());
        assert!(limits.buffer(2_147_483_648, "FFT").is_err());
    }
}
