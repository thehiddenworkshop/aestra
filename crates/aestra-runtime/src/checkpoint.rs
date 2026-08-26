use aestra_core::EffectId;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationSeekMode {
    /// The effect is an analytical function of time and can jump directly to any frame.
    StatelessDirect,
    /// The backend can snapshot state and replay fixed ticks from a restored checkpoint.
    CheckpointRestore,
    /// The backend is stateful but cannot snapshot, so backward seeks restart and replay.
    RestartReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckpointBackendId(String);

impl CheckpointBackendId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CheckpointContext {
    pub effect: EffectId,
    pub revision: u64,
    pub seed: u64,
    pub backend: CheckpointBackendId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointPolicy {
    pub cadence_frames: u64,
    pub max_entries: usize,
    pub max_bytes: usize,
}

impl Default for CheckpointPolicy {
    fn default() -> Self {
        Self {
            cadence_frames: 30,
            max_entries: 64,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

impl CheckpointPolicy {
    pub fn should_capture(self, frame: u64) -> bool {
        frame == 0 || frame.is_multiple_of(self.cadence_frames.max(1))
    }
}

#[derive(Debug, Clone)]
pub struct StoredCheckpoint<T> {
    pub context: CheckpointContext,
    pub frame: u64,
    pub state: T,
    pub estimated_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct CheckpointStore<T> {
    policy: CheckpointPolicy,
    entries: VecDeque<StoredCheckpoint<T>>,
    estimated_bytes: usize,
}

impl<T> Default for CheckpointStore<T> {
    fn default() -> Self {
        Self::new(CheckpointPolicy::default())
    }
}

impl<T> CheckpointStore<T> {
    pub fn new(mut policy: CheckpointPolicy) -> Self {
        policy.cadence_frames = policy.cadence_frames.max(1);
        policy.max_entries = policy.max_entries.max(1);
        policy.max_bytes = policy.max_bytes.max(1);
        Self {
            policy,
            entries: VecDeque::new(),
            estimated_bytes: 0,
        }
    }

    pub fn policy(&self) -> CheckpointPolicy {
        self.policy
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    pub fn insert(
        &mut self,
        context: CheckpointContext,
        frame: u64,
        state: T,
        estimated_bytes: usize,
    ) -> bool {
        if estimated_bytes > self.policy.max_bytes {
            return false;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.context == context && entry.frame == frame)
            && let Some(replaced) = self.entries.remove(index)
        {
            self.estimated_bytes = self
                .estimated_bytes
                .saturating_sub(replaced.estimated_bytes);
        }
        self.estimated_bytes = self.estimated_bytes.saturating_add(estimated_bytes);
        self.entries.push_back(StoredCheckpoint {
            context,
            frame,
            state,
            estimated_bytes,
        });
        while self.entries.len() > self.policy.max_entries
            || self.estimated_bytes > self.policy.max_bytes
        {
            let Some(evicted) = self.entries.pop_front() else {
                break;
            };
            self.estimated_bytes = self.estimated_bytes.saturating_sub(evicted.estimated_bytes);
        }
        true
    }

    pub fn nearest_at_or_before(
        &self,
        context: &CheckpointContext,
        frame: u64,
    ) -> Option<&StoredCheckpoint<T>> {
        self.entries
            .iter()
            .filter(|entry| &entry.context == context && entry.frame <= frame)
            .max_by_key(|entry| entry.frame)
    }

    pub fn invalidate_context(&mut self, context: &CheckpointContext) {
        self.entries.retain(|entry| &entry.context != context);
        self.recalculate_bytes();
    }

    pub fn invalidate_effect(&mut self, effect: EffectId) {
        self.entries.retain(|entry| entry.context.effect != effect);
        self.recalculate_bytes();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.estimated_bytes = 0;
    }

    pub fn plan_seek(
        &self,
        mode: SimulationSeekMode,
        context: &CheckpointContext,
        current_frame: u64,
        target_frame: u64,
    ) -> SeekPlan {
        if mode == SimulationSeekMode::StatelessDirect {
            return SeekPlan {
                target_frame,
                origin: SeekOrigin::Direct,
                replay_ticks: 0,
            };
        }
        if target_frame >= current_frame {
            return SeekPlan {
                target_frame,
                origin: SeekOrigin::Current,
                replay_ticks: target_frame - current_frame,
            };
        }
        if mode == SimulationSeekMode::CheckpointRestore
            && let Some(checkpoint) = self.nearest_at_or_before(context, target_frame)
        {
            return SeekPlan {
                target_frame,
                origin: SeekOrigin::Checkpoint {
                    frame: checkpoint.frame,
                },
                replay_ticks: target_frame - checkpoint.frame,
            };
        }
        SeekPlan {
            target_frame,
            origin: SeekOrigin::Restart,
            replay_ticks: target_frame,
        }
    }

    fn recalculate_bytes(&mut self) {
        self.estimated_bytes = self.entries.iter().map(|entry| entry.estimated_bytes).sum();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekOrigin {
    Direct,
    Current,
    Checkpoint { frame: u64 },
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekPlan {
    pub target_frame: u64,
    pub origin: SeekOrigin,
    pub replay_ticks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(revision: u64, seed: u64) -> CheckpointContext {
        CheckpointContext {
            effect: EffectId::from_u128(1),
            revision,
            seed,
            backend: CheckpointBackendId::new("test"),
        }
    }

    #[test]
    fn store_finds_nearest_compatible_checkpoint() {
        let mut store = CheckpointStore::default();
        let current = context(2, 7);
        store.insert(current.clone(), 30, "thirty", 8);
        store.insert(current.clone(), 60, "sixty", 8);
        store.insert(context(1, 7), 75, "stale", 8);
        store.insert(context(2, 8), 80, "other seed", 8);

        let checkpoint = store.nearest_at_or_before(&current, 70).unwrap();
        assert_eq!(checkpoint.frame, 60);
        assert_eq!(checkpoint.state, "sixty");
    }

    #[test]
    fn store_evicts_oldest_entries_to_respect_limits() {
        let mut store = CheckpointStore::new(CheckpointPolicy {
            cadence_frames: 10,
            max_entries: 3,
            max_bytes: 20,
        });
        let current = context(1, 0);
        store.insert(current.clone(), 0, 0, 8);
        store.insert(current.clone(), 10, 10, 8);
        store.insert(current.clone(), 20, 20, 8);

        assert_eq!(store.len(), 2);
        assert!(store.nearest_at_or_before(&current, 5).is_none());
        assert_eq!(store.estimated_bytes(), 16);
        assert!(!store.insert(current, 30, 30, 21));
    }

    #[test]
    fn backward_seek_prefers_checkpoint_then_replays() {
        let mut store = CheckpointStore::default();
        let current = context(1, 42);
        store.insert(current.clone(), 30, (), 1);
        store.insert(current.clone(), 60, (), 1);

        assert_eq!(
            store.plan_seek(SimulationSeekMode::CheckpointRestore, &current, 100, 75),
            SeekPlan {
                target_frame: 75,
                origin: SeekOrigin::Checkpoint { frame: 60 },
                replay_ticks: 15,
            }
        );
        assert_eq!(
            store.plan_seek(SimulationSeekMode::RestartReplay, &current, 100, 75),
            SeekPlan {
                target_frame: 75,
                origin: SeekOrigin::Restart,
                replay_ticks: 75,
            }
        );
        assert_eq!(
            store.plan_seek(SimulationSeekMode::StatelessDirect, &current, 100, 75),
            SeekPlan {
                target_frame: 75,
                origin: SeekOrigin::Direct,
                replay_ticks: 0,
            }
        );
    }

    #[test]
    fn invalidation_removes_stale_revision_and_seed_state() {
        let mut store = CheckpointStore::default();
        let old = context(1, 1);
        let current = context(2, 2);
        store.insert(old.clone(), 30, (), 4);
        store.insert(current.clone(), 30, (), 4);
        store.invalidate_context(&old);
        assert!(store.nearest_at_or_before(&old, 60).is_none());
        assert!(store.nearest_at_or_before(&current, 60).is_some());
        store.invalidate_effect(current.effect);
        assert!(store.is_empty());
    }
}
