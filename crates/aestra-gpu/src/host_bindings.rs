//! The GPU host-binding ABI (host bindings HB6): how an instance's binding snapshots are laid out in
//! the `aestra.resource.host_bindings` storage buffer, and the WGSL accessors kernels use to read it.
//!
//! One `array<u32>`:
//!
//! ```text
//! word 0                      slot count N
//! words 1 + 4·i .. 1 + 4·i+3  slot i header: flags (bit 0 = bound), presence mask, stride,
//!                             values offset (absolute word index)
//! words 1 + 4·N ..            each slot's packed f32 values (as bits), `stride` words per slot
//! ```
//!
//! Every slot keeps its region whether bound or not, so all offsets depend only on the compiled
//! layouts — a plugin stage lowerer can compute a field's absolute word at compile time
//! ([`host_binding_value_word`]). The host uploads the buffer once per tick; no per-particle host query
//! ever happens (host bindings roadmap §15).

use aestra_runtime::{BindingSlot, CompiledEffect, EffectInstance};

/// Words per slot header: flags, presence mask, stride, values offset.
pub const HOST_BINDING_HEADER_WORDS: usize = 4;

/// Header flag: the slot currently holds a value.
pub const HOST_BINDING_BOUND: u32 = 1;

/// Absolute word index where each slot's values start, for `effect`'s compiled layouts.
pub fn host_binding_value_offsets(effect: &CompiledEffect) -> Vec<u32> {
    let mut next = 1 + effect.bindings.len() * HOST_BINDING_HEADER_WORDS;
    effect
        .bindings
        .iter()
        .map(|binding| {
            let offset = next as u32;
            next += binding.layout.stride as usize;
            offset
        })
        .collect()
}

/// The absolute word of a field component: the slot's values offset plus the field's layout offset.
pub fn host_binding_value_word(
    effect: &CompiledEffect,
    slot: BindingSlot,
    field_offset: u32,
) -> Option<u32> {
    host_binding_value_offsets(effect)
        .get(slot.0)
        .map(|base| base + field_offset)
}

/// An instance's host bindings packed for upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuHostBindings {
    pub words: Vec<u32>,
}

impl GpuHostBindings {
    /// Packs every slot's value as readers see it (`Live`: current; `SnapshotOnSpawn`: latched).
    pub fn from_instance(instance: &EffectInstance) -> Self {
        let effect = instance.effect();
        let offsets = host_binding_value_offsets(effect);
        let total = 1
            + effect.bindings.len() * HOST_BINDING_HEADER_WORDS
            + effect
                .bindings
                .iter()
                .map(|binding| binding.layout.stride as usize)
                .sum::<usize>();
        let mut words = vec![0u32; total];
        words[0] = effect.bindings.len() as u32;
        for (index, binding) in effect.bindings.iter().enumerate() {
            let header = 1 + index * HOST_BINDING_HEADER_WORDS;
            let snapshot = instance.binding(BindingSlot(index));
            words[header] = if snapshot.is_some() {
                HOST_BINDING_BOUND
            } else {
                0
            };
            words[header + 1] = snapshot.map_or(0, |snapshot| snapshot.present);
            words[header + 2] = binding.layout.stride;
            words[header + 3] = offsets[index];
            if let Some(snapshot) = snapshot {
                let base = offsets[index] as usize;
                for (component, value) in snapshot.values.iter().enumerate() {
                    words[base + component] = value.to_bits();
                }
            }
        }
        Self { words }
    }

    /// Size of the storage buffer in bytes (never zero, so it can always be bound).
    pub fn byte_len(&self) -> u64 {
        (self.words.len().max(1) * 4) as u64
    }

    /// Little-endian bytes for upload.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }
}

/// WGSL accessors for the host-binding buffer. The consuming shader declares the buffer as
/// `var<storage, read> aestra_host_bindings: array<u32>;` at the group/binding it chooses, then
/// appends these functions. Field `offset`s are the compiled layout offsets; `field_index` is the
/// field's position in the layout (its presence bit).
pub const HOST_BINDINGS_WGSL: &str = r#"
// Aestra host bindings (host bindings HB6).
const AESTRA_BINDING_HEADER_WORDS: u32 = 4u;

fn aestra_binding_count() -> u32 {
    return aestra_host_bindings[0];
}

fn aestra_binding_header(slot: u32) -> u32 {
    return 1u + slot * AESTRA_BINDING_HEADER_WORDS;
}

fn aestra_binding_bound(slot: u32) -> bool {
    return slot < aestra_binding_count()
        && (aestra_host_bindings[aestra_binding_header(slot)] & 1u) != 0u;
}

fn aestra_binding_present(slot: u32, field_index: u32) -> bool {
    return aestra_binding_bound(slot)
        && field_index < 32u
        && (aestra_host_bindings[aestra_binding_header(slot) + 1u] & (1u << field_index)) != 0u;
}

fn aestra_binding_f32(slot: u32, offset: u32) -> f32 {
    let base = aestra_host_bindings[aestra_binding_header(slot) + 3u];
    return bitcast<f32>(aestra_host_bindings[base + offset]);
}

fn aestra_binding_vec3(slot: u32, offset: u32) -> vec3<f32> {
    return vec3<f32>(
        aestra_binding_f32(slot, offset),
        aestra_binding_f32(slot, offset + 1u),
        aestra_binding_f32(slot, offset + 2u),
    );
}

fn aestra_binding_vec4(slot: u32, offset: u32) -> vec4<f32> {
    return vec4<f32>(
        aestra_binding_vec3(slot, offset),
        aestra_binding_f32(slot, offset + 3u),
    );
}
"#;
