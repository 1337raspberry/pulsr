//! The live spectrum between the capture and the paint loop: a ring of recent frames
//! keyed by position in the captured audio, the clock that says which position is being
//! heard, and the analyser that fills both from PCM.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use crate::{bands, filter_bank::FilterBank, level_mapper::LevelMapper, ridge::FrameSource};

/// An audible-clock tick older than this means the capture has stopped delivering.
pub const CLOCK_STALE_SEC: f64 = 0.1;

/// How far the newest frame may trail the moment asked for and still be drawn.
pub const LAG_TOLERANCE_SEC: f64 = 0.1;

/// Seconds of frames the ring holds: the longest output delay it can serve.
pub const RING_SECONDS: f64 = 2.0;

#[derive(Clone, Copy)]
struct AudibleClock {
    position: f64,
    at: f64,
}

struct RingInner {
    width: usize,
    capacity: usize,
    /// Position of each slot's frame, in seconds of captured audio.
    pos: Vec<f64>,
    /// `capacity` frames of `width` levels each.
    levels: Vec<f32>,
    /// Slot the next push writes.
    head: usize,
    count: usize,
    clock: Option<AudibleClock>,
}

impl RingInner {
    /// Ring slot of the `i`th oldest frame held.
    fn slot(&self, i: usize) -> usize {
        (self.head + self.capacity - self.count + i) % self.capacity
    }

    fn frame(&self, i: usize) -> &[f32] {
        let slot = self.slot(i);
        &self.levels[slot * self.width..(slot + 1) * self.width]
    }

    /// The position being heard at `now`, extrapolated from the last tick.
    fn audible(&self, now: f64) -> Option<f64> {
        let clock = self.clock?;
        if now - clock.at > CLOCK_STALE_SEC {
            return None;
        }
        Some(clock.position + (now - clock.at))
    }
}

/// Written from the capture thread and read from the paint loop.
pub struct SpectrumRing {
    inner: Mutex<RingInner>,
    /// Longest gap in seconds between two frames that still counts as continuous audio.
    max_gap: f64,
}

impl SpectrumRing {
    pub fn new(width: usize, fps: u32) -> Self {
        let fps = f64::from(fps.max(1));
        let capacity = ((RING_SECONDS * fps).ceil() as usize).max(2);
        Self {
            inner: Mutex::new(RingInner {
                width,
                capacity,
                pos: vec![0.0; capacity],
                levels: vec![0.0; capacity * width],
                head: 0,
                count: 0,
                clock: None,
            }),
            max_gap: 2.5 / fps,
        }
    }

    pub fn width(&self) -> usize {
        self.lock().width
    }

    /// Frames arrive in order of position; a frame of the wrong width is dropped.
    pub fn push(&self, pos: f64, levels: &[f32]) {
        let mut ring = self.lock();
        if levels.len() != ring.width {
            return;
        }
        let (head, width) = (ring.head, ring.width);
        ring.pos[head] = pos;
        ring.levels[head * width..(head + 1) * width].copy_from_slice(levels);
        ring.head = (head + 1) % ring.capacity;
        ring.count = (ring.count + 1).min(ring.capacity);
    }

    /// `position` is being heard at time `at`.
    pub fn set_audible_clock(&self, position: f64, at: f64) {
        self.lock().clock = Some(AudibleClock { position, at });
    }

