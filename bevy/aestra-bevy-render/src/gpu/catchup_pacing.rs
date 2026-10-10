//! Catch-up budgeting shared by coupled particles and independent stages.
use aestra_runtime::SeekQuality;
use bevy::prelude::Resource;

/// Cap on fixed ticks advanced in a single frame, so a large seek or a first frame far into the
/// timeline cannot stall the GPU; the simulation catches up over subsequent frames.
/// Per-frame fixed-tick catch-up budget for an *exact* stateful seek: large, so a settled cursor
/// converges to the authoritative state in a few frames, but still bounded so one frame cannot stall
/// the GPU on a huge jump (the remainder continues on later frames).
pub(super) const STATEFUL_MAX_CATCHUP_TICKS: u32 = 300;

/// Per-frame catch-up budget for a *preview* seek (hybrid roadmap M12): tight, so rapid scrubbing stays
/// responsive. The reconstruction is temporally bounded — the presented state is an *exact* earlier
/// tick when the budget cannot reach the target, never a values-approximate one — and a preview is
/// never authoritative, so an exact pass on cursor-release replays the remainder to the target.
pub(super) const STATEFUL_PREVIEW_CATCHUP_TICKS: u32 = 24;

/// Paces how many fixed ticks one frame may simulate while a staged simulation catches up — a domain
/// rebuilt by an edit replaying from tick 0 to the playhead, a seek — from how long frames actually
/// take. A fluid tick costs milliseconds (and more at high resolution or with flow maps), so a fixed
/// tick budget either stalls the UI for a whole replay or crawls on cheap effects. The pace halves
/// after a frame that spent its budget and ran long, and grows while frames stay fast; it grows only
/// while catching up, starts over at a few ticks whenever a domain is rebuilt, and never drops below
/// what keeps playback in real time. Shared by the stages that advance alone and those coupled to
/// particles.
#[derive(Resource, Debug)]
pub(crate) struct CatchupPacer {
    ticks: f32,
    last_frame: Option<std::time::Instant>,
    saturated: bool,
    /// Off (see [`AestraCatchupPacing`]): every frame may spend the full budget.
    paced: bool,
}

/// Whether catch-up — a staged simulation replaying to the playhead after a rebuild or a seek — is
/// paced by frame time, so the UI stays responsive while it runs (the default, for editors), or may
/// spend the full per-quality budget every frame, so each frame shows exactly the tick it asks for
/// (captures, visual references, benchmarks). Main-world setting, read by the render world each frame.
#[derive(Resource, Clone, Copy, Debug)]
pub struct AestraCatchupPacing {
    pub paced: bool,
}

impl Default for AestraCatchupPacing {
    fn default() -> Self {
        Self { paced: true }
    }
}

/// A frame slower than this, having spent its catch-up budget, halves the pace.
const CATCHUP_FRAME_TARGET: std::time::Duration = std::time::Duration::from_millis(20);
/// Ticks a frame always may simulate: real-time playback at down to 30 frames per second.
const CATCHUP_MIN_TICKS: f32 = 2.0;
/// Where the pace starts, and starts over after a rebuild.
const CATCHUP_START_TICKS: f32 = 4.0;

impl Default for CatchupPacer {
    fn default() -> Self {
        Self {
            ticks: CATCHUP_START_TICKS,
            last_frame: None,
            saturated: false,
            paced: true,
        }
    }
}

impl CatchupPacer {
    /// Called once a frame, before any simulation: adapts the pace to the previous frame.
    pub(crate) fn frame(&mut self, now: std::time::Instant) {
        if let Some(last) = self.last_frame
            && self.saturated
        {
            self.ticks = if now.duration_since(last) > CATCHUP_FRAME_TARGET {
                (self.ticks * 0.5).max(CATCHUP_MIN_TICKS)
            } else {
                (self.ticks * 1.25 + 1.0).min(STATEFUL_MAX_CATCHUP_TICKS as f32)
            };
        }
        self.saturated = false;
        self.last_frame = Some(now);
    }

    /// A domain was rebuilt: its ticks may cost anything now.
    pub(crate) fn restart(&mut self) {
        self.ticks = CATCHUP_START_TICKS;
    }

    /// The ticks this frame may simulate for one effect at `quality`.
    pub(crate) fn budget(&self, quality: SeekQuality) -> u32 {
        if !self.paced {
            return stateful_catchup_budget(quality);
        }
        (self.ticks as u32).clamp(CATCHUP_MIN_TICKS as u32, stateful_catchup_budget(quality))
    }

    /// Takes the main world's [`AestraCatchupPacing`].
    pub(crate) fn set_paced(&mut self, paced: bool) {
        self.paced = paced;
    }

    /// Reports ticks simulated against the budget: spending it all means still catching up.
    pub(crate) fn spent(&mut self, ticks: u32, budget: u32) {
        if ticks >= budget {
            self.saturated = true;
        }
    }
}

/// The per-frame catch-up budget for a stateful seek at the requested quality (hybrid roadmap M12).
pub(super) fn stateful_catchup_budget(quality: SeekQuality) -> u32 {
    match quality {
        SeekQuality::Preview => STATEFUL_PREVIEW_CATCHUP_TICKS,
        SeekQuality::Exact => STATEFUL_MAX_CATCHUP_TICKS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A replay's catch-up is paced by frame time: it grows while frames stay fast, halves after a
    /// slow frame that spent its budget, does not grow when nothing is catching up, keeps real-time
    /// playback, stays within the quality's cap, and starts over after a rebuild.
    #[test]
    fn catch_up_is_paced_by_frame_time() {
        use std::time::{Duration, Instant};
        let exact = SeekQuality::Exact;
        let mut pacer = CatchupPacer::default();
        let mut now = Instant::now();
        pacer.frame(now);
        assert_eq!(pacer.budget(exact), 4, "a rebuilt domain starts slow");

        let mut frame = |pacer: &mut CatchupPacer, spend: bool, length: u64| {
            if spend {
                let budget = pacer.budget(exact);
                pacer.spent(budget, budget);
            }
            now += Duration::from_millis(length);
            pacer.frame(now);
            pacer.budget(exact)
        };
        let mut budget = 4;
        for _ in 0..10 {
            budget = frame(&mut pacer, true, 10);
        }
        assert!(budget > 40, "fast frames catch up faster ({budget})");
        for _ in 0..10 {
            assert_eq!(
                frame(&mut pacer, false, 10),
                budget,
                "idle frames do not grow it"
            );
        }
        let halved = frame(&mut pacer, true, 60);
        assert_eq!(halved, budget / 2, "a slow frame halves it");
        for _ in 0..20 {
            frame(&mut pacer, true, 200);
        }
        assert_eq!(pacer.budget(exact), 2, "never below real-time playback");
        for _ in 0..40 {
            frame(&mut pacer, true, 5);
        }
        assert_eq!(pacer.budget(exact), STATEFUL_MAX_CATCHUP_TICKS);
        assert_eq!(
            pacer.budget(SeekQuality::Preview),
            STATEFUL_PREVIEW_CATCHUP_TICKS,
            "a preview seek keeps its tighter cap"
        );
        pacer.restart();
        assert_eq!(pacer.budget(exact), 4);
    }
}
