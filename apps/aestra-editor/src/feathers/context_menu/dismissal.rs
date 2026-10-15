//! Shared production pointer-popup dismissal policy.
pub(crate) fn should_dismiss_pointer_context_menu(
    open: bool,
    primary_pressed: bool,
    escape_pressed: bool,
    pointer_over_surface: bool,
) -> bool {
    open && (escape_pressed || (primary_pressed && !pointer_over_surface))
}
