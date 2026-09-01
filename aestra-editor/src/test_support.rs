//! Deterministic semantic fixtures for editor behavior tests.
//!
//! Showcase effects are intentionally editable visual content. Behavioral tests should use these
//! compact builders unless the test explicitly verifies bundled content or end-to-end examples.

use crate::session::EditorSession;
use aestra_bevy::{
    ChoreographyTrackId, CurveId, EffectAsset, EffectId, EffectPlaybackMode, Emitter, EmitterId,
    GradientId, ModuleId, ModuleParameters, RendererId,
};

const TEST_EFFECT_ID: EffectId = EffectId::from_u128(0xa357_4a10_0000_4000_8000_0000_0000_0001);
const TEST_EMITTER_ID: EmitterId = EmitterId::from_u128(0xa357_4a10_0000_4000_8000_0000_0000_0010);
const TEST_SECOND_EMITTER_ID: EmitterId =
    EmitterId::from_u128(0xa357_4a10_0000_4000_8000_0000_0000_0011);
const TEST_THIRD_EMITTER_ID: EmitterId =
    EmitterId::from_u128(0xa357_4a10_0000_4000_8000_0000_0000_0012);
const TEST_FOURTH_EMITTER_ID: EmitterId =
    EmitterId::from_u128(0xa357_4a10_0000_4000_8000_0000_0000_0013);

fn assign_stable_emitter_ids(emitter: &mut Emitter, id: EmitterId, base: u128) {
    emitter.id = id;
    for (index, module) in emitter.modules.iter_mut().enumerate() {
        module.id = ModuleId::from_u128(base + index as u128);
        if let ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } = &mut module.parameters
        {
            size.id = CurveId::from_u128(base + 0x100);
            opacity.id = CurveId::from_u128(base + 0x101);
            color.id = GradientId::from_u128(base + 0x102);
        }
    }
    for (index, renderer) in emitter.renderers.iter_mut().enumerate() {
        renderer.id = RendererId::from_u128(base + 0x200 + index as u128);
    }
}

pub(crate) fn effect_with_timing_slack() -> EffectAsset {
    let mut effect = EffectAsset::new("Editor Test Effect", 4.0);
    effect.id = TEST_EFFECT_ID;
    effect.playback_mode = EffectPlaybackMode::LoopRestart;

    let mut emitter = Emitter::basic_sprite("Test Emitter", 2.5);
    emitter.start_time = 0.5;
    assign_stable_emitter_ids(
        &mut emitter,
        TEST_EMITTER_ID,
        0xa357_4a10_0000_4000_8000_0000_0000_0100,
    );

    let mut second = Emitter::basic_sprite("Other Test Emitter", effect.duration);
    assign_stable_emitter_ids(
        &mut second,
        TEST_SECOND_EMITTER_ID,
        0xa357_4a10_0000_4000_8000_0000_0000_1100,
    );

    let mut third = Emitter::basic_sprite("Third Test Emitter", 3.0);
    third.start_time = 0.25;
    assign_stable_emitter_ids(
        &mut third,
        TEST_THIRD_EMITTER_ID,
        0xa357_4a10_0000_4000_8000_0000_0000_2100,
    );

    let mut fourth = Emitter::basic_sprite("Fourth Test Emitter", 1.75);
    fourth.start_time = 1.0;
    assign_stable_emitter_ids(
        &mut fourth,
        TEST_FOURTH_EMITTER_ID,
        0xa357_4a10_0000_4000_8000_0000_0000_3100,
    );

    effect.choreography_order = vec![
        ChoreographyTrackId::Emitter(emitter.id),
        ChoreographyTrackId::Emitter(second.id),
        ChoreographyTrackId::Emitter(third.id),
        ChoreographyTrackId::Emitter(fourth.id),
    ];
    effect.emitters.push(emitter);
    effect.emitters.push(second);
    effect.emitters.push(third);
    effect.emitters.push(fourth);
    effect
}

pub(crate) fn session_with_timing_slack() -> EditorSession {
    EditorSession::from_test_effect(effect_with_timing_slack())
}

pub(crate) fn session_with_playback_mode(mode: EffectPlaybackMode) -> EditorSession {
    let mut effect = effect_with_timing_slack();
    effect.playback_mode = mode;
    EditorSession::from_test_effect(effect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_valid_compilable_and_has_timing_slack() {
        let session = session_with_timing_slack();
        let emitter = &session.effect.emitters[0];

        assert!(emitter.start_time > 0.0);
        assert!(emitter.start_time + emitter.duration < session.effect.duration);
        assert!(session.preview.is_some());
    }

    #[test]
    fn fixture_semantic_ids_are_stable() {
        assert_eq!(effect_with_timing_slack(), effect_with_timing_slack());
    }
}
