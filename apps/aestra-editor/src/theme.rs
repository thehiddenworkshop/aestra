//! Editor palette and contextual Feathers theme.
#[path = "theme/feathers.rs"]
mod boundary;
#[path = "theme/palette.rs"]
mod palette;
pub use boundary::feathers_theme;
pub use palette::*;

#[cfg(test)]
#[path = "theme/tests/theme.rs"]
mod tests;
