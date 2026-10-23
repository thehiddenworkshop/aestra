//! Per-document semantic/presentation chronology; no session or filesystem adapter.
use crate::material_target::MaterialEditingTarget;
use std::collections::VecDeque;

pub(super) const LIMIT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Context {
    Effect,
    Material(MaterialEditingTarget),
}
#[derive(Default, Clone)]
pub(crate) struct EditOrder {
    pub(super) serial: u64,
    pub(super) edits: VecDeque<(u64, Context)>,
    pub(super) applied: VecDeque<OrderedEdit>,
    pub(super) redo: Vec<OrderedEdit>,
}
#[derive(Clone)]
pub(super) struct OrderedEdit {
    pub(super) context: Context,
    pub(super) layout: Option<crate::material_graph::presentation::Transaction>,
    pub(super) semantic: bool,
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

#[cfg(test)]
#[path = "../tests/asset_order_model.rs"]
mod tests;
