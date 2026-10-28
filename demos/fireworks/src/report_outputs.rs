//! Bounded aggregate audit of delivered source cues, not a per-particle trace.
use aestra_bevy::AestraOutputEvent;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct OutputAudit {
    routes: BTreeMap<String, Value>,
    omitted_routes: u64,
}
impl OutputAudit {
    pub(super) fn observe(&mut self, event: &AestraOutputEvent, root_time: f32) {
        let Some(spatial) = &event.particle else {
            return;
        };
        let key = format!(
            "{:?}/{:?}/{:?}/{}",
            event.effect, event.clip_path, event.event.origin, event.event.kind
        );
        // Other report assets cannot grow this beyond the small show route set.
        if !self.routes.contains_key(&key) && self.routes.len() >= 256 {
            self.omitted_routes += 1;
            return;
        }
        let row = self.routes.entry(key).or_insert_with(|| json!({
            "messages":0,"kind":event.event.kind,"source_effect":spatial.source_effect.to_string(),
            "clip_path":event.clip_path.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "first_tick":event.event.tick,"last_tick":event.event.tick,"max_delivery_age_seconds":0.0
        }));
        row["messages"] = json!(row["messages"].as_u64().unwrap() + 1);
        row["last_tick"] = json!(event.event.tick);
        let age = (root_time - spatial.root_time_seconds).max(0.0);
        row["max_delivery_age_seconds"] =
            json!(age.max(row["max_delivery_age_seconds"].as_f64().unwrap() as f32));
    }

    pub(super) fn summary(&self) -> Value {
        json!({"routes":self.routes,"omitted_route_messages":self.omitted_routes,
            "scope":"At most 256 aggregate route keys. Main-world source messages and root-clock age at observation; not GPU capture counts or display latency."})
    }
}
