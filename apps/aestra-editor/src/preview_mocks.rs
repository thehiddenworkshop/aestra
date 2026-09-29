//! Editor-only stand-ins for an effect's host objects (host bindings HB11c). A binding the artist
//! gives a stand-in — held still, orbiting, flying by — is fed to the preview as a recorded
//! [`BindingTrace`], so homing, attached emitters and bound inputs play, seek and scrub exactly
//! without a game. Stand-ins are session state: the effect asset never names one.

use aestra_core::BindingId;
use aestra_runtime::{
    BindingFrame, BindingTrace, CompiledEffect, EffectInstance, SpatialBindingSnapshot,
    StatefulSimulation,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// How far the stand-ins are recorded: five minutes of preview. Past it they hold still.
const PREVIEW_MOCK_SECONDS: f32 = 300.0;

/// A stand-in's motion, in world space around the preview's origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PreviewMock {
    /// Held in front of the effect.
    Still,
    /// Circling the effect at 4 m.
    Orbit,
    /// Crossing in front of the effect every four seconds.
    FlyBy,
}

impl PreviewMock {
    pub(crate) const ALL: [Self; 3] = [Self::Still, Self::Orbit, Self::FlyBy];

    /// The Fluent message naming this stand-in.
    pub(crate) fn message_id(self) -> &'static str {
        match self {
            Self::Still => "interface-mock-still",
            Self::Orbit => "interface-mock-orbit",
            Self::FlyBy => "interface-mock-fly-by",
        }
    }

    /// Position and velocity at `time` seconds.
    pub(crate) fn pose(self, time: f32) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::Still => ([0.0, 2.0, 5.0], [0.0; 3]),
            Self::Orbit => {
                const RADIUS: f32 = 4.0;
                const SPEED: f32 = 1.2;
                let (sin, cos) = (time * SPEED).sin_cos();
                (
                    [RADIUS * cos, 1.5, RADIUS * sin],
                    [-RADIUS * SPEED * sin, 0.0, RADIUS * SPEED * cos],
                )
            }
            Self::FlyBy => {
                const PERIOD: f32 = 4.0;
                const HALF_SPAN: f32 = 8.0;
                let phase = (time / PERIOD).rem_euclid(1.0);
                (
                    [-HALF_SPAN + 2.0 * HALF_SPAN * phase, 2.0, 4.0],
                    [2.0 * HALF_SPAN / PERIOD, 0.0, 0.0],
                )
            }
        }
    }
}

/// The stand-ins of `effect`'s bindings as one trace, or `None` when none has a stand-in.
pub(crate) fn preview_mock_trace(
    effect: &CompiledEffect,
    mocks: &BTreeMap<BindingId, PreviewMock>,
) -> Option<BindingTrace> {
    let slots = effect
        .bindings
        .iter()
        .enumerate()
        .filter_map(|(slot, binding)| mocks.get(&binding.source).map(|mock| (slot, *mock)))
        .collect::<Vec<_>>();
    if slots.is_empty() {
        return None;
    }
    let ticks = (PREVIEW_MOCK_SECONDS / StatefulSimulation::TICK_DT) as u64;
    Some(BindingTrace::from_fn(ticks, |tick| {
        let time = tick as f32 * StatefulSimulation::TICK_DT;
        let mut snapshots = vec![None; effect.bindings.len()];
        for &(slot, mock) in &slots {
            let (position, velocity) = mock.pose(time);
            let mut object = SpatialBindingSnapshot::at(position);
            object.linear_velocity = Some(velocity);
            snapshots[slot] = Some(object.to_snapshot(&effect.bindings[slot].layout));
        }
        BindingFrame { snapshots }
    }))
}

/// The traces built for recent compiled effects, so each is recorded once per edit rather than
/// every frame.
#[derive(Default)]
pub(crate) struct PreviewMockTraces {
    entries: Vec<CachedTrace>,
}

