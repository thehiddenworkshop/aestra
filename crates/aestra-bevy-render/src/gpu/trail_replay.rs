//! Render-world history reconstruction. Advancing CPU time repeatedly before a
//! render does not simulate intermediate frames: each observation needs its own
//! GPU dispatch and ordered time upload.

const MAX_STEPS_PER_FRAME: usize = 240;

#[derive(Default)]
pub(super) struct TrailReplay {
    epoch: Option<u32>,
    time: f32,
    replaying: bool,
}

impl TrailReplay {
    pub(super) fn observations(&mut self, epoch: u32, target: f32) -> Vec<f32> {
        let mut times = Vec::new();
        if self.epoch != Some(epoch) || target < self.time {
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
