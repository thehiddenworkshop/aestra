//! Render-world history reconstruction. Advancing CPU time repeatedly before a
//! render does not simulate intermediate frames: each observation needs its own
//! GPU dispatch and ordered time upload.

const MAX_STEPS_PER_FRAME: usize = 240;

#[derive(Default)]
pub(super) struct TrailReplay {
    epoch: Option<u32>,
    time: f32,
    replaying: bool,
    checkpoint_times: Vec<f32>,
}

impl TrailReplay {
    /// A supplied trajectory can be observed at every canonical simulation tick
    /// during live playback as well as seeking. Never cache a path contaminated
    /// by a previous sub-frame observation: restore/replay the canonical prefix.
    pub(super) fn prepare_tracked(&mut self, target: f32) {
        if target <= self.time || self.replaying {
            return;
        }
        let frame = self.time as f64 * 60.0;
        if (frame - frame.round()).abs() > 0.0001 {
            self.epoch = None;
        }
        self.replaying = true;
    }
    pub(super) fn context_changed(&mut self) {
        // A pending seek must reconstruct in the new context. Ordinary moving-host
        // playback retains its observed world path, but never produces checkpoints.
        if self.replaying {
            *self = Self::default();
        }
    }

    pub(super) fn needs_restore(&self, epoch: u32, target: f32) -> bool {
        self.epoch != Some(epoch) || target < self.time
    }

    pub(super) fn restore(&mut self, epoch: u32, time: f32) {
        self.epoch = Some(epoch);
        self.time = time;
        self.replaying = true;
    }

    pub(super) fn should_capture(&self, time: f32) -> bool {
        self.checkpoint_times.contains(&time)
    }

    pub(super) fn observations(&mut self, epoch: u32, target: f32) -> Vec<f32> {
        let mut times = Vec::new();
        self.checkpoint_times.clear();
        if self.needs_restore(epoch, target) {
            self.epoch = Some(epoch);
            self.time = 0.0;
            self.replaying = true;
            times.push(0.0);
        }
        if self.replaying {
            while self.time < target && times.len() < MAX_STEPS_PER_FRAME {
                // Frame-aligned observations plus the exact (possibly sub-frame)
                // target. A bounded batch keeps long continuous seeks responsive.
                let next = ((self.time as f64 * 60.0 + 0.0001).floor() + 1.0) / 60.0;
                let next = (next as f32).max(self.time.next_up()).min(target);
                self.time = next;
                times.push(next);
                // Only canonical, frame-aligned replay is reusable. Live playback
                // can have arbitrary observation times or a moving host transform.
                if next >= 1.0 && next.fract() == 0.0 {
                    self.checkpoint_times.push(next);
                }
            }
            self.replaying = self.time < target;
        } else {
            self.time = target;
            times.push(target);
        }
        if times.is_empty() {
            times.push(target);
        }
        times
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const STEP: f32 = 1.0 / 60.0;

    #[test]
    fn tracked_playback_fills_skipped_ticks_and_rebuilds_after_subframe_samples() {
        let mut replay = TrailReplay::default();
        replay.observations(1, 1.0);
        replay.prepare_tracked(2.0);
        assert!(!replay.needs_restore(1, 2.0));
        assert_eq!(replay.observations(1, 2.0).len(), 60);
        assert!(replay.should_capture(2.0));
        replay.prepare_tracked(2.005);
        assert_eq!(replay.observations(1, 2.005), vec![2.005]);
        replay.prepare_tracked(2.005);
        assert!(!replay.needs_restore(1, 2.005));
        replay.prepare_tracked(2.5);
        assert!(replay.needs_restore(1, 2.5));
        replay.restore(1, 2.0);
        assert_eq!(replay.observations(1, 2.5).len(), 30);
    }

    #[test]
    fn checkpoints_resume_at_the_next_frame_and_never_capture_live_observations() {
        let mut replay = TrailReplay::default();
        replay.observations(1, 2.005);
        assert!(replay.should_capture(1.0));
        assert!(replay.should_capture(2.0));
        assert!(!replay.should_capture(2.005));
        replay.observations(1, 3.0);
        assert!(!replay.should_capture(3.0));
        replay.context_changed();
        assert!(!replay.needs_restore(1, 3.0));
        assert!(replay.needs_restore(2, 1.5));
        replay.restore(2, 1.0);
        let times = replay.observations(2, 1.5);
        assert_eq!(times.len(), 30);
        assert_eq!(times[0], 61.0 / 60.0);
        assert_eq!(*times.last().unwrap(), 1.5);
        replay.restore(3, 1.0);
        assert_eq!(replay.observations(3, 1.0), vec![1.0]);
        replay.observations(4, 10.0);
        replay.context_changed();
        assert_eq!(replay.observations(4, 0.5)[0], 0.0);
    }

    #[test]
    fn seek_replays_intermediate_frames_and_pause_preserves_the_result() {
        let mut replay = TrailReplay::default();
        let target = 86.0 / 60.0;
        let times = replay.observations(1, target);
        assert_eq!(times.len(), 87);
        assert_eq!(times[0], 0.0);
        assert_eq!(*times.last().unwrap(), target);
        assert!(
            times
                .windows(2)
                .all(|w| w[1] > w[0] && w[1] - w[0] <= STEP + 1e-6)
        );
        assert_eq!(replay.observations(1, target), vec![target]);
        assert_eq!(replay.observations(1, target + STEP), vec![target + STEP]);
        assert_eq!(replay.observations(2, 0.5).len(), 31);
        assert_eq!(replay.observations(3, 0.0), vec![0.0]);
    }

    #[test]
    fn long_seeks_are_batched_and_new_seeks_cancel_pending_history() {
        let mut replay = TrailReplay::default();
        let first = replay.observations(1, 10.005);
        assert_eq!(first.len(), MAX_STEPS_PER_FRAME);
        let second = replay.observations(1, 10.005);
        assert!(second[0] > *first.last().unwrap());
        assert_eq!(second.len(), MAX_STEPS_PER_FRAME);
        let third = replay.observations(1, 10.005);
        assert_eq!(*third.last().unwrap(), 10.005);
        assert_eq!(replay.observations(1, 10.005), vec![10.005]);
        assert_eq!(replay.observations(2, 20.0).len(), MAX_STEPS_PER_FRAME);
        assert_eq!(replay.observations(3, 0.005), vec![0.0, 0.005]);
        assert_eq!(replay.observations(3, 0.0), vec![0.0]);
    }
}
