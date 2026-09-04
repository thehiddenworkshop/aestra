use crate::menus::AboutDescription;
use bevy::prelude::*;
use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use std::collections::BTreeMap;
use unic_langid::LanguageIdentifier;

const FALLBACK_LOCALE: &str = "en-US";
const EN_US_SOURCE: &str = include_str!("../locales/en-US/editor.ftl");
const FR_FR_SOURCE: &str = include_str!("../locales/fr-FR/editor.ftl");

pub(crate) const SUPPORTED_LOCALES: [&str; 2] = ["en-US", "fr-FR"];

pub(crate) struct EditorLocalizationPlugin {
    locale: &'static str,
}

impl EditorLocalizationPlugin {
    pub(crate) fn new(requested_locale: &str) -> Self {
        Self {
            locale: resolve_locale(requested_locale),
        }
    }

    pub(crate) fn locale(&self) -> &'static str {
        self.locale
    }
}

impl Plugin for EditorLocalizationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(
            Localizer::new(self.locale).expect("embedded Fluent catalogs must be valid"),
        )
        .add_systems(Update, update_localized_text.in_set(LocalizationSet::Sync));
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LocalizationSet {
    Sync,
}

#[derive(Component)]
pub(crate) struct LocalizedText(pub(crate) &'static str);

#[cfg(test)]
const EDITOR_MESSAGE_IDS: &[&str] = &[
    "menu-file",
    "menu-edit",
    "menu-view",
    "menu-help",
    "file-new-effect",
    "file-open",
    "file-save",
    "file-save-as",
    "file-settings",
    "file-exit",
    "edit-undo",
    "edit-redo",
    "edit-add-emitter",
    "edit-duplicate-emitter",
    "edit-delete-emitter",
    "view-toggle-grid",
    "view-restart-preview",
    "view-panels",
    "view-reset-workspace",
    "help-about",
    "toolbar-play",
    "toolbar-pause",
    "toolbar-stop",
    "toolbar-loop-restart",
    "toolbar-loop-continuous",
    "toolbar-restart",
    "toolbar-save",
    "toolbar-source-back",
    "toolbar-source-forward",
    "toolbar-source-hidden-ancestors",
    "toolbar-runtime",
    "viewport-shape-radius",
    "viewport-shape-depth",
    "viewport-shape-extent-x",
    "viewport-shape-extent-y",
    "viewport-shape-extent-z",
    "panel-viewport",
    "panel-assets",
    "panel-properties",
    "panel-timeline",
    "panel-curves",
    "panel-diagnostics",
    "panel-compiler-inspector",
    "panel-material-graph",
    "panel-profiler",
    "panel-changes",
    "panel-settings",
    "material-graph-edit-hint",
    "material-graph-frame-all",
    "material-graph-frame-selection",
    "material-graph-collapse-node",
    "material-graph-expand-node",
    "material-graph-empty",
    "material-graph-empty-description",
    "material-graph-valid",
    "material-graph-invalid",
    "material-graph-unreachable",
    "material-graph-disabled",
    "material-graph-outputs",
    "material-graph-search-nodes",
    "material-graph-clear-search",
    "material-graph-no-compatible-nodes",
    "material-graph-duplicate-nodes",
    "material-graph-delete-nodes",
    "dock-float-panel",
    "dock-status-closed",
    "dock-status-showing",
    "dock-status-hidden",
    "dock-status-floated",
    "dock-status-workspace-reset",
    "dock-status-docked",
    "dock-status-moved-relative",
    "dock-status-moved-end",
    "dock-status-redocked-after-close",
    "dock-relation-before",
    "dock-relation-after",
    "persistence-file-filter-effect",
    "persistence-dialog-unsaved-title",
    "persistence-dialog-unsaved-description",
    "persistence-dialog-recovery-title",
    "persistence-dialog-recovery-description",
    "persistence-dialog-recovery-unsaved-source",
    "persistence-status-created-untitled",
    "persistence-status-new-cancelled",
    "persistence-status-opened",
    "persistence-status-open-cancelled",
    "persistence-status-open-failed",
    "persistence-status-saved",
    "persistence-status-save-cancelled",
    "persistence-status-save-failed",
    "persistence-status-exit-cancelled",
    "persistence-status-recovery-restored",
    "persistence-status-recovery-discarded",
    "persistence-status-recovery-discard-failed",
    "persistence-status-recovery-autosave-failed",
    "persistence-status-recovery-diagnostic",
    "persistence-status-settings-saved",
    "persistence-status-settings-save-failed",
    "persistence-status-settings-diagnostic",
    "persistence-status-settings-reset",
    "persistence-status-settings-reset-failed",
    "common-close",
    "common-on",
    "common-off",
    "common-reset-settings",
    "about-description",
    "compile-compiled",
    "compile-failed",
    "compile-preview-blocked",
    "compile-with-warnings",
    "settings-editor-settings",
    "settings-general",
    "settings-preview",
    "settings-performance",
    "settings-capture",
    "settings-appearance",
    "settings-language",
    "settings-keybindings",
    "settings-confirm-unsaved",
    "settings-confirm-unsaved-description",
    "settings-autosave-enabled",
    "settings-autosave-enabled-description",
    "settings-autosave-interval",
    "settings-autosave-interval-description",
    "settings-viewport-grid",
    "settings-viewport-grid-description",
    "settings-play-on-open",
    "settings-play-on-open-description",
    "settings-preview-particle-limit",
    "settings-preview-particle-limit-description",
    "settings-capture-frame-rate",
    "settings-capture-frame-rate-description",
    "settings-contact-sheet-columns",
    "settings-contact-sheet-columns-description",
    "settings-interface-scale",
    "settings-interface-scale-description",
    "settings-editor-language",
    "settings-language-description",
    "settings-keybinding-description",
    "settings-binding-play-pause",
    "settings-binding-restart",
    "settings-binding-save",
    "settings-binding-undo",
    "settings-binding-redo",
    "settings-binding-add-emitter",
    "locale-en-us",
    "locale-fr-fr",
    "properties-input-spawn-rate",
    "properties-input-spawn-rate-description",
    "properties-input-burst-count",
    "properties-input-burst-count-description",
    "properties-input-shape",
    "properties-input-shape-description",
    "properties-input-lifetime",
    "properties-input-lifetime-description",
    "properties-input-speed",
    "properties-input-speed-description",
    "properties-input-direction",
    "properties-input-direction-description",
    "properties-input-spread",
    "properties-input-spread-description",
    "properties-input-angular-velocity",
    "properties-input-angular-velocity-description",
    "properties-input-gravity",
    "properties-input-gravity-description",
    "properties-input-drag",
    "properties-input-drag-description",
    "properties-input-turbulence",
    "properties-input-turbulence-description",
    "properties-input-size",
    "properties-input-size-description",
    "properties-input-opacity",
    "properties-input-opacity-description",
    "properties-input-color",
    "properties-input-color-description",
    "properties-effect",
    "properties-effect-name",
    "properties-effect-name-description",
    "properties-effect-name-status-target",
    "properties-emitter",
    "properties-emitter-name",
    "properties-emitter-name-description",
    "properties-emitter-name-status-target",
    "properties-emitter-enabled",
    "properties-emitter-enabled-description",
    "properties-emitter-capacity",
    "properties-emitter-capacity-description",
    "properties-edit-source",
    "properties-explode-effect-clip",
    "properties-events",
    "properties-events-empty",
    "properties-events-add",
    "properties-events-add-description",
    "properties-events-no-targets",
    "properties-event-link",
    "properties-event-on-spawn",
    "properties-event-on-death",
    "properties-event-on-collision",
    "properties-status-selected-compiled",
    "properties-status-selected",
    "properties-status-module-registry-unavailable",
    "properties-status-module-missing",
    "properties-status-input-metadata-unavailable",
    "properties-status-not-choice",
    "properties-status-choice-unavailable",
    "properties-status-target-unavailable",
    "properties-status-finite-number-required",
    "properties-status-incompatible-metadata",
    "properties-status-updated",
    "properties-status-name-required",
    "properties-status-event-added",
    "properties-status-event-removed",
    "properties-status-event-duplicate",
    "properties-status-event-self-target",
    "properties-status-event-target-missing",
    "compiler-status-pending-target",
    "assets-current-effect",
    "assets-modified",
    "assets-saved",
    "assets-project-effects",
    "assets-found",
    "assets-render-assets",
    "assets-registered",
    "assets-no-render-assets",
    "assets-materials",
    "assets-add-sprite-material",
    "assets-sprite",
    "assets-flipbooks",
    "assets-add-grid-flipbook",
    "assets-flipbook-summary",
    "assets-layers",
    "assets-active",
    "assets-add-emitter",
    "assets-status-minimum-emitter",
    "assets-change-delete-emitter",
    "library-more-effect-actions",
    "library-rename-effect",
    "library-move-effect",
    "library-inspect-dependencies",
    "library-delete-effect",
    "library-dependencies-title",
    "library-dependencies-description",
    "library-dependencies-uses",
    "library-dependencies-used-by",
    "library-dependencies-none",
    "library-dependencies-direct",
    "library-dependencies-indirect",
    "library-dependencies-clip",
    "library-delete-title",
    "library-delete-unreferenced-warning",
    "library-delete-referenced-warning",
    "library-delete-usages-changed",
    "library-delete-confirm",
    "library-rename-dialog-title",
    "library-rename-dialog-description",
    "library-rename-input",
    "library-rename-confirm",
    "library-extract-dialog-title",
    "library-extract-dialog-description",
    "library-extract-input",
    "library-extract-replace-selection",
    "library-extract-confirm",
    "library-extract-default-name",
    "library-extract-no-selection",
    "library-extract-command",
    "library-extract-created",
    "library-explode-command",
    "library-explode-created",
    "library-explode-clip-missing",
    "library-move-dialog-title",
    "library-status-source-missing",
    "library-status-source-unresolvable",
    "library-status-switch-before-delete",
    "library-status-save-before-rename",
    "library-status-effect-renamed",
    "library-status-effect-moved",
    "library-status-effect-deleted",
    "library-status-operation-failed",
    "library-status-catalog-refreshed",
    "library-status-source-reloaded",
    "library-status-source-reload-failed",
    "library-status-source-conflict",
    "library-status-source-moved-dirty",
    "library-status-open-source-missing",
    "timeline-frame-all",
    "timeline-add-marker",
    "timeline-add-event",
    "timeline-events-lane",
    "timeline-event",
    "timeline-marker",
    "timeline-add-marker-command",
    "timeline-delete-marker-command",
    "timeline-delete-event-command",
    "timeline-move-marker-command",
    "properties-start-mode",
    "properties-start-mode-absolute",
    "properties-start-mode-marker",
    "properties-start-mode-description",
    "properties-start-marker",
    "properties-start-marker-description",
    "properties-start-offset",
    "properties-start-offset-description",
    "properties-start-reference-command",
    "properties-choreography-event",
    "properties-choreography-event-name-description",
    "properties-choreography-event-time-description",
    "properties-choreography-event-type",
    "properties-choreography-event-type-description",
    "properties-choreography-event-payload-description",
    "properties-choreography-event-delete",
    "properties-event-kind-gameplay-notify",
    "properties-event-kind-play-sound",
    "properties-event-kind-camera-shake",
    "properties-event-kind-spawn-child-effect",
    "properties-event-topic",
    "properties-event-cue",
    "properties-event-effect",
    "properties-event-intensity",
    "properties-event-intensity-description",
    "timeline-emitters",
    "timeline-select-emitter",
    "timeline-enable-emitter",
    "timeline-disable-emitter",
    "timeline-more-emitter-actions",
    "timeline-menu-mute",
    "timeline-menu-unmute",
    "timeline-menu-solo",
    "timeline-menu-unsolo",
    "timeline-menu-duplicate",
    "timeline-menu-delete",
    "timeline-menu-edit-source",
    "timeline-menu-explode-effect-clip",
    "timeline-menu-create-reusable-effect",
    "timeline-reorder-emitter",
    "timeline-reordered-emitter",
    "timeline-reorder-effect-clip",
    "timeline-reordered-effect-clip",
    "timeline-emitter-diagnostic",
    "timeline-expand-automation",
    "timeline-collapse-automation",
    "timeline-automation-visibility",
    "timeline-show-all-automation",
    "timeline-hide-all-automation",
    "timeline-show-automation-lane",
    "timeline-hide-automation-lane",
    "timeline-add-automation-key",
    "timeline-delete-automation-key",
    "timeline-resize-automation-lane",
    "timeline-selected-automation-key",
    "timeline-add-automation-key-command",
    "timeline-delete-automation-key-command",
    "timeline-move-automation-key-command",
    "timeline-automation-keep-two-keys",
    "timeline-snap-off",
    "timeline-snap-frames",
    "timeline-snap-time",
    "timeline-snap-smart",
    "timeline-snapping-description",
    "timeline-hertz",
    "timeline-duration",
    "curves-header-hint",
    "curves-choose-property",
    "curves-property-missing",
    "curves-add-key",
    "curves-delete-key",
    "curves-time",
    "curves-value",
    "curves-step",
    "generated-compiled-plan",
    "generated-status-last-valid",
    "generated-status-pending",
    "generated-status-live",
    "generated-status-unavailable",
    "generated-no-artifact",
    "generated-no-artifact-description",
    "generated-emitters",
    "generated-ops",
    "generated-attributes",
    "generated-parameters",
    "generated-capacity",
    "generated-particle-layout",
    "generated-stored",
    "generated-transient",
    "generated-optimized",
    "generated-optimization-summary",
    "generated-parameter-table",
    "generated-no-runtime-parameters",
    "generated-enabled",
    "generated-disabled",
    "generated-renderers",
    "generated-sprite-draw",
    "generated-stage-emitter-update",
    "generated-stage-particle-spawn",
    "generated-stage-particle-update",
    "generated-wesl-backend",
    "generated-simulation",
    "generated-wesl-description",
    "changes-none-pending",
    "changes-summary",
    "changes-empty-description",
    "changes-ready",
    "changes-blocked",
    "changes-discard",
    "changes-apply",
    "changes-apply-blocked",
    "changes-kind-added",
    "changes-kind-removed",
    "changes-kind-modified",
    "changes-kind-moved",
    "changes-target-preview-only",
    "changes-selected-target",
    "diagnostics-validation",
    "diagnostics-filter-all",
    "diagnostics-errors",
    "diagnostics-warnings",
    "diagnostics-info",
    "diagnostics-no-issues",
    "diagnostics-no-issues-description",
    "diagnostics-no-matches",
    "diagnostics-no-matches-description",
    "diagnostics-working-effect",
    "diagnostics-pending-transaction",
    "diagnostics-severity-error",
    "diagnostics-severity-warning",
    "diagnostics-severity-info",
    "diagnostics-code-unsupported-format",
    "diagnostics-code-nil-id",
    "diagnostics-code-duplicate-id",
    "diagnostics-code-invalid-duration",
    "diagnostics-code-invalid-timing",
    "diagnostics-code-invalid-capacity",
    "diagnostics-code-missing-module",
    "diagnostics-code-duplicate-module",
    "diagnostics-code-stage-mismatch",
    "diagnostics-code-invalid-value",
    "diagnostics-code-missing-renderer",
    "diagnostics-code-invalid-reference",
    "diagnostics-code-unknown-module",
    "diagnostics-code-unsupported-renderer",
    "diagnostics-code-missing-attribute",
    "diagnostics-code-unknown-parameter",
    "diagnostics-code-parameter-type-mismatch",
    "diagnostics-code-material-type-mismatch",
    "diagnostics-code-unsupported-material-domain",
    "diagnostics-code-unsupported-material-input",
    "diagnostics-code-evaluation-domain-mismatch",
    "diagnostics-code-missing-resource-declaration",
    "diagnostics-code-invalid-render-state",
    "diagnostics-code-unreachable-expression",
    "profiler-effect-profile",
    "profiler-status-last-valid",
    "profiler-status-live",
    "profiler-reset-peaks",
    "profiler-reset-status",
    "profiler-waiting",
    "profiler-waiting-description",
    "profiler-metric-cpu-update",
    "profiler-metric-gpu-time",
    "profiler-metric-live-particles",
    "profiler-metric-submitted-instances",
    "profiler-metric-peak-particles",
    "profiler-metric-capacity",
    "profiler-metric-emitters",
    "profiler-metric-draw-calls",
    "profiler-metric-dispatches",
    "profiler-metric-buffer-memory",
    "profiler-source-measured",
    "profiler-source-estimated",
    "profiler-source-unavailable",
    "profiler-cpu-history",
    "profiler-history-collecting",
    "profiler-history-summary",
    "profiler-emitter-summary",
    "profiler-emitters",
    "profiler-measurement-availability",
    "profiler-measured-description",
    "profiler-estimated-description",
    "profiler-unavailable-description",
];

#[derive(Resource)]
pub(crate) struct Localizer {
    bundles: BTreeMap<&'static str, FluentBundle<FluentResource>>,
    locale: &'static str,
}

impl Localizer {
    pub(crate) fn new(requested_locale: &str) -> Result<Self, String> {
        let mut bundles = BTreeMap::new();
        bundles.insert(
            FALLBACK_LOCALE,
            build_bundle(FALLBACK_LOCALE, EN_US_SOURCE)?,
        );
        bundles.insert("fr-FR", build_bundle("fr-FR", FR_FR_SOURCE)?);
        let locale = resolve_locale(requested_locale);
        Ok(Self { bundles, locale })
    }

    pub(crate) fn locale(&self) -> &'static str {
        self.locale
    }

    pub(crate) fn set_locale(&mut self, requested_locale: &str) -> bool {
        let locale = resolve_locale(requested_locale);
        if locale == self.locale {
            return false;
        }
        self.locale = locale;
        true
    }

    pub(crate) fn text(&self, id: &str) -> String {
        self.format(id, None)
    }

    pub(crate) fn text_with(&self, id: &str, args: &FluentArgs<'_>) -> String {
        self.format(id, Some(args))
    }

    pub(crate) fn locale_name(&self, locale: &str) -> String {
        self.text(match resolve_locale(locale) {
            "fr-FR" => "locale-fr-fr",
            _ => "locale-en-us",
        })
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        self.format_from(self.locale, id, args)
            .or_else(|| self.format_from(FALLBACK_LOCALE, id, args))
            .unwrap_or_else(|| format!("[{id}]"))
    }

    fn format_from(&self, locale: &str, id: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
        let bundle = self.bundles.get(locale)?;
        let pattern = bundle.get_message(id)?.value()?;
        let mut errors = Vec::new();
        let value = bundle.format_pattern(pattern, args, &mut errors);
        errors.is_empty().then(|| value.into_owned())
    }

    #[cfg(test)]
    fn missing_messages(&self, locale: &str) -> Vec<&'static str> {
        let Some(bundle) = self.bundles.get(locale) else {
            return EDITOR_MESSAGE_IDS.to_vec();
        };
        EDITOR_MESSAGE_IDS
            .iter()
            .copied()
            .filter(|id| !bundle.has_message(id))
            .collect()
    }
}

