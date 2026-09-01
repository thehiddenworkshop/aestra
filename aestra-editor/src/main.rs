// Bevy ECS systems express disjoint resources and queries in their signatures. Keeping those
// dependencies explicit is clearer than hiding them behind editor-specific parameter bundles.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod changes;
mod compiler_inspector;
mod curves;
mod diagnostics;
mod dock_ui;
mod docking;
mod feathers;
mod history;
mod library;
mod localization;
mod menus;
mod persistence;
mod profiler;
mod properties;
mod recovery;
mod session;
mod settings;
mod settings_ui;
mod shell;
#[cfg(test)]
mod test_support;
mod theme;
mod timeline;
mod transport;
mod viewport;

use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy_render::AestraRenderPlugin;
use aestra_compiler::ModuleMetadata;
use aestra_core::{
    AssetKind, BlendMode, DiagnosticCode, DiagnosticSeverity, EffectAsset, EffectPlaybackMode,
    EmitterId, EmitterShape, EmitterTransform, EventId, EventTrigger, FlipbookPlaybackMode,
    FlipbookTimeSource, MaterialInput, MaterialProperties, ModuleId, ModuleInstance, RendererId,
    RendererProperties, StageKind, Value,
};
pub(crate) use aestra_project::{EffectAssetRef, ProjectSourceId as ProjectEffectEntryId};
#[cfg(test)]
use bevy::ui_widgets::Activate;
use bevy::{
    asset::AssetPlugin,
    camera::{RenderTarget, visibility::RenderLayers},
    feathers::{
        constants::fonts,
        containers::{group, group_body, group_header, pane_header},
        controls::{NumberInputValue, UpdateNumberInput},
        cursor::{EntityCursor, OverrideCursor},
        display::{label, label_dim},
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemedText},
        tokens,
    },
    gizmos::transform_gizmo::TransformGizmoState,
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::tab_navigation::TabIndex,
    picking::events::{Click, Drag, DragDrop, DragEnd, DragStart, Out, Over, Pointer},
    picking::pointer::PointerButton,
    prelude::*,
    text::{EditableText, FontSource, TextEdit},
    ui::{Checked, InteractionDisabled, Pressed, RelativeCursorPosition},
    ui_widgets::{ListBox, ListItem, ScrollIntoView, ValueChange},
    window::{
        CursorIcon, CursorOptions, PrimaryWindow, SystemCursorIcon, WindowCloseRequested,
        WindowMoved, WindowRef, WindowResizeConstraints, WindowResized, WindowResolution,
    },
};
use bevy_resvg::prelude::SvgPlugin;
use bevy_winit::WINIT_WINDOWS;
pub(crate) use changes::spawn_changes_workspace;
use changes::{ChangesSet, EditorChangesPlugin};
pub(crate) use compiler_inspector::spawn_compiler_inspector_workspace;
use compiler_inspector::{CompilerInspectorSet, EditorCompilerInspectorPlugin};
pub(crate) use curves::{CurvesAction, CurvesState, spawn_curves_workspace};
use curves::{CurvesSet, EditorCurvesPlugin};
pub(crate) use diagnostics::{DiagnosticsPanelState, spawn_diagnostics_workspace};
use diagnostics::{DiagnosticsSet, EditorDiagnosticsPlugin, spawn_compile_status};
#[cfg(test)]
use dock_ui::{clear_finished_dock_drag, dock_pane_background};
#[cfg(test)]
use docking::DockDragState;
#[cfg(test)]
use docking::DockTab;
use docking::{
    DockPanel, DockTreeHost, DockingPlugin, DockingSet, NativeFloatingWindow, WorkspaceLayout,
};
#[cfg(test)]
use feathers::button::queue_action_activation as queue_feathers_action_activation;
pub(crate) use feathers::scenes as ui_shell;
#[cfg(test)]
use feathers::scroll::scrollbar_needed;
use feathers::{
    AestraFeathersPlugin, AestraFeathersSet,
    button::{
        EditorNativeControl, FeathersActionButton, PendingFeathersActivation,
        spawn_action_button as spawn_feathers_action_button, spawn_tool_button as mini_button,
    },
    combo_box::{ComboOption, spawn_action_menu, spawn_combo_control, spawn_icon_action_menu},
    list_row::{
        KeyboardNavigableList, KeyboardNavigableListRow, ListRowStatus, spawn_action_list_row,
        spawn_info_list_row, spawn_list_empty_state, spawn_list_section_header,
        spawn_status_list_row,
    },
    panel::spawn_panel_heading as panel_heading,
    scroll::{PersistedScroll, spawn_vertical_scroll_area},
    search_field::spawn_search_field,
    text_input::spawn_text_input,
    tooltip::EditorTooltip,
};
use fluent_bundle::FluentArgs;
pub(crate) use history::HistoryAction;
use history::{EditorHistoryPlugin, HistorySet};
use library::{EditorLibraryPlugin, LibrarySet};
pub(crate) use library::{
    LibraryAssetOperationState, LibraryState, ProjectEffectCatalog, spawn_library,
    spawn_library_asset_operation_overlay,
};
use localization::{EditorLocalizationPlugin, LocalizationSet};
pub(crate) use localization::{LocalizedText, Localizer};
pub(crate) use menus::{DocumentMenuLabel, MenuState, TabContextMenu};
use menus::{EditorMenusPlugin, spawn_about_overlay, spawn_menu_bar, spawn_tab_context_menu};
pub(crate) use persistence::persist_editor_settings;
use persistence::{
    DocumentAction, DocumentProtectionState, EditorPersistencePlugin, PersistenceSet,
    SourceNavigationState, spawn_document_protection_overlay,
};
use profiler::{EditorProfilerPlugin, ProfilerSet};
pub(crate) use profiler::{ProfilerState, spawn_profiler_workspace};
use properties::*;
use session::EditorSession;
use settings::{EditorSettings, SettingsPersistence};
use settings_ui::EditorSettingsUiPlugin;
pub(crate) use settings_ui::{SettingsPanelState, spawn_settings_workspace};
pub(crate) use shell::*;
pub(crate) use timeline::ChoreographyAction;
use timeline::{TimelinePlugin, TimelineSet};
pub(crate) use transport::TransportAction;
use transport::{EditorTransportPlugin, TransportSet, spawn_transport_controls};
use viewport::{
    EmitterTransformGizmoInteraction, EmitterTransformGizmoProxy, ViewportAction, ViewportPlugin,
    ViewportSet, emitter_transform_from_bevy,
};

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const EDITOR_ASSET_ROOT: &str = "../assets";
const EDITOR_ICON: &[u8] = include_bytes!("../../assets/project/icon.png");

