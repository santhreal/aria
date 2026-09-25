//! Every [`TextRenderer`] method: font loading, shaping, and rasterizing.
//!
//! A pure move out of `lib.rs` to stay under the file ceiling. Inherent methods
//! belong to the type rather than to the file, so no caller changes and the
//! private memo keys, ink boxes and glyph writer stay where they were — this is
//! a child of the module that defines them and reaches them by name.

use super::*;

impl TextRenderer {
    /// A renderer with isolated shaping and raster caches over the process's
    /// parsed host font catalog.
    pub fn new() -> Self {
        Self {
            font_system: font_system(),
            swash_cache: SwashCache::new(),
            shape_memo: HashMap::new(),
            raster_memo: HashMap::new(),
        }
    }

    /// Add a font from raw TTF/OTF bytes (e.g. a bundled face) to the set.
    ///
    /// Returns the number of font faces actually added — `0` means the bytes
    /// were not a parseable font (`fontdb` drops bad data silently, so this
    /// count is the only load-time failure signal; a face collection can add
    /// more than one). Invalidates the memo caches, since a new face can
    /// change shaping or fallback for any string.
    pub fn load_font(&mut self, data: Vec<u8>) -> usize {
        let before = self.font_system.db().len();
        self.font_system.db_mut().load_font_data(data);
        self.shape_memo.clear();
        self.raster_memo.clear();
        self.font_system.db().len().saturating_sub(before)
    }

    /// Number of font faces available for shaping. Zero means no glyphs can be
    /// produced (no system fonts and none loaded).
    pub fn font_count(&self) -> usize {
        self.font_system.db().len()
    }

    /// Shape `text` with `style` into positioned per-cluster bitmaps.
    ///
    /// The full string is shaped once (correct kerning/ligatures/BiDi), then
    /// each cluster's glyphs are rasterized into their own ink-tight image.
    /// [`TextStyle::padding`] is **not** applied here — it is bitmap headroom
    /// for [`rasterize`](Self::rasterize); animated cluster quads overlap
    /// freely and need none.
    ///
    /// Results are memoized: repeating a call with identical input skips
    /// shaping entirely and costs one copy-out of the cached clusters, so
    /// per-frame animators can call this unconditionally.
    pub fn shape(&mut self, text: &str, style: &TextStyle) -> ShapedText {
        let key = ShapeKey::new(text, style);
        if let Some(hit) = self.shape_memo.get(&key) {
            return hit.clone();
        }
        let shaped = self.shape_uncached(text, style);
        Self::memo_insert(&mut self.shape_memo, key, shaped.clone());
        shaped
    }

