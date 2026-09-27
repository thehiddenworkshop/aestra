# First user experience: project to first effect

## Goal

A new user can create a project, create a visible effect, edit it, save it, and
reopen it without needing to understand the repository layout or internal asset
formats. Returning users keep their restored workspace and unsaved-work safeguards.

## FX1 — New Project

Implemented:

- File → New Project, using the existing unsaved-work confirmation and project-open pipeline.
- Name and parent-folder fields, a native folder picker, and a full destination preview.
- A larger, clearly framed dialog: prominent title, boxed fields, inline Browse,
  quiet destination summary, and explicit primary action. Validation appears after
  editing, not as an initial error; Windows device prefixes stay out of displayed paths.
- Background creation and project loading, with busy controls and retryable errors.
- Successful creation reveals the new Project assets and activates Timeline/Module Stack,
  clears old editor focus, and exits maximized panels instead of leaving an old graph visible.
- Portable name validation, absolute existing parent directory validation, and refusal to
  merge with an existing destination (including empty folders and case-only collisions).
- A conventional empty structure: `assets/effects`, `assets/materials`, `assets/meshes`,
  `assets/shaders`, and `assets/textures`.
- Failure cleanup limited to newly created empty directories; foreign files are never deleted.
- Publication guarded against concurrent document/project changes. If creation succeeds
  but opening becomes stale, preserve the current document and report the new project path.
- Cancel/Escape, keyboard focus, Enter to create, and English/French strings.

Boundary: FX1 follows the existing Open Project session reset and leaves the standard
unsaved starter effect in the editor. It does not create an effect file or copy examples.
FX2 makes the first effect a named, explicitly created project asset.

Manual acceptance:

1. File → New Project; enter a name, select a parent folder, inspect the destination.
2. Create; verify the asset browser switches to the new empty asset folders.
3. Cancel/Escape; verify no folder is created and the current document stays open.
4. Try an existing name, invalid name, or unwritable location; verify a useful error,
   no overwrite, and the ability to correct the inputs and retry.
5. Modify the current effect, then choose New Project; exercise Cancel, Save, and Discard.

## FX2 — First effect creation

Implemented:

- An actionable empty-project state with Create Effect.
- One creation flow shared by File → New Effect and the Project asset browser.
- Name and destination selection; a simple visible sprite-emitter starter.
- Persist the new asset safely, select its browser row, and open its Timeline/Module Stack.
- Frame the preview and select the starter emitter. Avoid an unexplained unsaved placeholder.
- Background no-clobber source publication, portable names, and project-contained destinations.
- Cancel/retry and unsaved-work protection; untouched scratch starters need no discard prompt.

Manual acceptance:

1. Create a project → Create Effect; name it and verify its saved row, Timeline, and sprite preview.
2. File → New Effect / Ctrl+N and the Project browser plus/context-menu actions use the same dialog.
3. Browse to a project subfolder; verify the full destination and selected saved asset.
4. Cancel, invalid/outside folders, existing names, and Save/Discard of edited effects preserve work.
5. Edit, save, close, and reopen the newly created effect.

## FX3 — First launch and returning users (next)

- New Project, Open Project, Recent Projects, and explicit Explore Examples entry points.
- Stop treating the bundled sample project as the implicit new-user working project.
- Keep project/workspace restoration for returning users and handle missing recent locations.

## FX4 — Editing feedback

- Clear project/effect identity and saved/unsaved status.
- Actionable empty states and readable loading/error feedback.
- Sensible initial framing/selection, without persistent viewport navigation hints.
- Keep the preview and UI responsive during preparation and thumbnail work.

## FX5 — Complete journey verification

- Create project → create effect → edit → save → close → reopen.
- Cancellation, collisions, invalid names, write failures, and unsaved-work protection.
- Fresh-install and restored-workspace paths, including narrow windows and both locales.

Defer a large template gallery, tutorial wizard, and advanced project configuration
until this basic journey is reliable.