    /// Reads the moment `lead` s after `now`, band `k` a further `ahead[k]` s on, interpolating
    /// between frames. False, with `out` untouched, without a live clock or a recent frame.
    pub fn read(&self, now: f64, lead: f64, ahead: &[f64], out: &mut [f32]) -> bool {
        let ring = self.lock();
        let Some(audible) = ring.audible(now) else {
            return false;
        };
        let bands = ahead.len();
        if ring.count == 0 || bands == 0 || !ring.width.is_multiple_of(bands) || out.len() != ring.width {
            return false;
        }
        let target = audible + lead;
        let newest = ring.pos[ring.slot(ring.count - 1)];
        if newest < target - LAG_TOLERANCE_SEC {
            return false;
        }
        for (k, &extra) in ahead.iter().enumerate() {
            let want = target + extra;
            // First frame after `want`; the one before it is at or before.
            let (mut lo, mut hi) = (0, ring.count);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if ring.pos[ring.slot(mid)] <= want {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            let (a, b, t) = if lo == 0 {
                (0, 0, 0.0)
            } else if lo == ring.count {
                (lo - 1, lo - 1, 0.0)
            } else {
                let (pa, pb) = (ring.pos[ring.slot(lo - 1)], ring.pos[ring.slot(lo)]);
                let gap = pb - pa;
                if gap <= 0.0 || gap > self.max_gap {
                    // A break in the audio: nothing to interpolate across.
                    (lo - 1, lo - 1, 0.0)
                } else {
                    (lo - 1, lo, ((want - pa) / gap) as f32)
                }
            };
            let (fa, fb) = (ring.frame(a), ring.frame(b));
            let mut c = k;
            while c < ring.width {
                out[c] = fa[c] + (fb[c] - fa[c]) * t;
                c += bands;
            }
        }
        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RingInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Reads every band its onset delay further on, so a kick's body lands with its click.
pub struct SpectrumReader {
    ring: Arc<SpectrumRing>,
    ahead: Vec<f64>,
}

impl SpectrumReader {
    pub fn new(ring: Arc<SpectrumRing>, ahead: Vec<f64>) -> Self {
        Self { ring, ahead }
    }
}

impl FrameSource for SpectrumReader {
    fn width(&self) -> usize {
        self.ring.width()
    }

    fn read(&self, now: f64, lead_sec: f64, out: &mut [f32]) -> bool {
        self.ring.read(now, lead_sec, &self.ahead, out)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureStats {
    /// Sample rate of the capture; 0 before the first block.
    pub sample_rate: f64,
    /// Largest sample magnitude so far.
    pub peak: f32,
    /// Whether any non-zero sample has arrived.
    pub heard_signal: bool,
    pub blocks: u64,
}

/// State the capture thread and the main thread share.
pub struct LiveSpectrum {
    pub ring: Arc<SpectrumRing>,
    /// Seconds between a sample being captured and being heard, as `f64` bits.
    output_delay: AtomicU64,
    stats: Mutex<CaptureStats>,
}

impl LiveSpectrum {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ring: Arc::new(SpectrumRing::new(bands::BANDS * bands::CHANNELS, bands::FRAME_RATE)),
            output_delay: AtomicU64::new(0f64.to_bits()),
            stats: Mutex::new(CaptureStats::default()),
        })
    }

    pub fn reader(&self) -> SpectrumReader {
        SpectrumReader::new(self.ring.clone(), bands::onset_delays(bands::BANDS))
    }

    pub fn output_delay(&self) -> f64 {
        f64::from_bits(self.output_delay.load(Ordering::Relaxed))
    }

    pub fn set_output_delay(&self, seconds: f64) {
        self.output_delay.store(seconds.to_bits(), Ordering::Relaxed);
    }

    pub fn stats(&self) -> CaptureStats {
        *self.stats.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Turns captured PCM into frames in a `LiveSpectrum`. Owned by the capture thread.
pub struct Analyser {
    live: Arc<LiveSpectrum>,
    bank: Option<FilterBank>,
    mapper: LevelMapper,
    levels: Vec<f32>,
    /// Where the next block is expected to start, on the app's clock.
    expected_start: Option<f64>,
    /// Added to the bank's position; grows by every gap, so older audio is not read as recent.
    timeline_shift: f64,
}

impl Analyser {
    /// A block whose timestamp is this far from where it was expected marks a break.
    pub const DISCONTINUITY_SEC: f64 = 0.05;

    pub fn new(live: Arc<LiveSpectrum>) -> Self {
        Self {
            live,
            bank: None,
            mapper: LevelMapper::new(bands::FRAME_RATE),
            levels: vec![0.0; bands::BANDS * bands::CHANNELS],
            expected_start: None,
            timeline_shift: 0.0,
        }
    }

    /// `start` is the time of the block's first sample on the app's clock.
    pub fn process(&mut self, left: &[f32], right: &[f32], sample_rate: f64, start: f64) {
        let count = left.len().min(right.len());
        if count == 0 || !(sample_rate > 0.0) {
            return;
        }
        if self.bank.as_ref().is_none_or(|bank| bank.sample_rate() != sample_rate) {
            // A new capture, or its device changed rate: start afresh.
            let carried = self.bank.as_ref().map_or(0.0, |bank| bank.position());
            self.timeline_shift += carried;
            self.bank = Some(FilterBank::new(bands::BANDS, bands::FRAME_RATE, sample_rate));
            self.expected_start = None;
        }
        let bank = self.bank.as_mut().expect("the bank was just made");
        let duration = count as f64 / sample_rate;
        if let Some(expected) = self.expected_start
            && (start - expected).abs() > Self::DISCONTINUITY_SEC
        {
            self.timeline_shift += start - expected;
            bank.reset();
        }
        self.expected_start = Some(start + duration);

        let peak = left[..count]
            .iter()
            .chain(&right[..count])
            .fold(0f32, |peak, sample| peak.max(sample.abs()));

        let (ring, mapper, levels, shift) =
            (&self.live.ring, &mut self.mapper, &mut self.levels, self.timeline_shift);
        bank.process(&left[..count], &right[..count], |pos, db| {
            mapper.map(db, levels);
            ring.push(pos + shift, levels);
        });
        let end = start + duration;
        ring.set_audible_clock(bank.position() + shift - self.live.output_delay(), end);

        let mut stats = self.live.stats.lock().unwrap_or_else(|p| p.into_inner());
        stats.sample_rate = sample_rate;
        stats.peak = stats.peak.max(peak);
        stats.heard_signal |= peak > 0.0;
        stats.blocks += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-band, one-channel ring at 100 frames a second whose frame at
    /// position `p` holds `[p, 10 p]`.
    fn ramp_ring(frames: usize) -> SpectrumRing {
        let ring = SpectrumRing::new(2, 100);
        for i in 1..=frames {
            let p = i as f64 / 100.0;
            ring.push(p, &[p as f32, 10.0 * p as f32]);
        }
        ring
    }

    #[test]
    fn a_read_interpolates_between_frames() {
        let ring = ramp_ring(50);
        ring.set_audible_clock(0.2, 100.0);
        let mut out = [0.0; 2];
        assert!(ring.read(100.0, 0.0, &[0.0, 0.0], &mut out));
        assert!((out[0] - 0.2).abs() < 1e-6);
        // 3.7 ms on: between the frames at 0.20 and 0.21.
        assert!(ring.read(100.0037, 0.0, &[0.0, 0.0], &mut out));
        assert!((out[0] - 0.2037).abs() < 1e-5, "{}", out[0]);
        assert!((out[1] - 2.037).abs() < 1e-4);
    }

    #[test]
    fn each_band_reads_its_own_moment() {
        let ring = ramp_ring(50);
        ring.set_audible_clock(0.2, 100.0);
        let mut out = [0.0; 2];
        assert!(ring.read(100.0, 0.05, &[0.1, 0.0], &mut out));
        assert!((out[0] - 0.35).abs() < 1e-5);
        assert!((out[1] - 2.5).abs() < 1e-4);
    }

    #[test]
    fn a_moment_past_the_newest_frame_reads_the_newest_frame() {
        let ring = ramp_ring(50);
        ring.set_audible_clock(0.48, 100.0);
        let mut out = [0.0; 2];
        assert!(ring.read(100.0, 0.05, &[0.04, 0.0], &mut out));
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] - 5.0).abs() < 1e-5);
    }

    #[test]
    fn nothing_is_read_without_a_live_clock_or_a_recent_frame() {
        let ring = ramp_ring(50);
        let mut out = [0.0; 2];
        assert!(!ring.read(100.0, 0.0, &[0.0, 0.0], &mut out));
        ring.set_audible_clock(0.4, 100.0);
        assert!(ring.read(100.05, 0.0, &[0.0, 0.0], &mut out));
        // The clock has gone stale.
        assert!(!ring.read(100.0 + CLOCK_STALE_SEC + 0.01, 0.0, &[0.0, 0.0], &mut out));
        // The clock is live but the frames end long before it.
        ring.set_audible_clock(0.5 + LAG_TOLERANCE_SEC + 0.02, 200.0);
        assert!(!ring.read(200.0, 0.0, &[0.0, 0.0], &mut out));
    }

    #[test]
    fn the_ring_keeps_the_newest_frames() {
        let ring = ramp_ring(1000);
        ring.set_audible_clock(9.0, 50.0);
        let mut out = [0.0; 2];
        assert!(ring.read(50.0, 0.0, &[0.0, 0.0], &mut out));
        assert!((out[0] - 9.0).abs() < 1e-5);
        // The newest frame is the last one pushed.
        assert!(ring.read(50.0, 1.0, &[0.0, 0.0], &mut out));
        assert!((out[0] - 10.0).abs() < 1e-5);
        // Two seconds are held: a moment before them reads the oldest frame.
        ring.set_audible_clock(5.0, 50.0);
        assert!(ring.read(50.0, 0.0, &[0.0, 0.0], &mut out));
        assert!((out[0] - 8.01).abs() < 1e-5, "{}", out[0]);
    }

    #[test]
    fn the_analyser_fills_the_ring_and_sets_the_clock() {
        let live = LiveSpectrum::new();
        live.set_output_delay(0.02);
        let mut analyser = Analyser::new(live.clone());
        let rate = 48_000.0;
        let block: Vec<f32> = (0..256)
            .map(|i| (0.2 * (std::f64::consts::TAU * 1000.0 * i as f64 / rate).sin()) as f32)
            .collect();
        let mut start = 10.0;
        for _ in 0..200 {
            analyser.process(&block, &block, rate, start);
            start += 256.0 / rate;
        }
        let (frames, lead) = {
            let ring = live.ring.lock();
            (ring.count, ring.pos[ring.slot(ring.count - 1)] - ring.audible(start).unwrap())
        };
        assert_eq!(frames, 200 * 256 * 240 / 48_000);
        // The newest frame is at most a frame older than the capture's end,
        // which is the output delay ahead of what is heard.
        assert!(lead <= 0.02 + 1e-9 && lead > 0.02 - 1.0 / 240.0, "{lead}");
        let mut out = vec![0.0; live.ring.width()];
        assert!(live.reader().read(start, 0.0, &mut out));
        let loudest = out[..bands::BANDS].iter().copied().fold(0.0, f32::max);
        assert!(loudest > 0.8, "{loudest}");
        assert!(live.stats().heard_signal);
    }
}
