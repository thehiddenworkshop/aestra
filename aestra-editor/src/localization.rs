use bevy::prelude::Resource;
use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use std::collections::BTreeMap;
use unic_langid::LanguageIdentifier;

const FALLBACK_LOCALE: &str = "en-US";
const EN_US_SOURCE: &str = include_str!("../locales/en-US/editor.ftl");
const FR_FR_SOURCE: &str = include_str!("../locales/fr-FR/editor.ftl");

pub(crate) const SUPPORTED_LOCALES: [&str; 2] = ["en-US", "fr-FR"];

#[cfg(test)]
const SHELL_MESSAGE_IDS: &[&str] = &[
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
            return SHELL_MESSAGE_IDS.to_vec();
        };
        SHELL_MESSAGE_IDS
            .iter()
            .copied()
            .filter(|id| !bundle.has_message(id))
            .collect()
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
    fn every_shell_message_exists_in_every_catalog() {
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
}
