use super::*;

#[derive(Component)]
pub(super) struct MaterialProgramName(pub(super) MaterialProgramId);

pub(super) fn rename(
    change: On<ValueChange<String>>,
    controls: Query<&MaterialProgramName>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut history: ResMut<MaterialProgramEditHistory>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if session.standalone_material() != Some(control.0) {
        return;
    }
    session.material_history_active = true;
    let result = (|| {
        let name = change.value.trim();
        if name.is_empty() {
            return Err("A material name is required".to_owned());
        }
        let before = catalog.material_program(control.0)?;
        if before.name == name {
            return Ok(());
        }
        let mut after = before.clone();
        after.name = name.to_owned();
        history.execute_replacement(
            &mut session,
            &mut catalog,
            "Rename shared material",
            before,
            after,
        )
    })();
    if let Err(error) = result {
        session.status = error;
    }
    session.ui_revision += 1;
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) -> bool {
    if session.standalone_function().is_some() {
        panel_heading(parent, "SHARED FUNCTION", "SIGNATURE");
        spawn_vertical_scroll_area(
            parent,
            ScrollMemoryKey::Properties,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            |body| {
                body.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|body| {
                    if let Ok(function) = session.graph_function(catalog) {
                        crate::material_function_editor::spawn(body, &function);
                    }
                    crate::diagnostics::details::spawn_summary(body, &session.status, localizer);
                });
            },
        );
        return true;
    }
    let Some(id) = session.standalone_material() else {
        return false;
    };
    let result = session.graph_authoring_document(catalog);
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(panel, &localizer.text("material-document-shared"), "");
            panel
                .spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|scroll_body| {
                    spawn_vertical_scroll_area(
                        scroll_body,
                        ScrollMemoryKey::Properties,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            ..default()
                        },
                        |body| {
                            let document = match result {
                                Ok(document) => document,
                                Err(error) => {
                                    crate::diagnostics::details::spawn_summary(
                                        body, &error, localizer,
                                    );
                                    return;
                                }
                            };
                            let program = &document.programs[0];
                            crate::feathers::field_row::spawn_field_row(
                                body,
                                crate::feathers::field_row::FieldRowProps::new(
                                    localizer.text("properties-name"),
                                ),
                                EditorTooltip::description(
                                    localizer.text("material-document-description"),
                                ),
                                |input| {
                                    spawn_text_input(
                                        input,
                                        &program.name,
                                        &localizer.text("properties-name"),
                                        MaterialProgramName(id),
                                    );
                                },
                            );
                            spawn_properties_read_only_control(
                                body,
                                &localizer.text("material-document-domain"),
                                &format!("{:?}", program.domain),
                            );
                            spawn_properties_read_only_control(
                                body,
                                &localizer.text("material-document-state"),
                                &localizer.text(
                                    if catalog.material_drafts.programs.contains_key(&id) {
                                        "save-state-unsaved"
                                    } else {
                                        "save-state-saved"
                                    },
                                ),
                            );
                            body.spawn((
                                Text::new(localizer.text("material-document-description")),
                                TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT_MUTED),
                                Node {
                                    margin: UiRect::vertical(Val::Px(12.0)),
                                    ..crate::diagnostics::details::wrapped_text_node()
                                },
                            ));
                            for parameter in &program.parameters {
                                spawn_properties_read_only_control(
                                    body,
                                    &parameter.name,
                                    &default_summary(parameter.default.as_ref(), localizer),
                                );
                            }
                            for diagnostic in document.validation_report().diagnostics {
                                crate::diagnostics::details::spawn_summary(
                                    body,
                                    &format!(
                                        "{:?} · {:?}\n{}\n{}",
                                        diagnostic.severity,
                                        diagnostic.code,
                                        diagnostic.message,
                                        diagnostic.path
                                    ),
                                    localizer,
                                );
                            }
                        },
                    );
                });
        });
    true
}

fn default_summary(value: Option<&MaterialValue>, localizer: &Localizer) -> String {
    let components = |values: &[f32]| {
        values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    match value {
        Some(MaterialValue::Float(value)) => value.to_string(),
        Some(MaterialValue::Vec2(values)) => components(values),
        Some(MaterialValue::Vec3(values)) => components(values),
        Some(MaterialValue::Vec4(values) | MaterialValue::ColorSrgb(values)) => components(values),
        Some(MaterialValue::Bool(value)) => value.to_string(),
        Some(MaterialValue::Texture2D(id)) => id.to_string(),
        None => localizer.text("material-document-no-default"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_properties_show_source_controls_and_rename_only_the_shared_draft() {
        let root = tempfile::tempdir().unwrap();
        let program =
            aestra_core::material::MaterialProgram::additive_sprite("Shared").normalized();
        let path = root.path().join("shared.aestra.material.ron");
        program.save_ron(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session.open_material_program(&catalog, program.id).unwrap();
        let effect = session.effect.clone();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            bevy::text::TextPlugin,
        ));
        let assets = app.world().resource::<AssetServer>().clone();
        let localizer = Localizer::new("en-US").unwrap();
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                spawn_properties(
                    parent,
                    &session,
                    &EditorModuleRegistry::default(),
                    &ModulePaletteState::default(),
                    &localizer,
                    &EditorSettings::default(),
                    &catalog,
                    &TimelineState::default(),
                    &EffectClipRepairState::default(),
                    &MaterialStackInspectorState::default(),
                    None,
                    &assets,
                    &crate::editor_view::ActiveEditorContext::default(),
                    &crate::document::DocumentManager::default(),
                    &crate::wesl_document::WeslDocuments::default(),
                    &crate::wesl_document::WeslDiagnostics::default(),
                );
            });
        app.world_mut().flush();
        let world = app.world_mut();
        assert_eq!(world.query::<&DocumentTextControl>().iter(world).count(), 0);
        let control = world
            .query_filtered::<Entity, With<MaterialProgramName>>()
            .single(world)
            .unwrap();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<MaterialProgramEditHistory>()
            .add_observer(rename);
        app.world_mut().trigger(ValueChange {
            source: control,
            value: "Renamed shared".to_owned(),
            is_final: true,
        });
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(program.id)
                .unwrap()
                .name,
            "Renamed shared"
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        assert_eq!(app.world().resource::<EditorSession>().effect_undo_len(), 0);
        assert_eq!(
            aestra_core::material::MaterialProgram::load_ron(&path).unwrap(),
            program
        );
        app.world_mut()
            .resource_mut::<EditorSession>()
            .return_to_effect_material();
        app.world_mut().trigger(ValueChange {
            source: control,
            value: "Stale event".to_owned(),
            is_final: true,
        });
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(program.id)
                .unwrap()
                .name,
            "Renamed shared"
        );
    }
}
