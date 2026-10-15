//! Production track color and localized label helpers shared by timeline compositions.
use crate::localization::Localizer;
use aestra_core::{EffectAssetRef, EmitterId};
use bevy::prelude::Color;
use fluent_bundle::FluentArgs;

pub(super) fn emitter_timing_label(localizer: &Localizer, message_id: &str, name: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    localizer.text_with(message_id, &args)
}

pub(super) fn layer_color(emitter: EmitterId, display_color: Option<[f32; 4]>) -> Color {
    if let Some([red, green, blue, alpha]) = display_color {
        return Color::srgba(red, green, blue, alpha);
    }
    let id = emitter.as_uuid().as_u128();
    let palette_index = (id ^ (id >> 32) ^ (id >> 64) ^ (id >> 96)) as usize % 4;
    match palette_index {
        0 => Color::srgb(0.48, 0.31, 0.98),
        1 => Color::srgb(0.17, 0.75, 0.95),
        2 => Color::srgb(0.98, 0.47, 0.21),
        _ => Color::srgb(0.84, 0.29, 0.72),
    }
}

pub(super) fn effect_reference_color(source: EffectAssetRef) -> Color {
    let id = source.id.as_uuid().as_u128();
    let palette_index = (id ^ (id >> 32) ^ (id >> 64) ^ (id >> 96)) as usize % 4;
    match palette_index {
        0 => Color::srgb(0.40, 0.72, 0.98),
        1 => Color::srgb(0.55, 0.82, 0.45),
        2 => Color::srgb(0.96, 0.65, 0.27),
        _ => Color::srgb(0.72, 0.47, 0.98),
    }
}
