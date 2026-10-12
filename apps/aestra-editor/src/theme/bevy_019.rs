//! Shipping Bevy 0.19 Feathers color-map boundary.
use super::palette::{BORDER, PANEL, PANEL_DARK};
use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, tokens};

pub fn feathers_theme() -> UiTheme {
    let mut theme = UiTheme(create_dark_theme());
    theme.0.color.extend(super::palette::colors());
    theme.0.color.insert(tokens::GROUP_HEADER_BG, PANEL_DARK);
    theme.0.color.insert(tokens::GROUP_HEADER_BORDER, BORDER);
    theme.0.color.insert(tokens::GROUP_BODY_BG, PANEL);
    theme.0.color.insert(tokens::GROUP_BODY_BORDER, BORDER);
    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipping_group_header_and_body_colors_are_preserved() {
        let theme = feathers_theme();
        assert_eq!(theme.color(&tokens::GROUP_HEADER_BG), PANEL_DARK);
        assert_eq!(theme.color(&tokens::GROUP_HEADER_BORDER), BORDER);
        assert_eq!(theme.color(&tokens::GROUP_BODY_BG), PANEL);
        assert_eq!(theme.color(&tokens::GROUP_BODY_BORDER), BORDER);
    }
}
