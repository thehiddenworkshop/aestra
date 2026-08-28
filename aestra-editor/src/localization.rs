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
    "toolbar-restart",
    "toolbar-save",
    "toolbar-choreography",
    "toolbar-runtime",
    "viewport-shape-radius",
    "viewport-shape-depth",
    "viewport-shape-extent-x",
    "viewport-shape-extent-y",
    "viewport-shape-extent-z",
    "panel-viewport",
    "panel-assets",
    "panel-inspector",
    "panel-timeline",
    "panel-curves",
    "panel-diagnostics",
    "panel-generated-code",
    "panel-profiler",
    "panel-changes",
    "panel-settings",
    "dock-float-panel",
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
    "inspector-input-spawn-rate",
    "inspector-input-spawn-rate-description",
    "inspector-input-burst-count",
    "inspector-input-burst-count-description",
    "inspector-input-shape",
    "inspector-input-shape-description",
    "inspector-input-lifetime",
    "inspector-input-lifetime-description",
    "inspector-input-speed",
    "inspector-input-speed-description",
    "inspector-input-direction",
    "inspector-input-direction-description",
    "inspector-input-spread",
    "inspector-input-spread-description",
    "inspector-input-angular-velocity",
    "inspector-input-angular-velocity-description",
    "inspector-input-gravity",
    "inspector-input-gravity-description",
    "inspector-input-drag",
    "inspector-input-drag-description",
    "inspector-input-turbulence",
    "inspector-input-turbulence-description",
    "inspector-input-size-over-life",
    "inspector-input-size-over-life-description",
    "inspector-input-opacity-over-life",
    "inspector-input-opacity-over-life-description",
    "inspector-input-color-over-life",
    "inspector-input-color-over-life-description",
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

        let mut args = FluentArgs::new();
        args.set("count", 12_u32);
        args.set("average", "1,2 ms");
        args.set("maximum", "2,4 ms");
        let summary = localizer.text_with("profiler-history-summary", &args);
        assert!(summary.contains("12"));
        assert!(summary.contains("IMAGES"));
        assert!(summary.contains("MOY."));
        assert!(summary.contains("1,2 ms"));
    }
}
