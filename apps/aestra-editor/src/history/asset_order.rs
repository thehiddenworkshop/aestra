//! Ordering bridge for filesystem deletion and existing document histories.
//! Enabled by the first deletion; existing per-document behaviour is unchanged
//! otherwise. Recovery records outlive this bounded, session-local Undo history.
use super::*;
use crate::material_document::MaterialEditingTarget;
use aestra_project::content::operations::DeletedSource;

const LIMIT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Context {
    Effect,
    Material(MaterialEditingTarget),
}
impl Context {
    pub(crate) fn current(session: &EditorSession) -> Self {
        if (session.standalone_function().is_some() || session.standalone_material().is_some())
            && session.material_history_active
        {
            Self::Material(session.material_target.clone())
        } else {
            Self::Effect
        }
    }
    pub(crate) fn select(&self, session: &mut EditorSession) {
        match self {
            Self::Effect => {
                session.material_target = MaterialEditingTarget::EffectInstance;
                session.material_history_active = false;
            }
            Self::Material(target) => {
                session.material_target = target.clone();
                session.material_history_active = true;
            }
        }
    }
}

#[derive(Default, Clone)]
pub(crate) struct EditOrder {
    serial: u64,
    edits: VecDeque<(u64, Context)>,
    applied: VecDeque<OrderedEdit>,
    redo: Vec<OrderedEdit>,
}
#[derive(Clone)]
struct OrderedEdit {
    context: Context,
    layout: Option<crate::material_graph::presentation::Transaction>,
    semantic: bool,
}
impl EditOrder {
    pub(crate) fn edit_serial(&self) -> u64 {
        self.serial
    }
    pub(crate) fn clear_context(&mut self, context: &Context) {
        self.applied.retain(|entry| &entry.context != context);
        self.redo.retain(|entry| &entry.context != context);
        self.edits.retain(|(_, entry)| entry != context);
    }
    pub(crate) fn record(&mut self, context: Context) {
        self.record_entry(context, None, true);
    }
    fn record_entry(
        &mut self,
        context: Context,
        layout: Option<crate::material_graph::presentation::Transaction>,
        semantic: bool,
    ) {
        self.serial += 1;
        self.applied.push_back(OrderedEdit {
            context: context.clone(),
            layout,
            semantic,
        });
        self.redo.retain(|entry| entry.context != context);
        self.edits.push_back((self.serial, context));
        while self.edits.len() > LIMIT {
            self.edits.pop_front();
        }
        while self.applied.len() > LIMIT {
            self.applied.pop_front();
        }
    }
    pub(crate) fn step(&mut self, context: Context, undo: bool) {
        if undo {
            if let Some(index) = self
                .applied
                .iter()
                .rposition(|entry| entry.context == context)
            {
                self.redo.push(self.applied.remove(index).unwrap());
            }
        } else {
            if let Some(index) = self.redo.iter().rposition(|entry| entry.context == context) {
                self.applied.push_back(self.redo.remove(index));
            }
            while self.applied.len() > LIMIT {
                self.applied.pop_front();
            }
        }
    }
    pub(crate) fn record_layout(
        &mut self,
        context: Context,
        transaction: crate::material_graph::presentation::Transaction,
    ) {
        self.record_entry(context, Some(transaction), false);
    }
    pub(crate) fn attach_layout(
        &mut self,
        context: Context,
        transaction: crate::material_graph::presentation::Transaction,
    ) {
        if let Some(entry) = self
            .applied
            .back_mut()
            .filter(|entry| entry.context == context && entry.semantic)
        {
            entry.layout = Some(transaction);
        }
    }
    pub(crate) fn layout(
        &self,
        context: &Context,
        undo: bool,
    ) -> Option<(&crate::material_graph::presentation::Transaction, bool)> {
        let entry = if undo {
            self.applied
                .iter()
                .rev()
                .find(|entry| &entry.context == context)
        } else {
            self.redo
                .iter()
                .rev()
                .find(|entry| &entry.context == context)
        }?;
        entry.layout.as_ref().map(|layout| (layout, entry.semantic))
    }
    pub(crate) fn clear_redo(&mut self) {
        self.redo.clear();
    }
    pub(crate) fn invalidate_layouts(&mut self, graph: Option<&str>) {
        for entry in self.applied.iter_mut().chain(self.redo.iter_mut()) {
            if let Some(layout) = &mut entry.layout {
                layout.invalidate(graph);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Entry {
    Document(Context),
    Delete(Box<DeletedSource>),
}

#[derive(Resource, Default)]
pub(crate) struct AssetOrder {
    root: Option<(PathBuf, u64)>,
    generation: u64,
    effect: Option<EffectId>,
    observed: u64,
    pub(super) revision: u64,
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
}
impl AssetOrder {
    pub(crate) fn clear_context(&mut self, context: &Context) {
        self.undo
            .retain(|entry| !matches!(entry, Entry::Document(candidate) if candidate == context));
        self.redo
            .retain(|entry| !matches!(entry, Entry::Document(candidate) if candidate == context));
        self.revision += 1;
    }
    /// Returns true when a new document edit invalidates the global redo branch.
    pub(crate) fn sync(&mut self, session: &EditorSession, catalog: &ProjectEffectCatalog) -> bool {
        let Some((root, generation)) = &self.root else {
            return false;
        };
        if root != catalog.root()
            || *generation != catalog.content_revision().generation
            || self.generation != session.history_generation()
            || self.effect != Some(session.effect.id)
        {
            *self = Self::default();
            return false;
        }
        let serial = session.operation_order.serial;
        if serial == self.observed {
            return false;
        }
        // Never guess ordering after a bounded journal overflow.
        if serial < self.observed || serial - self.observed > LIMIT as u64 {
            *self = Self::default();
            return false;
        }
        for (_, context) in session
            .operation_order
            .edits
            .iter()
            .filter(|(serial, _)| *serial > self.observed)
        {
            self.undo.push_back(Entry::Document(context.clone()));
        }
        self.observed = serial;
        self.redo.clear();
        self.revision += 1;
        while self.undo.len() > LIMIT {
            self.undo.pop_front();
        }
        true
    }
    pub(super) fn active(&self) -> bool {
        self.root.is_some()
    }
    pub(super) fn top(&self, undo: bool) -> Option<&Entry> {
        if undo {
            self.undo.back()
        } else {
            self.redo.last()
        }
    }
    pub(super) fn finish_document(&mut self, undo: bool, fallback: Context) {
        let entry = if undo {
            self.undo.pop_back()
        } else {
            self.redo.pop()
        }
        .unwrap_or(Entry::Document(fallback));
        if undo {
            self.redo.push(entry);
        } else {
            self.undo.push_back(entry);
        }
        self.revision += 1;
    }
    pub(crate) fn record(
        &mut self,
        item: DeletedSource,
        session: &EditorSession,
        catalog: &ProjectEffectCatalog,
    ) {
        self.sync(session, catalog);
        if !self.active() {
            self.undo.extend(
                session
                    .operation_order
                    .applied
                    .iter()
                    .map(|entry| entry.context.clone())
                    .map(Entry::Document),
            );
        }
        self.root = Some((
            catalog.root().to_owned(),
            catalog.content_revision().generation,
        ));
        self.effect = Some(session.effect.id);
        self.generation = session.history_generation();
        self.observed = session.operation_order.serial;
        self.undo.push_back(Entry::Delete(Box::new(item)));
        while self.undo.len() > LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.revision += 1;
    }
    pub(crate) fn finish_delete(&mut self, undo: bool, revision: u64, item: DeletedSource) -> bool {
        if self.revision != revision || !matches!(self.top(undo), Some(Entry::Delete(_))) {
            return false;
        }
        if undo {
            self.undo.pop_back();
            self.redo.push(Entry::Delete(Box::new(item)));
        } else {
            self.redo.pop();
            self.undo.push_back(Entry::Delete(Box::new(item)));
        }
        self.revision += 1;
        true
    }
    pub(crate) fn matches_delete(&self, undo: bool, revision: u64) -> bool {
        self.revision == revision && matches!(self.top(undo), Some(Entry::Delete(_)))
    }
    pub(crate) fn forget_restored(&mut self, journal: &std::path::Path) {
        self.undo
            .retain(|entry| !matches!(entry, Entry::Delete(item) if item.journal() == journal));
        self.redo.clear();
        self.revision += 1;
    }
}

pub(crate) fn record_delete(world: &mut World, item: DeletedSource) {
    world.init_resource::<AssetOrder>();
    world.resource_scope(|world, mut order: Mut<AssetOrder>| {
        order.record(
            item,
            world.resource::<EditorSession>(),
            world.resource::<ProjectEffectCatalog>(),
        );
    });
    clear_redo(world);
}

pub(crate) fn clear_redo(world: &mut World) {
    world.resource_mut::<EditorSession>().clear_effect_redo();
    world
        .resource_mut::<EditorSession>()
        .operation_order
        .clear_redo();
    if let Some(mut ledger) = world.get_resource_mut::<EditorHistoryLedger>() {
        ledger.redo.clear();
    }
    if let Some(mut history) = world.get_resource_mut::<MaterialProgramEditHistory>() {
        clear_material_redo(&mut history);
    }
    if let Some(mut functions) =
        world.get_resource_mut::<crate::material_function_editor::FunctionEditor>()
    {
        functions.clear_redo();
    }
}
pub(super) fn clear_material_redo(history: &mut MaterialProgramEditHistory) {
    history.redo.clear();
    for history in history.standalone.values_mut() {
        history.redo.clear();
    }
}
