use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Peak meter written by the audio thread, read (and reset) by the UI.
#[derive(Default)]
pub struct StereoMeter {
    left: AtomicU32,
    right: AtomicU32,
}

impl StereoMeter {
    /// Audio thread: raise the held peak. Positive f32 bit patterns order like u32s.
    #[inline]
    pub fn feed(&self, left: f32, right: f32) {
        self.left.fetch_max(left.abs().to_bits(), Ordering::Relaxed);
        self.right.fetch_max(right.abs().to_bits(), Ordering::Relaxed);
    }

    /// UI thread: take the peaks since the last call.
    pub fn take(&self) -> (f32, f32) {
        (f32::from_bits(self.left.swap(0, Ordering::Relaxed)), f32::from_bits(self.right.swap(0, Ordering::Relaxed)))
    }
}

/// Telemetry shared between the audio thread and the UI.
pub struct Shared {
    pub sample_rate: f64,
    pub playing: AtomicBool,
    position_tick: AtomicU64,
    cpu_load: AtomicU32,
    peak_cpu_load: AtomicU32,
    pub callbacks: AtomicU64,
    /// Callbacks that took longer than the buffer duration.
    pub overloads: AtomicU64,
    /// Garbage that could not be returned (queue full) and was leaked instead of freed on the audio thread.
    pub leaked_garbage: AtomicU64,
    /// Commands rejected because the queue was full (counted on the control side).
    pub dropped_commands: AtomicU64,
    pub track_meters: Vec<StereoMeter>,
    pub channel_meters: Vec<StereoMeter>,
}

impl Shared {
    pub fn new(sample_rate: f64, tracks: usize, channels: usize) -> Self {
        Self {
            sample_rate,
            playing: AtomicBool::new(false),
            position_tick: AtomicU64::new(0f64.to_bits()),
            cpu_load: AtomicU32::new(0),
            peak_cpu_load: AtomicU32::new(0),
            callbacks: AtomicU64::new(0),
            overloads: AtomicU64::new(0),
            leaked_garbage: AtomicU64::new(0),
            dropped_commands: AtomicU64::new(0),
            track_meters: (0..tracks).map(|_| StereoMeter::default()).collect(),
            channel_meters: (0..channels).map(|_| StereoMeter::default()).collect(),
        }
    }

    pub fn position_tick(&self) -> f64 {
        f64::from_bits(self.position_tick.load(Ordering::Relaxed))
    }

    pub(crate) fn set_position_tick(&self, tick: f64) {
        self.position_tick.store(tick.to_bits(), Ordering::Relaxed);
    }

    /// Fraction of the buffer period spent rendering in the last callback.
    pub fn cpu_load(&self) -> f32 {
        f32::from_bits(self.cpu_load.load(Ordering::Relaxed))
    }

    /// Highest load since the last call.
    pub fn take_peak_cpu_load(&self) -> f32 {
        f32::from_bits(self.peak_cpu_load.swap(0, Ordering::Relaxed))
    }

    pub(crate) fn report_load(&self, load: f32) {
        self.cpu_load.store(load.to_bits(), Ordering::Relaxed);
        self.peak_cpu_load.fetch_max(load.max(0.0).to_bits(), Ordering::Relaxed);
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        if load > 1.0 {
            self.overloads.fetch_add(1, Ordering::Relaxed);
        }
    }
}