fn update_localized_text(
    localizer: Res<Localizer>,
    mut labels: Query<(&LocalizedText, &mut Text), Without<AboutDescription>>,
    mut about: Query<&mut Text, (With<AboutDescription>, Without<LocalizedText>)>,
) {
    if !localizer.is_changed() {
        return;
    }
    for (message, mut text) in &mut labels {
        text.0 = localizer.text(message.0);
    }
    let mut args = FluentArgs::new();
    args.set("version", env!("CARGO_PKG_VERSION"));
    for mut text in &mut about {
        text.0 = localizer.text_with("about-description", &args);
    }
}

fn build_bundle(
    locale: &'static str,
    source: &'static str,
) -> Result<FluentBundle<FluentResource>, String> {
    let language: LanguageIdentifier = locale
        .parse()
        .map_err(|error| format!("invalid locale {locale}: {error}"))?;
    let resource = FluentResource::try_new(source.to_string())
        .map_err(|(_, errors)| format!("invalid {locale} Fluent catalog: {errors:?}"))?;
    let mut bundle = FluentBundle::new_concurrent(vec![language]);
    bundle
        .add_resource(resource)
        .map_err(|errors| format!("invalid {locale} Fluent bundle: {errors:?}"))?;
    Ok(bundle)
}

fn resolve_locale(requested: &str) -> &'static str {
    if let Some(locale) = SUPPORTED_LOCALES
        .iter()
        .find(|locale| locale.eq_ignore_ascii_case(requested))
    {
        return locale;
    }
    let language = requested.split(['-', '_']).next().unwrap_or_default();
    SUPPORTED_LOCALES
        .iter()
        .find(|locale| {
            locale
                .split('-')
                .next()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(language))
        })
        .copied()
        .unwrap_or(FALLBACK_LOCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_editor_message_exists_in_every_catalog() {
        let localizer = Localizer::new("en-US").unwrap();
        for locale in SUPPORTED_LOCALES {
            assert_eq!(localizer.missing_messages(locale), Vec::<&str>::new());
        }
    }

    #[test]
    fn locale_resolution_supports_region_language_and_fallback() {
        assert_eq!(resolve_locale("fr-FR"), "fr-FR");
        assert_eq!(resolve_locale("fr_CA"), "fr-FR");
        assert_eq!(resolve_locale("unknown"), "en-US");
    }

    #[test]
    fn switching_locale_changes_messages_live() {
        let mut localizer = Localizer::new("en-US").unwrap();
        assert_eq!(localizer.text("menu-file"), "File");
        assert!(localizer.set_locale("fr"));
        assert_eq!(localizer.text("menu-file"), "Fichier");
    }

    #[test]
    fn fluent_arguments_are_interpolated() {
        let localizer = Localizer::new("en-US").unwrap();
        let mut args = FluentArgs::new();
        args.set("version", "9.8.7");
        assert!(
            localizer
                .text_with("about-description", &args)
                .contains("9.8.7")
        );
    }

    #[test]
    fn malformed_catalogs_are_rejected() {
        assert!(build_bundle("en-US", "broken = { $missing").is_err());
    }

    #[test]
    fn missing_selected_messages_fall_back_to_english() {
        let mut bundles = BTreeMap::new();
        bundles.insert("en-US", build_bundle("en-US", EN_US_SOURCE).unwrap());
        bundles.insert(
            "fr-FR",
            build_bundle("fr-FR", "menu-edit = Édition").unwrap(),
        );
        let localizer = Localizer {
            bundles,
            locale: "fr-FR",
        };
        assert_eq!(localizer.text("menu-file"), "File");
    }

    #[test]
    fn localized_text_updates_when_the_locale_changes() {
        let mut app = App::new();
        app.add_plugins(EditorLocalizationPlugin::new("en-US"));
        let label = app
            .world_mut()
            .spawn((LocalizedText("menu-file"), Text::new("stale")))
            .id();
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "File");

        app.world_mut()
            .resource_mut::<Localizer>()
            .set_locale("fr-FR");
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "Fichier");
    }

    #[test]
    fn deep_workspace_messages_and_arguments_are_localized() {
        let localizer = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localizer.text("diagnostics-code-invalid-value"),
            "Valeur non valide"
        );
        assert_eq!(
            localizer.text("profiler-metric-live-particles"),
            "PARTICULES ACTIVES"
        );
        assert_eq!(localizer.text("assets-current-effect"), "EFFET COURANT");
        assert_eq!(
            localizer.text("timeline-snap-smart"),
            "Magnétisme : intelligent"
        );
        assert_eq!(localizer.text("curves-value"), "Valeur");
        assert_eq!(localizer.text("generated-compiled-plan"), "PLAN COMPILÉ");
        assert_eq!(localizer.text("changes-discard"), "Ignorer");

        let mut args = FluentArgs::new();
        args.set("count", 12_u32);
        args.set("average", "1,2 ms");
        args.set("maximum", "2,4 ms");
        let summary = localizer.text_with("profiler-history-summary", &args);
        assert!(summary.contains("12"));
        assert!(summary.contains("IMAGES"));
        assert!(summary.contains("MOY."));
        assert!(summary.contains("1,2 ms"));

        let mut args = FluentArgs::new();
        args.set("transaction", "SUPPRESSION");
        args.set("count", 3_u32);
        let summary = localizer.text_with("changes-summary", &args);
        assert!(summary.contains("SUPPRESSION"));
        assert!(summary.contains('3'));
        assert!(summary.contains("MODIFICATIONS"));
    }
}
