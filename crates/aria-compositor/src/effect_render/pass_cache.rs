//! Reuse of pass bind groups and uniform buffers.
//!
//! A pass is described completely by the layout it binds against and what it
//! binds into it: the textures, the samplers, and the constants. Two passes
//! agreeing on all of that issue the same binding, so they can share one bind
//! group and one uniform buffer — a bind group is a *binding*, not a result, so
//! sharing it says nothing about what the bound texture holds when each pass
//! executes. That is what makes the sharing safe across layers: a grade applied
//! to eight clips reads pool slot 0 eight times, each time after a different
//! layer was drawn into it, and one bind group serves all eight.
//!
//! Without this, every pass of every frame allocated a uniform buffer and a
//! bind group and dropped both at submit: twelve layers with a grade, a LUT and
//! two effects each meant four of each per layer, per frame, for descriptors
//! that were usually identical to the previous frame's.
//!
//! The pipeline is not part of what makes two passes the same. A bind group is
//! valid for any pipeline built against the same layout, so two effects reading
//! one slot with equal constants share a descriptor.
//!
//! # Identity
//!
//! A slot holds the resource handle, not its address. Two handles to one wgpu
//! resource compare equal, and holding one keeps the resource alive, so an entry
//! can neither name something that was freed nor collide with a later resource
//! that landed at the same address. That is what lets passes outside the
//! offscreen pool — a canvas target, an uploaded LUT — be cached at all.
//!
//! # Bounds
//!
//! Entries survive across frames, since a paused playhead re-renders the same
//! descriptors, and are bounded twice. A frame that has not drawn with an entry
//! for [`MAX_IDLE_FRAMES`] drops it, so a scene that shrinks stops holding the
//! textures the larger one bound. At [`MAX_ENTRIES`] the entries not used in the
//! current frame are dropped, and if every one of them was used the whole table
//! is dropped and rebuilt. Memory is therefore capped whatever the scene does,
//! at the cost of rebuilding after a scene far larger than the cap.

use std::cell::{RefCell, RefMut};

/// Live entries kept before the table is pruned.
///
/// A frame draws one pass per effect per layer, plus a handful per layer for the
/// grade, LUT and composite. Bind-group entries are capped independently; the
/// uniform pool retains the maximum number of distinct constants used
/// concurrently by any completed frame.
pub(crate) const MAX_ENTRIES: usize = 128;

/// Frames an entry may go undrawn before it is dropped.
///
/// An entry holds the textures it binds alive, so an unbounded table would keep
/// a shrunken scene's offscreens and LUTs resident. Eight frames is long enough
/// that a paused playhead, a scrub and a seek all still reuse, and short enough
/// that a scene change gives the memory back within a frame time.
pub(crate) const MAX_IDLE_FRAMES: u64 = 8;

/// What the cached passes of one frame cost.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PassStats {
    /// Passes drawn through the cache.
    pub passes: u32,
    /// Bind groups built for them.
    pub created: u32,
    /// Passes that reused one already built.
    pub reused: u32,
    /// Uniform-buffer writes issued this frame.
    pub uniform_writes: u32,
    /// Uniform buffers allocated this frame. Warm playback must report zero.
    pub uniform_buffers_created: u32,
    /// Persistent uniform slots retained for concurrent pass values.
    pub uniform_slots: u32,
    /// Entries the cache holds after the frame.
    pub entries: u32,
    /// Entries dropped to stay under `MAX_ENTRIES` or past
    /// `MAX_IDLE_FRAMES`, across all frames so far.
    pub evictions: u32,
}

