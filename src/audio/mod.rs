//! Audio sources: the system's output captured as it plays, or a synthetic test signal.
//! Blocks reach the handler on the capture's thread, stamped on the app's clock.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod pulse;
mod test_tone;

/// Called on the capture thread with left, right, sample rate and the time of the
/// block's first sample (seconds on the app's clock).
pub type PcmHandler = Box<dyn FnMut(&[f32], &[f32], f64, f64) + Send>;

/// Dropping it stops the capture; the handler is not called after the drop returns.
pub struct Capture {
    _inner: Box<dyn std::any::Any>,
}

/// The device the system is playing through.
pub struct OutputInfo {
    /// Seconds from a sample reaching the device to it being heard, as the device reports.
    pub latency: f64,
}

/// Hint for a system capture that delivers only zeros. macOS feeds the tap silence
/// until the terminal is allowed to record system audio.
pub const SILENCE_NOTICE: Option<&str> = if cfg!(target_os = "macos") {
    Some(
        "only silence so far: if something is playing, allow this terminal under \
         System Settings > Privacy & Security > Screen & System Audio Recording",
    )
} else {
    None
};

pub fn start_system(handler: PcmHandler) -> Result<Capture, String> {
    #[cfg(target_os = "macos")]
    {
        let tap = macos::SystemTap::start(handler)?;
        Ok(Capture { _inner: Box::new(tap) })
    }
    #[cfg(target_os = "linux")]
    {
        let monitor = pulse::Monitor::start(handler)?;
        Ok(Capture { _inner: Box::new(monitor) })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        drop(handler);
        Err("system audio capture is not built for this platform yet".into())
    }
}

/// The test signal goes to the handler only; it makes no sound.
pub fn start_test_tone(handler: PcmHandler) -> Capture {
    Capture { _inner: Box::new(test_tone::TestTone::start(handler)) }
}

pub fn output_info() -> Option<OutputInfo> {
    #[cfg(target_os = "macos")]
    {
        macos::output_info()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
