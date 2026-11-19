//! Seconds on the monotonic clock shared by the audio capture and the paint loop.

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;

    use mach2::mach_time::{mach_absolute_time, mach_timebase_info};

    fn seconds_per_tick() -> f64 {
        static SCALE: OnceLock<f64> = OnceLock::new();
        *SCALE.get_or_init(|| {
            let mut info = mach_timebase_info { numer: 0, denom: 0 };
            // SAFETY: `info` is a valid out-pointer for the call.
            unsafe { mach_timebase_info(&mut info) };
            if info.denom == 0 {
                1e-9
            } else {
                f64::from(info.numer) / f64::from(info.denom) * 1e-9
            }
        })
    }

    /// The time base is the host clock Core Audio stamps its IO cycles with.
    pub fn now() -> f64 {
        // SAFETY: no arguments, no preconditions.
        from_host_ticks(unsafe { mach_absolute_time() })
    }

    /// A Core Audio host time (`AudioTimeStamp::mHostTime`) on this clock.
    pub fn from_host_ticks(ticks: u64) -> f64 {
        ticks as f64 * seconds_per_tick()
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::{sync::OnceLock, time::Instant};

    /// Seconds since the first call.
    pub fn now() -> f64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64()
    }
}

pub use imp::*;
