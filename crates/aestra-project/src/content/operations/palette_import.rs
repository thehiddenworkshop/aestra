use super::*;
use aestra_core::{MAX_PALETTE_COLORS, PaletteAsset};
use image::{ColorType, ImageEncoder, ImageFormat, ImageReader, codecs::png::PngEncoder};
use std::io::{Cursor, Read, Write};

#[derive(Clone)]
pub struct PaletteSource {
    root: PathBuf,
    path: PathBuf,
    original: Vec<u8>,
    palette: PaletteAsset,
}

impl PaletteSource {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn palette(&self) -> &PaletteAsset {
        &self.palette
    }

    /// Replaces the same project texture after checking the original on-disk bytes.
    pub fn save(&self, palette: &PaletteAsset) -> Result<(), OperationError> {
        self.save_revision(palette).map(|_| ())
    }

    /// Saves the palette and returns the exact replacement as the next edit baseline.
    pub fn save_revision(&self, palette: &PaletteAsset) -> Result<Self, OperationError> {
        transaction::ensure_idle(&self.root)?;
        let parent = self.path.parent().expect("palette is a project file");
        checked_parent(&self.root, parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        let mut original = Vec::new();
        write_palette(&mut original, palette)?;
        staged.write_all(&original)?;

        transaction::ensure_idle(&self.root)?;
        checked_parent(&self.root, parent)?;
        let (current, metadata) = read_palette_file(&self.path)?;
        if current != self.original {
            return Err(blocked(
                "Palette changed on disk. Reopen it to edit the new version.",
            ));
        }
        if metadata.permissions().readonly() {
            return Err(blocked("Palette is read-only"));
        }
        staged.as_file().set_permissions(metadata.permissions())?;
        staged.as_file().sync_all()?;
        staged
            .persist(&self.path)
            .map_err(|error| OperationError::Io(error.error.to_string()))?;
        Ok(Self {
            root: self.root.clone(),
            path: self.path.clone(),
            original,
            palette: palette.clone(),
        })
    }
}

impl ProjectContent {
    pub fn is_palette_source(&self, source: ProjectSourceId) -> bool {
        self.source(source).is_some_and(|entry| {
            matches!(entry.kind, ProjectSourceKind::File(_))
                && entry
                    .name
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".palette.png")
        })
    }

    pub fn open_palette_source(
        &self,
        source: ProjectSourceId,
    ) -> Result<PaletteSource, OperationError> {
        let entry = self
            .source(source)
            .filter(|_| self.is_palette_source(source))
            .ok_or_else(|| blocked("Choose a project palette texture"))?;
        let root = self.source_tree().root_path();
        checked_parent(
            root,
            entry
                .path
                .parent()
                .ok_or_else(|| blocked("Palette has no parent folder"))?,
        )?;
        let (original, _) = read_palette_file(&entry.path)?;
        let reader = || ImageReader::with_format(Cursor::new(&original), ImageFormat::Png);
        let (width, height) = reader().into_dimensions().map_err(palette_image_error)?;
        if height != 1 || !(1..=MAX_PALETTE_COLORS as u32).contains(&width) {
            return Err(blocked(
                "Palette must contain one row of up to 256 opaque swatches",
            ));
        }
        let image = reader().decode().map_err(palette_image_error)?.into_rgba8();
        let colors = image
            .pixels()
            .map(|pixel| {
                let [red, green, blue, alpha] = pixel.0;
                if alpha != 255 {
                    return Err(blocked("Palette swatches must be opaque"));
                }
                Ok([red, green, blue])
            })
            .collect::<Result<Vec<_>, _>>()?;
        let palette = PaletteAsset::new(colors).map_err(|error| blocked(&error.to_string()))?;
        Ok(PaletteSource {
            root: root.to_owned(),
            path: entry.path.clone(),
            original,
            palette,
        })
    }

