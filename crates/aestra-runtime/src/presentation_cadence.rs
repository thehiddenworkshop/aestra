//! An effect-relative sample clock shared by presentation and offline tools.

use std::num::NonZeroU32;

/// Hold the sample at the start of each presentation frame.
///
/// The clock has no accumulated state. A seek, loop restart, or skipped
/// display frame selects the same sample as continuous playback at that time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationCadence(NonZeroU32);

impl PresentationCadence {
    pub const FPS_12: Self = Self(NonZeroU32::new(12).unwrap());

    pub const fn new(frames_per_second: NonZeroU32) -> Self {
        Self(frames_per_second)
    }

    pub const fn frames_per_second(self) -> NonZeroU32 {
        self.0
    }

    /// The fixed simulation tick to show at `tick`. When a presentation frame
    /// starts between two ticks, use the first tick at or after that boundary.
    pub fn sample_tick(self, tick: u32, ticks_per_second: NonZeroU32) -> u32 {
        let fps = u64::from(self.0.get());
        let tick_rate = u64::from(ticks_per_second.get());
        let frame = u64::from(tick) * fps / tick_rate;
        (frame * tick_rate).div_ceil(fps) as u32
    }

    /// The start of the frame containing `seconds`, in effect-local seconds.
    pub fn sample_time(self, seconds: f32) -> f32 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0.0;
        }
        let fps = f64::from(self.0.get());
        let frame = (f64::from(seconds) * fps).floor();
        // Timeline seconds and sample boundaries are published as f32. Treat
        // an exact match in that representation as the start of the new frame.
        let next = ((frame + 1.0) / fps) as f32;
        if next <= seconds {
            next
        } else {
            (frame / fps) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlaybackClock;

    #[test]
    fn sixty_playback_ticks_produce_twelve_deterministic_samples() {
        let mut clock = PlaybackClock::default();
        let cadence = PresentationCadence::FPS_12;
        for tick in (0..=120).rev().chain(0..=120) {
            clock.seek_frame(tick, 2.0);
            assert_eq!(
                cadence.sample_time(clock.time(2.0)),
                (tick / 5) as f32 / 12.0
            );
            assert_eq!(clock.frame(), tick);
            assert_eq!(
                cadence.sample_tick(tick as u32, NonZeroU32::new(60).unwrap()),
                (tick / 5 * 5) as u32
            );
        }
        let boundary = 1.0_f32 / 12.0;
        assert_eq!(cadence.sample_time(boundary.next_down()), 0.0);
        assert_eq!(cadence.sample_time(boundary), boundary);
    }

    #[test]
    fn fixed_samples_follow_non_divisor_and_fast_rates_without_future_ticks() {
        let rate = NonZeroU32::new(60).unwrap();
        let cadence = PresentationCadence::new(NonZeroU32::new(24).unwrap());
        assert_eq!(
            (0..=10)
                .map(|tick| cadence.sample_tick(tick, rate))
                .collect::<Vec<_>>(),
            [0, 0, 0, 3, 3, 5, 5, 5, 8, 8, 10],
        );
        let fast = PresentationCadence::new(NonZeroU32::new(u32::MAX).unwrap());
        assert_eq!(fast.sample_tick(u32::MAX, rate), u32::MAX);
    }
}
