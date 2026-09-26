//! Shared graph clipboard: immutable source snapshots, fresh identities and internal-wire remapping.
//! Text editors own their own clipboard shortcuts; this payload is only used by graph canvases.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Resource, Default)]
pub(crate) struct GraphClipboard {
    pub(crate) fragment: Option<Fragment>,
}

#[derive(Clone)]
pub(crate) struct Fragment {
    expressions: Vec<MaterialExpression>,
    inline_defaults: Vec<MaterialExpression>,
    positions: BTreeMap<MaterialExpressionId, Vec2>,
    disabled: BTreeSet<MaterialExpressionId>,
    constants: BTreeSet<MaterialExpressionId>,
}

pub(crate) struct Insert {
    pub(crate) expressions: Vec<MaterialExpression>,
    pub(crate) positions: BTreeMap<MaterialExpressionId, Vec2>,
    pub(crate) disabled: BTreeSet<MaterialExpressionId>,
    pub(crate) constants: BTreeSet<MaterialExpressionId>,
}

impl Fragment {
    pub(crate) fn capture(
        expressions: &[MaterialExpression],
        selected: &BTreeSet<MaterialExpressionId>,
        positions: BTreeMap<MaterialExpressionId, Vec2>,
        disabled: &[MaterialExpressionId],
        constants: &[MaterialExpressionId],
    ) -> Result<Self, String> {
        let expressions = expressions
            .iter()
            .filter(|expression| selected.contains(&expression.id))
            .cloned()
            .collect::<Vec<_>>();
        if expressions.is_empty() || expressions.len() != selected.len() {
            return Err("Select existing expression nodes first".into());
        }
        let positions = selected
            .iter()
            .map(|id| (*id, positions.get(id).copied().unwrap_or(Vec2::ZERO)))
            .collect::<BTreeMap<_, _>>();
        if positions.values().any(|position| !position.is_finite()) {
            return Err("Cannot copy nodes with invalid graph positions".into());
        }
        // Selected constants are explicit nodes even if their originals were graph outputs.
        // Keep their copies from being pruned as orphaned inline defaults during normalization.
        let constants = expressions
            .iter()
            .filter(|node| matches!(node.kind, MaterialExpressionKind::Constant(_)))
            .map(|node| node.id)
            .chain(constants.iter().filter(|id| selected.contains(id)).copied())
            .collect();
        Ok(Self {
            expressions,
            inline_defaults: Vec::new(),
            positions,
            disabled: disabled
                .iter()
                .filter(|id| selected.contains(id))
                .copied()
                .collect(),
            constants,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.expressions.len()
    }

    /// Inline socket defaults belong to the copied node, not to the original node. Clone them even
    /// when the original still exists, and retain their values if Cut prunes them from the source.
    pub(super) fn with_inline_defaults(mut self, program: &MaterialProgram) -> Self {
        let inline = program.inline_constants();
        let needed = self
            .expressions
            .iter()
            .flat_map(|node| node.kind.dependencies())
            .filter(|id| inline.contains(id) && !self.positions.contains_key(id))
            .collect::<BTreeSet<_>>();
        self.inline_defaults = program
            .expressions
            .iter()
            .filter(|node| needed.contains(&node.id))
            .cloned()
            .collect();
        self.disabled.extend(
            program
                .disabled_expressions
                .iter()
                .filter(|id| needed.contains(id))
                .copied(),
        );
        self
    }

    /// Anchor the group's top-left corner at the cursor, preserving relative placement.
    pub(crate) fn offset_to(&self, anchor: Vec2) -> Vec2 {
        let origin = self
            .positions
            .values()
            .copied()
            .reduce(Vec2::min)
            .unwrap_or(Vec2::ZERO);
        anchor - origin
    }

    pub(crate) fn instantiate(
        &self,
        existing: &BTreeSet<MaterialExpressionId>,
        offset: Vec2,
    ) -> Result<Insert, String> {
        if !offset.is_finite() {
            return Err("Cannot paste at an invalid graph position".into());
        }
        let all = self.inline_defaults.iter().chain(&self.expressions);
        let remapped = all
            .clone()
            .map(|expression| (expression.id, MaterialExpressionId::new()))
            .collect::<BTreeMap<_, _>>();
        // External wires may still connect within the same graph. Never introduce dangling wires
        // when pasting into another graph or after deleting an external source.
        if all.clone().any(|expression| {
            expression
                .kind
                .dependencies()
                .iter()
                .any(|source| !remapped.contains_key(source) && !existing.contains(source))
        }) {
            return Err("Some input nodes are missing here. Copy those input nodes too".into());
        }
        let expressions = all
            .map(|expression| {
                let mut kind = expression.kind.clone();
                aestra_authoring::remap_expression_sources(&mut kind, &remapped);
                MaterialExpression {
                    id: remapped[&expression.id],
                    kind,
                }
            })
            .collect();
        let positions = self
            .positions
            .iter()
            .map(|(id, position)| (remapped[id], *position + offset))
            .collect::<BTreeMap<_, _>>();
        if positions.values().any(|position| !position.is_finite()) {
            return Err("Cannot paste nodes with invalid graph positions".into());
        }
        Ok(Insert {
            expressions,
            positions,
            disabled: self.disabled.iter().map(|id| remapped[id]).collect(),
            constants: self.constants.iter().map(|id| remapped[id]).collect(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shortcut {
    Copy,
    Cut,
    Paste,
    Duplicate,
}

pub(crate) fn shortcut(keys: &ButtonInput<KeyCode>) -> Option<Shortcut> {
    if !(keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) {
        return None;
    }
    [
        (KeyCode::KeyC, Shortcut::Copy),
        (KeyCode::KeyX, Shortcut::Cut),
        (KeyCode::KeyV, Shortcut::Paste),
        (KeyCode::KeyD, Shortcut::Duplicate),
    ]
    .into_iter()
    .find_map(|(key, action)| keys.just_pressed(key).then_some(action))
}

/// Uses the normal material validation/history path; a rejected paste changes nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn insert_material(
    fragment: &Fragment,
    offset: Vec2,
    label: &str,
    program: MaterialProgramId,
    scope: MaterialSelectionScope,
    target: &crate::material_document::MaterialEditingTarget,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    history: &mut MaterialProgramEditHistory,
    ledger: &mut EditorHistoryLedger,
    memory: &mut GraphViewportMemory,
    inspector: &mut MaterialStackInspectorState,
    selection: &mut MaterialGraphSelectionState,
) -> Result<usize, String> {
    activate_material_graph_target(session, catalog, target, program)?;
    let before = session
        .graph_material_programs(catalog)?
        .into_iter()
        .find(|candidate| candidate.id == program)
        .ok_or("Material is unavailable")?;
    let insert = fragment.instantiate(
        &before.expressions.iter().map(|node| node.id).collect(),
        offset,
    )?;
    let mut after = before.clone();
    after.expressions.extend(insert.expressions.iter().cloned());
    after.disabled_expressions.extend(&insert.disabled);
    after.node_constants.extend(&insert.constants);
    let key = material_graph_view_key(program);
    let layout = presentation::Snapshot::capture(&key, catalog, session, memory);
    history.execute_replacement(session, catalog, label, before, after)?;
    ledger.record_material_edit(session);
    for (id, position) in &insert.positions {
        memory.place_node(&key, material_graph_expression_node_key(*id), *position);
    }
    let created = insert.positions.keys().copied().collect();
    selection.select_material_expressions(scope, program, &created, GraphSelectionMode::Replace);
    inspector.selected = insert.expressions.last().map(|node| (program, node.id));
    if let Some(layout) = layout {
        layout.attach(catalog, session, memory);
    }
    Ok(fragment.len())
}
