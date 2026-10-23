//! Contextual Feathers theme with Aestra's semantic palette.
use bevy::feathers::{
    dark_theme::create_dark_theme,
    theme::{SemanticToken, UiTheme},
};

pub fn feathers_theme() -> UiTheme {
    let mut theme = UiTheme(create_dark_theme());
    let mut colors = super::palette::colors();
    // 0.20 merges group header/body surfaces. Preserve the existing body fill;
    // Aestra's custom headers continue using PANEL_DARK directly.
    colors.insert(bevy::feathers::tokens::GROUP_BG, super::palette::PANEL);
    colors.insert(bevy::feathers::tokens::GROUP_BORDER, super::palette::BORDER);
    for (token, color) in colors {
        // Upstream shares semantic fills between controls, but Aestra intentionally
        // distinguishes menu hover, plain-button hover and list selection. Give
        // each existing assignment its own semantic identity so contextual upstream
        // overrides cannot silently recolor it. Unassigned tokens (e.g. axis sigils)
        // retain the complete upstream defaults. Contextual restyling is not part
        // of the required API port: current colors stay exact on every surface.
        let semantic = SemanticToken::new(format!("aestra.compat.{token}").into());
        theme.0.token_assignments.insert(token, semantic.clone());
        theme.0.semantic_base.insert(semantic, color);
    }
    theme
}