fn set_editor_window_icon(world: &mut World) {
    let Some(window_entity) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    else {
        warn!("Aestra window icon could not be applied: primary window is missing");
        return;
    };
    let Ok(image) = image::load_from_memory(EDITOR_ICON) else {
        warn!("Aestra window icon could not be decoded");
        return;
    };
    let image = image.into_rgba8();
    let (width, height) = image.dimensions();
    let Ok(icon) = winit::window::Icon::from_rgba(image.into_raw(), width, height) else {
        warn!("Aestra window icon has invalid RGBA dimensions");
        return;
    };
    WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(window_entity) else {
            warn!("Aestra window icon could not be applied: native window is missing");
            return;
        };
        window.set_window_icon(Some(icon));
    });
}

fn main() {
    let (mut settings, persistence) = SettingsPersistence::load();
    let localization = EditorLocalizationPlugin::new(&settings.language.locale);
    settings.language.locale = localization.locale().into();
    let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
    let show_grid = settings.preview.show_grid;
    let ui_scale = settings.appearance.ui_scale;
    App::new()
        .insert_resource(ClearColor(theme::APP_BG))
        .insert_resource(session)
        .insert_resource(settings)
        .insert_resource(persistence)
        .insert_resource(UiScale(ui_scale))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: EDITOR_ASSET_ROOT.into(),
                    ..default()
                })
                .set(WindowPlugin {
                    close_when_requested: false,
                    primary_window: Some(Window {
                        title: "Aestra — VFX Choreography Editor".into(),
                        resolution: WindowResolution::new(1440, 900),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(SvgPlugin)
        .add_plugins(AestraFeathersPlugin)
        .add_plugins(localization)
        .add_plugins(EditorMenusPlugin::new(show_grid))
        .add_plugins(EditorLibraryPlugin)
        .add_plugins(EditorChangesPlugin)
        .add_plugins(EditorCompilerInspectorPlugin)
        .add_plugins(EditorCurvesPlugin)
        .add_plugins(EditorDiagnosticsPlugin)
        .add_plugins(EditorHistoryPlugin)
        .add_plugins(EditorProfilerPlugin)
        .add_plugins(EditorSettingsUiPlugin)
        .add_plugins(EditorShellPlugin)
        .add_plugins(EditorPersistencePlugin)
        .add_plugins(AestraRenderPlugin)
        .add_plugins(DockingPlugin)
        .add_plugins(PropertiesPlugin)
        .add_plugins(TimelinePlugin)
        .add_plugins(EditorTransportPlugin)
        .add_plugins(ViewportPlugin)
        .add_systems(Startup, set_editor_window_icon)
        .configure_sets(
            Startup,
            (
                PersistenceSet::Startup,
                ViewportSet::Setup,
                EditorSet::Setup,
            )
                .chain(),
        )
        .configure_sets(
            Update,
            (
                (
                    LibrarySet::Input,
                    TransportSet::Input,
                    HistorySet::Input,
                    ViewportSet::Input,
                )
                    .chain(),
                TimelineSet::Input,
                PropertiesSet::Input,
                DockingSet::Input,
                AestraFeathersSet::Input,
                (
                    LibrarySet::Actions,
                    ChangesSet::Actions,
                    CompilerInspectorSet::Actions,
                    CurvesSet::Actions,
                    DiagnosticsSet::Actions,
                    DockingSet::Actions,
                    HistorySet::Actions,
                    PropertiesSet::Actions,
                    ProfilerSet::Actions,
                    PersistenceSet::Actions,
                    TimelineSet::Actions,
                    TransportSet::Actions,
                    ViewportSet::Actions,
                )
                    .chain(),
                EditorSet::PreViewport,
                TransportSet::Playback,
                PersistenceSet::Lifecycle,
                ViewportSet::Update,
                LocalizationSet::Sync,
                EditorSet::MainUpdate,
                DockingSet::Reconcile,
                EditorSet::UiRebuild,
                TimelineSet::Visuals,
                (
                    LibrarySet::Sync,
                    DiagnosticsSet::Sync,
                    HistorySet::Sync,
                    ProfilerSet::Sync,
                    PropertiesSet::Sync,
                )
                    .chain(),
                DockingSet::Sync,
                AestraFeathersSet::Sync,
                TransportSet::Sync,
                EditorSet::UiSync,
            )
                .chain(),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_dock_is_a_transparent_cutout_for_the_preview_camera() {
        assert_eq!(dock_pane_background(Some(DockPanel::Viewport)), Color::NONE);
        assert_eq!(
            dock_pane_background(Some(DockPanel::Properties)),
            theme::PANEL_DARK
        );
        assert_eq!(dock_pane_background(None), theme::PANEL_DARK);
    }

    #[test]
    fn dock_drag_state_clears_even_if_the_dragged_tab_was_rebuilt() {
        let mut app = App::new();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Left);
        app.insert_resource(buttons);
        app.insert_resource(DockDragState(Some(DockPanel::Properties)));
        app.add_systems(Update, clear_finished_dock_drag);
        let tab = app
            .world_mut()
            .spawn((
                DockTab(DockPanel::Properties),
                UiTransform {
                    translation: Val2::px(20.0, 10.0),
                    ..default()
                },
                GlobalZIndex(160),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().resource::<DockDragState>().0,
            Some(DockPanel::Properties)
        );

        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();

        assert_eq!(app.world().resource::<DockDragState>().0, None);
        assert_eq!(
            app.world().get::<UiTransform>(tab).unwrap().translation,
            Val2::ZERO
        );
        assert!(!app.world().entity(tab).contains::<GlobalZIndex>());
    }

    #[test]
    fn bundled_effect_is_valid() {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE).expect("bundled effect should parse");
        assert_eq!(effect.format_version, 3);
        assert_eq!(effect.emitters.len(), 4);
    }

    #[test]
    fn editor_asset_root_contains_bundled_textures() {
        let asset_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(EDITOR_ASSET_ROOT);
        for source in [
            include_str!("../../assets/effects/ember_sigil.aestra.ron"),
            include_str!("../../assets/effects/plasma_burst.aestra.ron"),
        ] {
            let effect = EffectAsset::from_ron(source).unwrap();
            for asset in effect.assets {
                assert!(
                    asset_root.join(&asset.path).is_file(),
                    "missing bundled asset {}",
                    asset.path
                );
            }
        }
        for icon in [
            "play.svg",
            "pause.svg",
            "stop.svg",
            "loop.svg",
            "move.svg",
            "rotate.svg",
            "scale.svg",
            "center-focus.svg",
            "solid.svg",
            "wireframe.svg",
        ] {
            assert!(
                asset_root.join("icons").join(icon).is_file(),
                "missing bundled transport icon {icon}"
            );
        }
    }

    #[test]
    fn bundled_window_icon_is_valid_rgba() {
        let icon = image::load_from_memory(EDITOR_ICON)
            .expect("bundled window icon should decode")
            .into_rgba8();
        assert!(icon.width() > 0);
        assert!(icon.height() > 0);
        assert_eq!(
            icon.as_raw().len(),
            (icon.width() * icon.height() * 4) as usize
        );
    }

    #[test]
    fn scrollbar_only_appears_for_overflowing_content() {
        assert!(!scrollbar_needed(320.0, 320.0));
        assert!(!scrollbar_needed(320.0, 320.4));
        assert!(scrollbar_needed(320.0, 321.0));
    }
}
