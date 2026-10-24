//! Soundesti's real-time audio engine.
//!
//! Threading model:
//! - The UI/control thread owns an [`EngineHandle`]. It builds everything that
//!   needs allocation (instruments, [`PlaybackSnapshot`]s) and sends it through a
//!   lock-free SPSC queue.
//! - The audio thread owns the [`Engine`] and calls [`Engine::process`]. That path
//!   never allocates, locks, or does I/O. Replaced objects are sent back through a
//!   second queue so they are freed on the control thread.
//! - Telemetry (play position, meters, CPU load) flows back through atomics in
//!   [`Shared`].
//!
//! The same `Engine` renders offline (export, tests) — there is one code path.

mod engine;
mod metronome;
mod shared;
mod snapshot;

pub use engine::{Command, Engine, EngineConfig, EngineHandle, Garbage, MASTER_TRACK, MIXER_TRACKS};
pub use shared::{Shared, StereoMeter};
pub use snapshot::{PlaybackNote, PlaybackSnapshot};
