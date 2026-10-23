use super::*;
use bevy::feathers::tokens;

#[test]
fn menu_items_share_visible_hover_and_keyboard_focus_colors() {
    let theme = feathers_theme();
    assert_eq!(theme.color(&tokens::MENUITEM_BG_HOVER), MENU_ITEM_HOVER);
    assert_eq!(theme.color(&tokens::MENUITEM_BG_FOCUSED), MENU_ITEM_HOVER);
    assert_eq!(theme.color(&tokens::MENUITEM_BG_PRESSED), MENU_ITEM_PRESSED);
    assert_ne!(MENU_ITEM_HOVER, theme.color(&tokens::MENU_BG));
}

#[test]
fn every_editor_assignment_survives_theme_construction() {
    let theme = feathers_theme();
    for (token, color) in palette::colors() {
        assert_eq!(theme.color(&token), color, "{token}");
    }
}
