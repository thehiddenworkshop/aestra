//! Aestra's editor widget layer built on Bevy Feathers.
//!
//! Bevy owns control behavior, focus, accessibility, and theme tokens. This module owns the
//! reusable editor-facing compositions so panels do not each invent their own spacing, variants,
//! activation bridge, or overflow behavior.

pub(crate) mod automation_curve;
pub(crate) mod breadcrumb;
pub(crate) mod button;
pub(crate) mod color_picker;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod field_row;
pub(crate) mod icon;
pub(crate) mod list_row;
pub(crate) mod number_input;
pub(crate) mod panel;
pub(crate) mod panel_card;
pub(crate) mod scenes;
pub(crate) mod scroll;
pub(crate) mod search_field;
pub(crate) mod separator;
pub(crate) mod slider_row;
pub(crate) mod status_bar;
pub(crate) mod text_input;
pub(crate) mod tooltip;

use crate::theme;
use bevy::{feathers::FeathersPlugins, prelude::*};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AestraFeathersSet {
    Input,
    Sync,
}

pub(crate) struct AestraFeathersPlugin;

impl Plugin for AestraFeathersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FeathersPlugins)
            .insert_resource(theme::feathers_theme())
            .init_resource::<automation_curve::AutomationCurveImageCache>()
            .init_resource::<tooltip::TooltipState>()
            .init_resource::<number_input::NumberScrubState>()
            .add_observer(button::queue_action_activation)
            .add_observer(color_picker::handle_color_plane_change)
            .add_observer(color_picker::handle_color_lightness_change)
            .add_observer(color_picker::handle_color_alpha_change)
            .add_observer(color_picker::handle_color_channel_change)
            .add_observer(color_picker::handle_color_hex_change)
            .add_observer(number_input::begin_number_scrub)
            .add_observer(number_input::update_number_scrub)
            .add_observer(number_input::finish_number_scrub)
            .add_observer(text_input::emit_text_change)
            .add_observer(text_input::submit_text_on_enter)
            .add_observer(text_input::submit_text_on_focus_loss)
            .add_observer(search_field::clear_search_field)
            .add_observer(tooltip::begin_tooltip)
            .add_observer(tooltip::dismiss_tooltip_on_drag)
            .add_systems(
                Update,
                button::audit_action_controls.in_set(AestraFeathersSet::Input),
            )
            .add_systems(
                Update,
                (
                    automation_curve::rasterize_automation_curves,
                    list_row::update_keyboard_list_focus_visuals,
                    color_picker::sync_color_picker_visuals,
                    number_input::decorate_scrubbable_numbers,
                    scroll::update_scrollbar_visibility,
                    search_field::sync_search_clear_visibility,
                    tooltip::update_tooltip,
                )
                    .in_set(AestraFeathersSet::Sync),
            );
    }
}
