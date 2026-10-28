// Only the missing scalar math primitive is shared. No grain, sampler,
// filtering, dispatch or array work crosses a language boundary.
#[unsafe(no_mangle)]
pub extern "C" fn grain_lgamma(x: f64) -> f64 {
    libm::lgamma(x)
}
#[unsafe(no_mangle)]
pub extern "C" fn grain_rss() -> u64 {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            static mach_task_self_: u32;
            fn task_info(task: u32, flavor: u32, info: *mut u32, count: *mut u32) -> i32;
        }
        let mut info = [0u64; 6];
        let mut count = 12;
        if task_info(mach_task_self_, 20, info.as_mut_ptr().cast(), &mut count) == 0 {
            return info[1];
        }
    }
    0
}
