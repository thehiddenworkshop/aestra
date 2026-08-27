use bevy::{
    feathers::{dark_theme::create_dark_theme, theme::UiTheme, tokens},
    prelude::Color,
};

pub const APP_BG: Color = Color::srgb(0.027, 0.031, 0.047);
pub const MENU: Color = Color::srgb(0.032, 0.037, 0.055);
pub const PANEL_DARK: Color = Color::srgb(0.039, 0.045, 0.066);
pub const PANEL: Color = Color::srgb(0.055, 0.062, 0.087);
pub const PANEL_LIGHT: Color = Color::srgb(0.070, 0.078, 0.105);
pub const VIEWPORT_FRAME: Color = Color::srgb(0.020, 0.024, 0.038);
pub const VIEWPORT: Color = Color::srgb(0.013, 0.017, 0.030);
pub const TIMELINE_BG: Color = Color::srgb(0.030, 0.035, 0.052);
pub const BORDER: Color = Color::srgb(0.105, 0.116, 0.151);
pub const BORDER_BRIGHT: Color = Color::srgb(0.148, 0.164, 0.211);
pub const SPLITTER_GUTTER: Color = Color::srgb(0.024, 0.027, 0.039);
pub const SPLITTER: Color = Color::srgb(0.16, 0.18, 0.25);
pub const SPLITTER_HOVER: Color = Color::srgb(0.46, 0.31, 0.92);
pub const DOCK_TARGET: Color = Color::srgb(0.56, 0.40, 1.0);
pub const DOCK_TARGET_IDLE: Color = Color::srgba(0.31, 0.22, 0.58, 0.025);
pub const DOCK_TARGET_HOVER: Color = Color::srgba(0.38, 0.27, 0.70, 0.34);
pub const DOCK_TARGET_TEXT_IDLE: Color = Color::srgba(0.59, 0.62, 0.70, 0.42);
pub const DOCK_TARGET_LABEL_IDLE: Color = Color::srgba(0.03, 0.03, 0.06, 0.48);
pub const DOCK_TARGET_LABEL: Color = Color::srgba(0.07, 0.06, 0.12, 0.92);
pub const GRID: Color = Color::srgba(0.20, 0.23, 0.31, 0.18);
pub const BUTTON: Color = Color::srgb(0.085, 0.095, 0.128);
pub const BUTTON_HOVER: Color = Color::srgb(0.135, 0.143, 0.190);
pub const SELECTION: Color = Color::srgb(0.100, 0.089, 0.173);
pub const ACCENT: Color = Color::srgb(0.61, 0.47, 1.0);
pub const ACCENT_DIM: Color = Color::srgb(0.31, 0.23, 0.53);
pub const PLAYHEAD: Color = Color::srgb(0.95, 0.44, 0.78);
pub const TEXT: Color = Color::srgb(0.88, 0.90, 0.96);
pub const TEXT_MUTED: Color = Color::srgb(0.59, 0.62, 0.70);
pub const TEXT_FAINT: Color = Color::srgb(0.36, 0.39, 0.47);

