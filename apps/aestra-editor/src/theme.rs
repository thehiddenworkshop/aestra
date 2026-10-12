//! Editor palette with a small engine-version-specific Feathers storage boundary.
#[path = "theme/bevy_019.rs"]
mod boundary;
#[path = "theme/palette.rs"]
mod palette;
pub use boundary::feathers_theme;
pub use palette::*;

#[cfg(test)]
#[path = "theme/tests.rs"]
mod tests;