    fn shape_uncached(&mut self, text: &str, style: &TextStyle) -> ShapedText {
        let buffer = self.layout(text, style);

        // Source byte offset of each buffer line. `LineIter` is the exact
        // split `Buffer::set_text` uses, so indices line up with `line_i`.
        let line_starts: Vec<usize> = LineIter::new(text).map(|(range, _)| range.start).collect();

        // Pass A: walk the laid-out glyphs, group them into shaping clusters,
        // and record every glyph's draw box (the swash image is cached, so
        // pass B's second lookup is a map hit).
        let mut clusters: Vec<PendingCluster> = Vec::new();
        for (visual_line, run) in buffer.layout_runs().enumerate() {
            let line_start = line_starts.get(run.line_i).copied().unwrap_or(0);
            for glyph in run.glyphs {
                // Mirrors cosmic-text's `Buffer::render`: the physical
                // position bakes in the baseline; the image placement then
                // offsets left/up from it.
                let phys = glyph.physical((0., run.line_y), 1.0);
                let ink = self
                    .swash_cache
                    .get_image(&mut self.font_system, phys.cache_key)
                    .as_ref()
                    .filter(|img| img.placement.width > 0 && img.placement.height > 0)
                    .map(|img| InkBox {
                        left: phys.x + img.placement.left,
                        top: phys.y - img.placement.top,
                        width: img.placement.width,
                        height: img.placement.height,
                    });

                let range = line_start + glyph.start..line_start + glyph.end;
                // Glyphs of one cluster (base + combining marks, or a single
                // ligature glyph) are adjacent within a run, so comparing to
                // the last cluster suffices.
                let same_cluster = clusters
                    .last()
                    .is_some_and(|c| c.line == visual_line && c.range == range);
                if !same_cluster {
                    clusters.push(PendingCluster {
                        line: visual_line,
                        range,
                        pen_x: glyph.x,
                        line_top: run.line_top,
                        baseline_y: run.line_y,
                        glyphs: Vec::new(),
                        ink: None,
                    });
                }
                let cluster = clusters.last_mut().expect("just ensured non-empty");
                if let Some(b) = ink {
                    cluster.ink = Some(match cluster.ink {
                        None => b.bounds(),
                        Some(u) => union(u, b.bounds()),
                    });
                }
                cluster.glyphs.push(PendingGlyph {
                    key: phys.cache_key,
                    ink,
                });
            }
        }

        // Run-wide ink box; every offset/extent is relative to its origin, so
        // alignment shifts baked into glyph positions normalize out and the
        // bitmap stays tight no matter how lines are aligned.
        let global = clusters.iter().filter_map(|c| c.ink).reduce(union);
        let (gx, gy, extent) = match global {
            Some((l, t, r, b)) => (l, t, ((r - l) as u32, (b - t) as u32)),
            None => (0, 0, (0, 0)),
        };

        // Pass B: rasterize each cluster's glyphs into its own tight image.
        let style_alpha = u32::from(style.color[3]);
        let mut out = Vec::with_capacity(clusters.len());
        for c in clusters {
            let (image, offset) = match c.ink {
                Some((l, t, r, b)) => {
                    let (w, h) = ((r - l) as u32, (b - t) as u32);
                    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
                    for g in &c.glyphs {
                        let Some(ink) = g.ink else { continue };
                        if let Some(img) = self
                            .swash_cache
                            .get_image(&mut self.font_system, g.key)
                            .as_ref()
                        {
                            write_glyph(
                                &mut pixels,
                                w,
                                ink.left - l,
                                ink.top - t,
                                img,
                                style.color,
                                style_alpha,
                            );
                        }
                    }
                    (
                        RgbaImage::new(w, h, pixels),
                        [(l - gx) as f32, (t - gy) as f32],
                    )
                }
                // No coverage (whitespace): keep the slot, at the pen position.
                None => (
                    RgbaImage::transparent(0, 0),
                    [c.pen_x - gx as f32, c.line_top - gy as f32],
                ),
            };
            out.push(ClusterBox {
                text_range: c.range,
                line: c.line,
                offset,
                baseline: c.baseline_y - gy as f32,
                image,
            });
        }

        // Visual order (per run) -> logical order: line, then byte position.
        // This is the reveal order a typewriter should follow, also for RTL.
        out.sort_by_key(|c| (c.line, c.text_range.start));

        ShapedText {
            extent,
            clusters: out,
        }
    }

    /// Shape `text` with `style` and rasterize it to a single straight-alpha
    /// [`RgbaImage`], sized to the **ink extents** of the laid-out text plus
    /// [`TextStyle::padding`] on every side.
    ///
    /// Text with no coverage — empty or whitespace-only — yields a zero-area
    /// image regardless of padding.
    ///
    /// Results are memoized: repeating a call with identical input costs one
    /// bitmap copy, not a re-shape — safe to call every frame.
    pub fn rasterize(&mut self, text: &str, style: &TextStyle) -> RgbaImage {
        let key = RasterKey::new(text, style);
        if let Some(hit) = self.raster_memo.get(&key) {
            return hit.clone();
        }
        let image = self.rasterize_uncached(text, style);
        Self::memo_insert(&mut self.raster_memo, key, image.clone());
        image
    }