    /// Stores ordered sRGB swatches as a one-row texture that can be bound by a material.
    pub fn import_palette(
        &self,
        parent: ProjectSourceId,
        name: &str,
        palette: &PaletteAsset,
    ) -> Result<PathBuf, OperationError> {
        if !valid_name(name) {
            return Err(blocked("Choose a portable palette filename"));
        }
        let plan = self.plan_operation(OperationRequest::CreateFolder {
            parent,
            name: format!("{name}.palette.png"),
        })?;
        let mut staged = tempfile::NamedTempFile::new_in(&plan.parent)?;
        write_palette(&mut staged, palette)?;
        staged.as_file().sync_all()?;

        transaction::ensure_idle(&plan.root)?;
        checked_parent(&plan.root, &plan.parent)?;
        vacant(&plan.parent, &plan.destination)?;
        staged
            .persist_noclobber(&plan.destination)
            .map_err(|error| OperationError::Io(error.error.to_string()))?;
        Ok(plan.destination)
    }
}

fn palette_image_error(error: image::ImageError) -> OperationError {
    OperationError::Io(error.to_string())
}

fn write_palette(
    output: impl std::io::Write,
    palette: &PaletteAsset,
) -> Result<(), OperationError> {
    let colors = palette.colors();
    PngEncoder::new(output)
        .write_image(
            colors.as_flattened(),
            colors.len() as u32,
            1,
            ColorType::Rgb8.into(),
        )
        .map_err(palette_image_error)
}

fn read_palette_file(path: &Path) -> Result<(Vec<u8>, fs::Metadata), OperationError> {
    // An authored strip has at most 768 RGB bytes. Bound reads while allowing PNG metadata.
    const MAX_PNG_BYTES: u64 = 1024 * 1024;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || super::super::source_tree::is_link(&metadata) {
        return Err(blocked("Palette must be a regular project file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_PNG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PNG_BYTES {
        return Err(blocked("Palette PNG exceeds 1 MiB"));
    }
    Ok((bytes, metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::PaletteError;

    #[test]
    fn gimp_palette_keeps_swatch_order_and_import_never_overwrites() {
        let palette = PaletteAsset::from_gpl(
            "\u{feff}GIMP Palette\r\nName: Test\r\nColumns: 2\r\n# Swatches\r\n240 190 80 Amber\r\n230\t70\t150 Rose\r\n",
        )
        .unwrap();
        assert_eq!(palette.colors(), &[[240, 190, 80], [230, 70, 150]]);
        assert_eq!(
            PaletteAsset::from_gpl("GIMP Palette\n256 0 0"),
            Err(PaletteError::InvalidSwatch(2))
        );
        let too_many = format!("GIMP Palette\n{}", "1 2 3\n".repeat(MAX_PALETTE_COLORS + 1));
        assert_eq!(
            PaletteAsset::from_gpl(&too_many),
            Err(PaletteError::InvalidColorCount)
        );

        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        let parent = content.source_tree().root();
        let path = content.import_palette(parent, "colors", &palette).unwrap();
        let png = image::open(&path).unwrap().to_rgb8();
        assert_eq!(png.dimensions(), (2, 1));
        assert_eq!(png.as_raw(), palette.colors().as_flattened());
        let other = PaletteAsset::new(vec![[0; 3]]).unwrap();
        assert!(content.import_palette(parent, "COLORS", &other).is_err());
        assert!(content.import_palette(parent, "../escape", &other).is_err());
        assert_eq!(image::open(&path).unwrap().to_rgb8(), png);
    }

    #[test]
    fn palette_edits_preserve_path_and_reject_stale_writes() {
        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        let original = PaletteAsset::new(vec![[1, 2, 3], [4, 5, 6]]).unwrap();
        let path = content
            .import_palette(content.source_tree().root(), "pulse", &original)
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let id = content
            .source_tree()
            .at_relative_path(Path::new("pulse.palette.png"))
            .unwrap()
            .id;
        let source = content.open_palette_source(id).unwrap();
        assert_eq!(source.palette(), &original);
        let edited = PaletteAsset::new(vec![[4, 5, 6], [10, 20, 30], [1, 2, 3]]).unwrap();
        source.save(&edited).unwrap();
        assert_eq!(source.path(), path);
        assert_eq!(content.open_palette_source(id).unwrap().palette(), &edited);
        let stale = source.save(&original).unwrap_err();
        assert!(stale.to_string().contains("changed on disk"));
        assert_eq!(content.open_palette_source(id).unwrap().palette(), &edited);
    }
}