/// Builds the Feathers theme used by the editor.
///
/// Feathers supplies the control behavior and token contract while Aestra owns the visual
/// language. Keeping the mapping here prevents standard controls from falling back to the
/// upstream gray/blue palette when the surrounding editor uses the Aestra palette.
pub fn feathers_theme() -> UiTheme {
    let mut theme = UiTheme(create_dark_theme());
    let colors = &mut theme.0.color;

    colors.insert(tokens::WINDOW_BG, APP_BG);
    colors.insert(tokens::FOCUS_RING, Color::srgba(0.61, 0.47, 1.0, 0.58));
    colors.insert(tokens::TEXT_MAIN, TEXT);
    colors.insert(tokens::TEXT_DIM, TEXT_MUTED);

    colors.insert(tokens::BUTTON_BG, BUTTON);
    colors.insert(tokens::BUTTON_BG_HOVER, BUTTON_HOVER);
    colors.insert(tokens::BUTTON_BG_PRESSED, ACCENT_DIM);
    colors.insert(tokens::BUTTON_BG_DISABLED, PANEL_LIGHT);
    colors.insert(tokens::BUTTON_TEXT, TEXT);
    colors.insert(
        tokens::BUTTON_TEXT_DISABLED,
        Color::srgba(0.59, 0.62, 0.70, 0.50),
    );
    colors.insert(tokens::BUTTON_PRIMARY_BG, Color::srgb(0.36, 0.25, 0.69));
    colors.insert(
        tokens::BUTTON_PRIMARY_BG_HOVER,
        Color::srgb(0.43, 0.31, 0.82),
    );
    colors.insert(tokens::BUTTON_PRIMARY_BG_PRESSED, ACCENT_DIM);
    colors.insert(tokens::BUTTON_PRIMARY_BG_DISABLED, PANEL_LIGHT);
    colors.insert(tokens::BUTTON_PRIMARY_TEXT, TEXT);
    colors.insert(
        tokens::BUTTON_PRIMARY_TEXT_DISABLED,
        Color::srgba(0.59, 0.62, 0.70, 0.50),
    );
    colors.insert(tokens::BUTTON_PLAIN_BG, Color::NONE);
    colors.insert(tokens::BUTTON_PLAIN_BG_HOVER, PANEL_LIGHT);
    colors.insert(tokens::BUTTON_PLAIN_BG_PRESSED, SELECTION);
    colors.insert(tokens::BUTTON_PLAIN_BG_DISABLED, Color::NONE);

    colors.insert(tokens::SLIDER_BG, PANEL);
    colors.insert(tokens::SLIDER_BG_HOVER, PANEL_LIGHT);
    colors.insert(tokens::SLIDER_BG_PRESSED, BUTTON_HOVER);
    colors.insert(tokens::SLIDER_BG_DISABLED, PANEL_DARK);
    colors.insert(tokens::SLIDER_BAR, ACCENT_DIM);
    colors.insert(tokens::SLIDER_BAR_HOVER, ACCENT);
    colors.insert(tokens::SLIDER_BAR_PRESSED, ACCENT);
    colors.insert(tokens::SLIDER_BAR_DISABLED, BORDER_BRIGHT);
    colors.insert(tokens::SLIDER_TEXT, TEXT);
    colors.insert(tokens::SLIDER_TEXT_DISABLED, TEXT_FAINT);

    colors.insert(tokens::SCROLLBAR_BG, PANEL_DARK);
    colors.insert(tokens::SCROLLBAR_THUMB, SPLITTER);
    colors.insert(tokens::SCROLLBAR_THUMB_HOVER, SPLITTER_HOVER);

    for token in [
        tokens::CHECKBOX_BG,
        tokens::CHECKBOX_BG_HOVER,
        tokens::CHECKBOX_BG_PRESSED,
        tokens::CHECKBOX_BORDER,
        tokens::CHECKBOX_BORDER_HOVER,
        tokens::CHECKBOX_BORDER_PRESSED,
    ] {
        colors.insert(token, BUTTON);
    }
    colors.insert(tokens::CHECKBOX_BG_DISABLED, PANEL_DARK);
    colors.insert(tokens::CHECKBOX_BORDER_DISABLED, BORDER);
    for token in [
        tokens::CHECKBOX_BG_CHECKED,
        tokens::CHECKBOX_BG_CHECKED_HOVER,
        tokens::CHECKBOX_BG_CHECKED_PRESSED,
        tokens::CHECKBOX_BORDER_CHECKED,
        tokens::CHECKBOX_BORDER_CHECKED_HOVER,
        tokens::CHECKBOX_BORDER_CHECKED_PRESSED,
    ] {
        colors.insert(token, ACCENT_DIM);
    }
    colors.insert(tokens::CHECKBOX_BG_CHECKED_DISABLED, PANEL_LIGHT);
    colors.insert(tokens::CHECKBOX_BORDER_CHECKED_DISABLED, BORDER_BRIGHT);
    colors.insert(tokens::CHECKBOX_MARK, TEXT);
    colors.insert(tokens::CHECKBOX_MARK_DISABLED, TEXT_FAINT);
    colors.insert(tokens::CHECKBOX_TEXT, TEXT);
    colors.insert(tokens::CHECKBOX_TEXT_DISABLED, TEXT_FAINT);

    for token in [
        tokens::RADIO_BORDER,
        tokens::RADIO_BORDER_HOVER,
        tokens::RADIO_BORDER_PRESSED,
    ] {
        colors.insert(token, BORDER_BRIGHT);
    }
    colors.insert(tokens::RADIO_BORDER_DISABLED, BORDER);
    for token in [
        tokens::RADIO_BORDER_CHECKED,
        tokens::RADIO_BORDER_CHECKED_HOVER,
        tokens::RADIO_BORDER_CHECKED_PRESSED,
        tokens::RADIO_MARK,
        tokens::RADIO_MARK_HOVER,
        tokens::RADIO_MARK_PRESSED,
    ] {
        colors.insert(token, ACCENT);
    }
    colors.insert(tokens::RADIO_BORDER_CHECKED_DISABLED, BORDER_BRIGHT);
    colors.insert(tokens::RADIO_MARK_DISABLED, TEXT_FAINT);
    colors.insert(tokens::RADIO_TEXT, TEXT);
    colors.insert(tokens::RADIO_TEXT_DISABLED, TEXT_FAINT);

    for token in [
        tokens::SWITCH_BG,
        tokens::SWITCH_BG_HOVER,
        tokens::SWITCH_BG_PRESSED,
        tokens::SWITCH_BORDER,
        tokens::SWITCH_BORDER_HOVER,
        tokens::SWITCH_BORDER_PRESSED,
    ] {
        colors.insert(token, BUTTON);
    }
    colors.insert(tokens::SWITCH_BG_DISABLED, PANEL_DARK);
    colors.insert(tokens::SWITCH_BORDER_DISABLED, BORDER);
    for token in [
        tokens::SWITCH_BG_CHECKED,
        tokens::SWITCH_BG_CHECKED_HOVER,
        tokens::SWITCH_BG_CHECKED_PRESSED,
        tokens::SWITCH_BORDER_CHECKED,
        tokens::SWITCH_BORDER_CHECKED_HOVER,
        tokens::SWITCH_BORDER_CHECKED_PRESSED,
    ] {
        colors.insert(token, ACCENT_DIM);
    }
    colors.insert(tokens::SWITCH_BG_CHECKED_DISABLED, PANEL_LIGHT);
    colors.insert(tokens::SWITCH_BORDER_CHECKED_DISABLED, BORDER_BRIGHT);
    for token in [
        tokens::SWITCH_SLIDE_BG,
        tokens::SWITCH_SLIDE_BG_HOVER,
        tokens::SWITCH_SLIDE_BG_PRESSED,
        tokens::SWITCH_SLIDE_BG_CHECKED,
        tokens::SWITCH_SLIDE_BG_CHECKED_HOVER,
        tokens::SWITCH_SLIDE_BG_CHECKED_PRESSED,
        tokens::SWITCH_SLIDE_BORDER,
        tokens::SWITCH_SLIDE_BORDER_HOVER,
        tokens::SWITCH_SLIDE_BORDER_PRESSED,
        tokens::SWITCH_SLIDE_BORDER_CHECKED,
        tokens::SWITCH_SLIDE_BORDER_CHECKED_HOVER,
        tokens::SWITCH_SLIDE_BORDER_CHECKED_PRESSED,
    ] {
        colors.insert(token, TEXT);
    }
    colors.insert(tokens::SWITCH_SLIDE_BG_DISABLED, TEXT_FAINT);
    colors.insert(tokens::SWITCH_SLIDE_BG_CHECKED_DISABLED, TEXT_FAINT);
    colors.insert(tokens::SWITCH_SLIDE_BORDER_DISABLED, TEXT_FAINT);
    colors.insert(tokens::SWITCH_SLIDE_BORDER_CHECKED_DISABLED, TEXT_FAINT);

    colors.insert(tokens::COLOR_PLANE_BG, PANEL_DARK);
    colors.insert(tokens::MENU_BG, MENU);
    colors.insert(tokens::MENU_BORDER, BORDER_BRIGHT);
    colors.insert(tokens::MENUITEM_BG_HOVER, PANEL_LIGHT);
    colors.insert(tokens::MENUITEM_BG_PRESSED, SELECTION);
    colors.insert(tokens::MENUITEM_BG_FOCUSED, SELECTION);
    colors.insert(tokens::MENUITEM_TEXT, TEXT);
    colors.insert(tokens::MENUITEM_TEXT_DISABLED, TEXT_FAINT);

    colors.insert(tokens::TEXT_INPUT_BG, PANEL);
    colors.insert(tokens::TEXT_INPUT_LABEL_BG, PANEL_LIGHT);
    colors.insert(tokens::TEXT_INPUT_TEXT, TEXT);
    colors.insert(tokens::TEXT_INPUT_TEXT_DISABLED, TEXT_FAINT);
    colors.insert(tokens::TEXT_INPUT_CURSOR, ACCENT);
    colors.insert(tokens::TEXT_INPUT_SELECTION, ACCENT_DIM);
    colors.insert(tokens::TEXT_INPUT_SELECTION_UNFOCUSED, SELECTION);

    colors.insert(tokens::PANE_HEADER_BG, PANEL_DARK);
    colors.insert(tokens::PANE_HEADER_BORDER, BORDER);
    colors.insert(tokens::PANE_HEADER_TEXT, TEXT);
    colors.insert(tokens::PANE_HEADER_DIVIDER, BORDER);
    colors.insert(tokens::PANE_BODY_BG, APP_BG);
    colors.insert(tokens::SUBPANE_HEADER_BG, PANEL_DARK);
    colors.insert(tokens::SUBPANE_HEADER_BORDER, BORDER);
    colors.insert(tokens::SUBPANE_HEADER_TEXT, TEXT);
    colors.insert(tokens::SUBPANE_BODY_BG, PANEL_DARK);
    colors.insert(tokens::SUBPANE_BODY_BORDER, BORDER);
    colors.insert(tokens::GROUP_HEADER_BG, PANEL_DARK);
    colors.insert(tokens::GROUP_HEADER_BORDER, BORDER);
    colors.insert(tokens::GROUP_HEADER_TEXT, TEXT);
    colors.insert(tokens::GROUP_BODY_BG, PANEL);
    colors.insert(tokens::GROUP_BODY_BORDER, BORDER);

    colors.insert(tokens::LISTROW_BG, Color::NONE);
    colors.insert(tokens::LISTROW_BG_HOVER, PANEL_LIGHT);
    colors.insert(tokens::LISTROW_BG_SELECTED, SELECTION);
    colors.insert(tokens::LISTROW_TEXT, TEXT);
    colors.insert(tokens::LISTROW_TEXT_DISABLED, TEXT_FAINT);

    theme
}