    fn rasterize_uncached(&mut self, text: &str, style: &TextStyle) -> RgbaImage {
        // Lift the shaped run out of the memo instead of asking `shape` for an
        // owned copy: painting only reads it, and a run holds one bitmap per
        // cluster, so the copy was the largest allocation on a raster miss. It
        // goes straight back, so the entry survives for the next caller.
        let key = ShapeKey::new(text, style);
        let shaped = match self.shape_memo.remove(&key) {
            Some(hit) => hit,
            None => self.shape_uncached(text, style),
        };
        // Stroke / background / shadow (and plain padding) are painted here —
        // `shape` stays ink-tight so cluster animation can place glyphs freely.
        let image = if shaped.has_ink() {
            effects::paint(&shaped, style)
        } else {
            RgbaImage::transparent(0, 0)
        };
        Self::memo_insert(&mut self.shape_memo, key, shaped);
        image
    }

    /// Insert into a memo map, bounding memory with a wholesale clear at the
    /// cap. Frame loops keep a handful of live keys, so an LRU would buy
    /// nothing; the clear only ever costs one re-shape per entry after it.
    fn memo_insert<K: Eq + Hash, V>(memo: &mut HashMap<K, V>, key: K, value: V) {
        const MEMO_CAP: usize = 64;
        if memo.len() >= MEMO_CAP {
            memo.clear();
        }
        memo.insert(key, value);
    }

    /// Memo entry counts `(shape, rasterize)` — test observability.
    ///
    /// Crate-visible because the cases live beside `lib.rs`, not in this file.
    #[cfg(test)]
    pub(crate) fn memo_sizes(&self) -> (usize, usize) {
        (self.shape_memo.len(), self.raster_memo.len())
    }

    /// Build and lay out a buffer for `text` + `style` (the single shaping
    /// path both `shape` and `rasterize` go through).
    fn layout(&mut self, text: &str, style: &TextStyle) -> Buffer {
        let metrics = Metrics::new(style.font_size.max(1.0), style.line_height.max(1.0));
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        // Width constraint drives word wrapping; height is unbounded so every
        // line is laid out (and thus measurable).
        buffer.set_size(&mut self.font_system, style.max_width, None);

        let family = match &style.family {
            FontFamily::SansSerif => Family::SansSerif,
            FontFamily::Serif => Family::Serif,
            FontFamily::Monospace => Family::Monospace,
            FontFamily::Named(name) => Family::Name(name),
        };
        let font_size = style.font_size.max(1.0);
        let tracking_px = if style.letter_spacing.is_finite() {
            style.letter_spacing.clamp(-4096.0, 4096.0)
        } else {
            0.0
        };
        let attrs = Attrs::new()
            .family(family)
            .weight(if style.bold {
                Weight::BOLD
            } else {
                Weight::NORMAL
            })
            .style(if style.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            })
            // cosmic-text expresses tracking in em, while Aria exposes px.
            .letter_spacing(tracking_px / font_size);
        let align = match style.align {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        };
        buffer.set_text(
            &mut self.font_system,
            text,
            &attrs,
            Shaping::Advanced,
            Some(align),
        );

        // Alignment needs a container width: without one, cosmic-text aligns
        // each paragraph against its own width (offset 0 — a no-op). Measure,
        // then relayout at the widest line so Center/Right align lines against
        // each other. The +1 slack keeps the widest line from re-wrapping;
        // ink-extent measurement trims the slack back off the bitmap.
        if style.max_width.is_none() && style.align != TextAlign::Left {
            let widest = buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0f32, f32::max);
            if widest > 0.0 && buffer.layout_runs().count() > 1 {
                buffer.set_size(&mut self.font_system, Some(widest.ceil() + 1.0), None);
            }
        }

        buffer
    }
}
