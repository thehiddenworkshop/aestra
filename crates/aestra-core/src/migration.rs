//! Authored-format version detection and the migration entry point.
//!
//! Aestra is **pre-release**, so it does not carry migrations for formats older than the current one:
//! there are no external assets to migrate, and legacy migration code is pure maintenance weight. The
//! machinery here — version detection, the `MigrationRequired` load outcome, and the explicit
//! `UnsupportedFormat` error — is kept, because a permanent migration path will be reintroduced at
//! public release. Today `MIGRATIONS` is empty: a non-current version loads as `UnsupportedFormat`
//! rather than being silently mangled. The one-shot v3 → v4 conversion of the in-repo assets is a
//! dev tool (`aestra-core`'s `migrate_v3_to_v4` example / test helper), not a runtime loader step.

use crate::{AssetError, EffectAsset};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct EffectAssetMigration {
    pub source_version: u32,
    pub target_version: u32,
    pub asset: EffectAsset,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectAssetLoad {
    Current(EffectAsset),
    MigrationRequired(EffectAssetMigration),
}

impl EffectAssetLoad {
    pub fn asset(&self) -> &EffectAsset {
        match self {
            Self::Current(asset) => asset,
            Self::MigrationRequired(migration) => &migration.asset,
        }
    }
}

#[derive(Deserialize)]
struct EffectFormatHeader {
    format_version: u32,
}

pub fn detect_effect_format(source: &str) -> Result<u32, AssetError> {
    Ok(ron::from_str::<EffectFormatHeader>(source)?.format_version)
}

pub fn prepare_effect_asset(source: &str) -> Result<EffectAssetLoad, AssetError> {
    let source_version = detect_effect_format(source)?;
    if source_version == crate::CURRENT_FORMAT_VERSION {
        return Ok(EffectAssetLoad::Current(EffectAsset::from_ron(source)?));
    }

    let Some(step) = MIGRATIONS.iter().find(|step| {
        step.source_version == source_version
            && step.target_version == crate::CURRENT_FORMAT_VERSION
    }) else {
        return Err(AssetError::UnsupportedFormat {
            found: source_version,
            current: crate::CURRENT_FORMAT_VERSION,
        });
    };
    let asset = (step.migrate)(source)?;
    asset.validate()?;
    Ok(EffectAssetLoad::MigrationRequired(EffectAssetMigration {
        source_version,
        target_version: crate::CURRENT_FORMAT_VERSION,
        asset,
    }))
}

struct MigrationStep {
    source_version: u32,
    target_version: u32,
    migrate: fn(&str) -> Result<EffectAsset, AssetError>,
}

/// No runtime migrations pre-release (see the module docs). Reintroduced at public release.
const MIGRATIONS: &[MigrationStep] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_current_and_legacy_versions_without_full_deserialization() {
        assert_eq!(detect_effect_format("(format_version: 3)").unwrap(), 3);
        assert_eq!(
            detect_effect_format("(format_version: 4, extra: 1)").unwrap(),
            4
        );
    }

    #[test]
    fn non_current_versions_are_rejected_without_mutation() {
        // Pre-release: no migrations are carried, so any non-current version is an explicit
        // unsupported-format error rather than a silent best-effort load.
        for version in [
            "(format_version: 2)",
            "(format_version: 3)",
            "(format_version: 99)",
        ] {
            let detected = detect_effect_format(version).unwrap();
            if detected == crate::CURRENT_FORMAT_VERSION {
                continue;
            }
            let error = prepare_effect_asset(version).unwrap_err();
            assert!(matches!(
                error,
                AssetError::UnsupportedFormat {
                    found,
                    current,
                } if found == detected && current == crate::CURRENT_FORMAT_VERSION
            ));
        }
    }
}
