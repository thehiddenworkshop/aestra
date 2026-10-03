//! Bounded route high-water marks for asynchronous particle output rings.
use aestra_gpu::ParticleOutputRecord;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Delivery {
    heard: BTreeMap<u32, (u32, u64)>,
}

impl Delivery {
    pub(super) fn accept(
        &mut self,
        ring: u32,
        record: &ParticleOutputRecord,
        epoch: u32,
        suppress_through: u64,
    ) -> bool {
        if record.epoch != epoch || record.tick <= suppress_through {
            return false;
        }
        if self
            .heard
            .get(&ring)
            .is_some_and(|&(heard_epoch, tick)| heard_epoch == epoch && record.tick <= tick)
        {
            return false;
        }
        self.heard.insert(ring, (epoch, record.tick));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(epoch: u32, tick: u64) -> ParticleOutputRecord {
        ParticleOutputRecord {
            epoch,
            tick,
            count: 1,
            first: vec![(0, [1.0, 2.0, 3.0])],
        }
    }

    #[test]
    fn repeated_and_out_of_order_readbacks_never_replay_a_heard_tick() {
        let mut delivery = Delivery::default();
        assert!(delivery.accept(0, &record(0, 31), 0, 0));
        assert!(delivery.accept(0, &record(0, 32), 0, 0));
        assert!(!delivery.accept(0, &record(0, 31), 0, 0));
        assert!(!delivery.accept(0, &record(0, 32), 0, 0));
        assert!(delivery.accept(0, &record(0, 33), 0, 0));
        assert!(delivery.accept(100, &record(0, 31), 0, 0));
        assert_eq!(
            delivery.heard.len(),
            2,
            "memory is per route, not per event/tick"
        );
    }

    #[test]
    fn seek_silences_reconstruction_restart_allows_a_new_launch_and_old_epochs_are_rejected() {
        let mut delivery = Delivery::default();
        assert!(delivery.accept(0, &record(0, 100), 0, 0));
        assert!(!delivery.accept(0, &record(0, 101), 1, 50));
        assert!(!delivery.accept(0, &record(1, 50), 1, 50));
        assert!(delivery.accept(0, &record(1, 51), 1, 50));
        assert!(delivery.accept(0, &record(2, 1), 2, 0));
        assert!(!delivery.accept(0, &record(1, 52), 2, 0));
        assert!(!delivery.accept(0, &record(2, 1), 2, 0));
    }
}
