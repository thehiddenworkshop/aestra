//! Stable identity of one material graph preview; rendering/cache ownership stays host-side.
use aestra_core::MaterialExpressionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MaterialGraphPreviewTarget {
    Expression(MaterialExpressionId),
    Output,
}