struct CachedTrace {
    effect: Arc<CompiledEffect>,
    mocks: BTreeMap<BindingId, PreviewMock>,
    trace: Option<Arc<BindingTrace>>,
}

impl PreviewMockTraces {
    const CAPACITY: usize = 4;

    /// The stand-in trace for `effect`.
    pub(crate) fn trace(
        &mut self,
        effect: &Arc<CompiledEffect>,
        mocks: &BTreeMap<BindingId, PreviewMock>,
    ) -> Option<Arc<BindingTrace>> {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| Arc::ptr_eq(&entry.effect, effect) && &entry.mocks == mocks)
        {
            return entry.trace.clone();
        }
        let trace = preview_mock_trace(effect, mocks).map(Arc::new);
        if self.entries.len() == Self::CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push(CachedTrace {
            effect: effect.clone(),
            mocks: mocks.clone(),
            trace: trace.clone(),
        });
        trace
    }
}

/// Whether `instance` is driven by something other than `trace`.
pub(crate) fn needs_trace(instance: &EffectInstance, trace: Option<&Arc<BindingTrace>>) -> bool {
    instance
        .binding_trace()
        .map(|installed| installed.identity())
        != trace.map(|trace| trace.identity())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{BindingUpdateMode, EffectBinding};
    use aestra_runtime::BindingSlot;

    fn bound_effect() -> (Arc<CompiledEffect>, BindingId) {
        let mut effect = crate::test_support::session_with_timing_slack().effect;
        let target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
        let id = target.id;
        effect.bindings.push(target);
        let compiled = aestra_compiler::EffectCompiler::with_extensions(
            aestra_compiler::ExtensionRegistry::builtin(),
        )
        .compile(&effect)
        .expect("a declared binding compiles");
        (Arc::new(compiled), id)
    }

    #[test]
    fn a_stand_in_drives_the_preview_and_seeks_exactly() {
        let (effect, id) = bound_effect();
        let mocks = BTreeMap::from([(id, PreviewMock::Orbit)]);
        let mut traces = PreviewMockTraces::default();
        let trace = traces
            .trace(&effect, &mocks)
            .expect("a stand-in records a trace");
        assert!(Arc::ptr_eq(&trace, &traces.trace(&effect, &mocks).unwrap()));

        let mut instance = EffectInstance::new(effect.clone());
        assert!(needs_trace(&instance, Some(&trace)));
        instance.set_binding_trace(Some(trace.clone()));
        assert!(!needs_trace(&instance, Some(&trace)));
        let position = aestra_core::BindingFieldId::new(aestra_core::AESTRA_FIELD_POSITION);
        let at = |instance: &mut EffectInstance, time: f32| {
            instance.seek(time);
            instance
                .binding_field(BindingSlot(0), &position)
                .map(<[f32]>::to_vec)
        };
        let late = at(&mut instance, 2.0).expect("the stand-in is bound");
        let early = at(&mut instance, 0.5).expect("the stand-in is bound");
        assert_ne!(late, early);
        let tick = aestra_runtime::trace_tick(2.0);
        let (expected, _) = PreviewMock::Orbit.pose(tick as f32 * StatefulSimulation::TICK_DT);
        assert_eq!(at(&mut instance, 2.0).unwrap(), expected);

        // No stand-in: nothing drives the bindings.
        assert!(traces.trace(&effect, &BTreeMap::new()).is_none());
    }

    #[test]
    fn stand_ins_move_as_described() {
        assert_eq!(PreviewMock::Still.pose(3.0), PreviewMock::Still.pose(0.0));
        let (start, velocity) = PreviewMock::FlyBy.pose(0.0);
        let (later, _) = PreviewMock::FlyBy.pose(1.0);
        assert!((later[0] - start[0] - velocity[0]).abs() < 1e-4);
        let (orbit, _) = PreviewMock::Orbit.pose(1.7);
        assert!((orbit[0].hypot(orbit[2]) - 4.0).abs() < 1e-4);
    }
}
