# Aestra editor Feathers

This folder is the reusable widget boundary for `aestra-editor`. Panels compose these controls;
they do not duplicate control styling, activation bridges, overflow behavior, or numeric scrub
policy.

The structure follows the useful separation in Jackdaw's Bevy 0.19 `jackdaw_feathers` crate:
small data-driven widget modules sit above Bevy Feathers, while panel and application code owns
domain-specific state and semantic commands.

## Current widgets

| Module | Responsibility |
| --- | --- |
| `button` | Editor-action buttons, tool buttons, activation bridging, and action-control auditing |
| `combo_box` | Data-driven combo options and compact action menus |
| `field_row` | Compact, wrapping label/control columns for inspector-like forms |
| `number_input` | Shared precision, modifier, formatting, and delta policy for scrub inputs |
| `panel` | Reusable panel heading chrome |
| `scenes` | BSN scenes for the editor shell and upstream Bevy Feathers controls |
| `scroll` | Native scroll areas, persisted scroll markers, and overflow-only scrollbars |
| `separator` | Theme-aware horizontal and vertical separators |
| `status_bar` | The persistent editor status surface |

## Ownership rule

- Put reusable visual or interaction behavior here.
- Keep effect commands, selections, validation, docking models, timeline semantics, and viewport
  behavior in their owning plugins.
- Prefer Bevy 0.19 Feathers controls and accessibility/focus behavior. Wrap them when Aestra needs
  consistent composition or editor-specific policy.
- Add a widget only when an Aestra surface uses it. Candidate additions from Jackdaw include a
  generic delayed tooltip, remembered panel cards, slider rows, swatch rows, and richer text edits.