/// One resource a pass binds.
///
/// Position in the list is the binding number: every layout the registry
/// declares numbers its bindings from zero without a gap, and a list that
/// disagrees with its layout is refused by wgpu when the bind group is built.
pub(crate) enum Bound<'a> {
    Texture(&'a wgpu::TextureView),
    Sampler(&'a wgpu::Sampler),
    /// Pass constants. Changed bytes rewrite a persistent buffer slot.
    Uniform(&'a [u8]),
}

/// Maximum resources in one pass layout. YUV-with-effects uses six; keeping
/// per-call preparation on the stack avoids a heap vector.
const MAX_BOUND_SLOTS: usize = 6;

/// A resource retained by one bind-group entry.
enum Slot {
    Texture(wgpu::TextureView),
    Sampler(wgpu::Sampler),
    /// Index into the process-stable uniform slot pool.
    Uniform(usize),
}

impl Slot {
    fn matches_exact(&self, bound: &Bound<'_>, uniforms: &[UniformSlot]) -> bool {
        match (self, bound) {
            (Slot::Texture(held), Bound::Texture(asked)) => held == *asked,
            (Slot::Sampler(held), Bound::Sampler(asked)) => held == *asked,
            (Slot::Uniform(slot), Bound::Uniform(asked)) => {
                uniforms[*slot].bytes.as_slice() == *asked
            }
            _ => false,
        }
    }

    fn matches_binding(&self, bound: &Bound<'_>, uniform_slot: Option<usize>) -> bool {
        match (self, bound) {
            (Slot::Texture(held), Bound::Texture(asked)) => held == *asked,
            (Slot::Sampler(held), Bound::Sampler(asked)) => held == *asked,
            (Slot::Uniform(held), Bound::Uniform(_)) => Some(*held) == uniform_slot,
            _ => false,
        }
    }
}

struct UniformSlot {
    buffer: wgpu::Buffer,
    capacity: u64,
    bytes: Vec<u8>,
    used: bool,
}

struct Entry {
    layout: wgpu::BindGroupLayout,
    slots: Box<[Slot]>,
    bind_group: wgpu::BindGroup,
    /// Frame this entry was last drawn with.
    used: u64,
}

struct Table {
    entries: Vec<Entry>,
    uniforms: Vec<UniformSlot>,
    queue: wgpu::Queue,
    frame: u64,
    stats: PassStats,
    evictions: u32,
    reuse: bool,
}

/// Pass bind groups and uniform buffers, reused by what they bind.
pub(crate) struct PassCache {
    table: RefCell<Table>,
}

impl PassCache {
    pub fn new(queue: wgpu::Queue) -> Self {
        Self {
            table: RefCell::new(Table {
                entries: Vec::new(),
                uniforms: Vec::new(),
                queue,
                frame: 0,
                stats: PassStats::default(),
                evictions: 0,
                reuse: true,
            }),
        }
    }

    /// Start a frame: the counts report this frame only, the entries carry over
    /// unless they have gone [`MAX_IDLE_FRAMES`] undrawn.
    pub fn begin_frame(&self) {
        let mut table = self.table.borrow_mut();
        table.frame += 1;
        table.retire();
        for uniform in &mut table.uniforms {
            uniform.used = false;
        }
        let evictions = table.evictions;
        table.stats = PassStats {
            entries: table.entries.len() as u32,
            uniform_slots: table.uniforms.len() as u32,
            evictions,
            ..PassStats::default()
        };
    }

    /// What the passes since [`begin_frame`](Self::begin_frame) cost.
    pub fn stats(&self) -> PassStats {
        let table = self.table.borrow();
        PassStats {
            entries: table.entries.len() as u32,
            ..table.stats
        }
    }

    /// Draw every pass with its own bind group. Diagnostic only: it exists so
    /// the batching measurement can report both numbers from one run. The
    /// table and its eviction count describe one another, so both reset.
    pub fn set_reuse(&self, reuse: bool) {
        let mut table = self.table.borrow_mut();
        table.reuse = reuse;
        table.entries.clear();
        table.evictions = 0;
        table.stats.evictions = 0;
    }

    /// The bind group for one pass, built only if this exact binding is not
    /// already in the table.
    ///
    /// The returned borrow lasts until it is dropped, so a caller holds it for
    /// its render pass and no longer: a second lookup while one is alive is a
    /// double borrow of the table.
    pub(crate) fn bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        label: &'static str,
        bounds: &[Bound<'_>],
    ) -> RefMut<'_, wgpu::BindGroup> {
        let mut table = self.table.borrow_mut();
        table.stats.passes += 1;
        let index = match table.find_exact(layout, bounds) {
            Some(index) => {
                table.reserve_entry_uniforms(index);
                table.stats.reused += 1;
                index
            }
            None => {
                let uniform_slots = table.prepare_uniforms(device, bounds);
                match table.find_binding(layout, bounds, &uniform_slots) {
                    Some(index) => {
                        table.stats.reused += 1;
                        index
                    }
                    None => {
                        table.stats.created += 1;
                        table.insert(device, layout, label, bounds, &uniform_slots)
                    }
                }
            }
        };
        let frame = table.frame;
        table.entries[index].used = frame;
        RefMut::map(table, |table| &mut table.entries[index].bind_group)
    }
}

impl Table {
    fn find_exact(&self, layout: &wgpu::BindGroupLayout, bounds: &[Bound<'_>]) -> Option<usize> {
        if !self.reuse {
            return None;
        }
        self.entries.iter().position(|entry| {
            &entry.layout == layout
                && entry.slots.len() == bounds.len()
                && entry
                    .slots
                    .iter()
                    .zip(bounds)
                    .all(|(slot, bound)| slot.matches_exact(bound, &self.uniforms))
        })
    }

    fn find_binding(
        &self,
        layout: &wgpu::BindGroupLayout,
        bounds: &[Bound<'_>],
        uniform_slots: &[Option<usize>; MAX_BOUND_SLOTS],
    ) -> Option<usize> {
        if !self.reuse {
            return None;
        }
        self.entries.iter().position(|entry| {
            &entry.layout == layout
                && entry.slots.len() == bounds.len()
                && entry
                    .slots
                    .iter()
                    .zip(bounds)
                    .enumerate()
                    .all(|(binding, (slot, bound))| {
                        slot.matches_binding(bound, uniform_slots[binding])
                    })
        })
    }

    fn reserve_entry_uniforms(&mut self, index: usize) {
        for slot in &self.entries[index].slots {
            if let Slot::Uniform(index) = slot {
                self.uniforms[*index].used = true;
            }
        }
    }

    fn prepare_uniforms(
        &mut self,
        device: &wgpu::Device,
        bounds: &[Bound<'_>],
    ) -> [Option<usize>; MAX_BOUND_SLOTS] {
        assert!(
            bounds.len() <= MAX_BOUND_SLOTS,
            "pass layout exceeds the fixed binding census"
        );
        let mut prepared = [None; MAX_BOUND_SLOTS];
        for (binding, bound) in bounds.iter().enumerate() {
            let Bound::Uniform(bytes) = bound else {
                continue;
            };
            assert!(
                !bytes.is_empty() && bytes.len().is_multiple_of(4),
                "uniform payloads must be non-empty four-byte words"
            );
            if let Some(existing) = self
                .uniforms
                .iter()
                .position(|uniform| uniform.bytes.as_slice() == *bytes)
            {
                self.uniforms[existing].used = true;
                prepared[binding] = Some(existing);
                continue;
            }

            let slot = self
                .uniforms
                .iter()
                .position(|uniform| !uniform.used)
                .unwrap_or_else(|| {
                    self.uniforms.push(UniformSlot {
                        buffer: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("aria.pass.uniform"),
                            size: bytes.len().max(16).next_power_of_two() as u64,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        }),
                        capacity: bytes.len().max(16).next_power_of_two() as u64,
                        bytes: Vec::with_capacity(bytes.len().max(16).next_power_of_two()),
                        used: false,
                    });
                    self.stats.uniform_buffers_created += 1;
                    self.uniforms.len() - 1
                });
            let required = bytes.len().max(16).next_power_of_two() as u64;
            if self.uniforms[slot].capacity < required {
                let before = self.entries.len();
                self.entries.retain(|entry| {
                    !entry
                        .slots
                        .iter()
                        .any(|held| matches!(held, Slot::Uniform(index) if *index == slot))
                });
                self.evictions += (before - self.entries.len()) as u32;
                self.uniforms[slot].buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("aria.pass.uniform"),
                    size: required,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.uniforms[slot].capacity = required;
                self.uniforms[slot].bytes = Vec::with_capacity(required as usize);
                self.stats.uniform_buffers_created += 1;
            }

            let uniform = &mut self.uniforms[slot];
            uniform.bytes.clear();
            uniform.bytes.extend_from_slice(bytes);
            uniform.used = true;
            self.queue.write_buffer(&uniform.buffer, 0, bytes);
            self.stats.uniform_writes += 1;
            prepared[binding] = Some(slot);
        }
        self.stats.uniform_slots = self.uniforms.len() as u32;
        prepared
    }

    fn insert(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        label: &'static str,
        bounds: &[Bound<'_>],
        uniform_slots: &[Option<usize>; MAX_BOUND_SLOTS],
    ) -> usize {
        self.prune();
        let entries: Vec<wgpu::BindGroupEntry<'_>> = bounds
            .iter()
            .enumerate()
            .map(|(binding, bound)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: match bound {
                    Bound::Texture(view) => wgpu::BindingResource::TextureView(view),
                    Bound::Sampler(sampler) => wgpu::BindingResource::Sampler(sampler),
                    Bound::Uniform(_) => self.uniforms
                        [uniform_slots[binding].expect("one persistent slot per uniform binding")]
                    .buffer
                    .as_entire_binding(),
                },
            })
            .collect();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &entries,
        });
        drop(entries);
        self.entries.push(Entry {
            layout: layout.clone(),
            slots: bounds
                .iter()
                .enumerate()
                .map(|(binding, bound)| match bound {
                    Bound::Texture(view) => Slot::Texture((*view).clone()),
                    Bound::Sampler(sampler) => Slot::Sampler((*sampler).clone()),
                    Bound::Uniform(_) => {
                        Slot::Uniform(uniform_slots[binding].expect("one slot per uniform binding"))
                    }
                })
                .collect(),
            bind_group,
            used: self.frame,
        });
        self.entries.len() - 1
    }

    /// Drop what no recent frame has drawn with, so a scene change gives back
    /// the textures its entries hold.
    fn retire(&mut self) {
        let before = self.entries.len();
        let frame = self.frame;
        self.entries
            .retain(|entry| frame.saturating_sub(entry.used) <= MAX_IDLE_FRAMES);
        self.evictions += (before - self.entries.len()) as u32;
    }

    /// Make room for one entry: drop what this frame has not drawn with, and
    /// if this frame drew with all of them, drop everything.
    fn prune(&mut self) {
        if self.entries.len() < MAX_ENTRIES {
            return;
        }
        let before = self.entries.len();
        let frame = self.frame;
        self.entries.retain(|entry| entry.used == frame);
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.clear();
        }
        self.evictions += (before - self.entries.len()) as u32;
        self.stats.evictions = self.evictions;
    }
}
