//! Caret geometry for shaped rows that hold right-to-left text.
//!
//! Every text system lays a row's glyphs out in visual order, but gpui's
//! `x_for_index` and `closest_index_for_x` read them as if visual order and byte
//! order agree, which only holds for left-to-right text. In an Arabic or Hebrew
//! run the caret for a byte offset sits on the right edge of the character after
//! it, and a click on the left half of a glyph lands after that character. This
//! answers those questions the way a browser text field does.
use gpui::{Pixels, ShapedLine, px};
use std::ops::Range;
use unicode_bidi::{BidiClass, BidiInfo, Direction};

/// Whether `text` holds any character that can start a right-to-left run.
pub(crate) fn has_rtl(text: &str) -> bool {
    !text.is_ascii()
        && text.chars().any(|c| {
            matches!(
                unicode_bidi::bidi_class(c),
                BidiClass::R
                    | BidiClass::AL
                    | BidiClass::AN
                    | BidiClass::RLE
                    | BidiClass::RLO
                    | BidiClass::RLI
            )
        })
}

/// Whether a paragraph reads right to left, decided by its first strong
/// character as `dir="auto"` does on the web.
pub(crate) fn is_rtl_paragraph(text: &str) -> bool {
    !text.is_ascii() && unicode_bidi::get_base_direction(text) == Direction::Rtl
}

/// One glyph cluster of a row: the source bytes it draws and where it is drawn.
#[derive(Debug, Clone, PartialEq)]
struct Cluster {
    range: Range<usize>,
    left: Pixels,
    right: Pixels,
    rtl: bool,
}

impl Cluster {
    /// The x of the boundary `ix` bytes into this cluster, interpolated across a
    /// ligature such as lam-alef that draws several characters as one glyph.
    fn x_at(&self, ix: usize) -> Pixels {
        let len = self.range.len().max(1) as f32;
        let fraction = (ix.saturating_sub(self.range.start)).min(self.range.len()) as f32 / len;
        let width = self.right - self.left;
        if self.rtl {
            self.right - width * fraction
        } else {
            self.left + width * fraction
        }
    }
}

/// The clusters of one shaped row in visual order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BidiRow {
    clusters: Vec<Cluster>,
    width: Pixels,
}

impl BidiRow {
    /// Returns `None` for a row without right-to-left text, whose glyphs are in
    /// byte order and need none of this.
    pub(crate) fn new(line: &ShapedLine) -> Option<Self> {
        let text = line.text.as_ref();
        if !has_rtl(text) || line.len != text.len() {
            return None;
        }

        // The left edge of each cluster: its leftmost glyph, so a mark drawn over
        // its base does not split it.
        let mut starts: Vec<(usize, Pixels)> = Vec::new();
        for glyph in line.runs.iter().flat_map(|run| run.glyphs.iter()) {
            match starts.iter_mut().find(|(index, _)| *index == glyph.index) {
                Some((_, left)) => *left = (*left).min(glyph.position.x),
                None => starts.push((glyph.index, glyph.position.x)),
            }
        }
        if starts.is_empty() {
            return None;
        }

        let mut byte_ends: Vec<usize> = starts.iter().map(|(index, _)| *index).collect();
        byte_ends.sort_unstable();

        let info = BidiInfo::new(text, None);
        let mut levels = Vec::with_capacity(text.len());
        for paragraph in &info.paragraphs {
            let paragraph_levels = info.reordered_levels(paragraph, paragraph.range.clone());
            levels.extend_from_slice(&paragraph_levels[paragraph.range.clone()]);
        }

        starts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let clusters = starts
            .iter()
            .enumerate()
            .map(|(ix, (start, left))| {
                let end = byte_ends
                    .iter()
                    .find(|end| **end > *start)
                    .copied()
                    .unwrap_or(text.len());
                let right = starts.get(ix + 1).map_or(line.width, |(_, next)| *next);
                Cluster {
                    range: *start..end,
                    left: *left,
                    right: right.max(*left),
                    rtl: levels.get(*start).is_some_and(|level| level.is_rtl()),
                }
            })
            .collect();

        Some(Self {
            clusters,
            width: line.width,
        })
    }

    fn cluster_containing(&self, ix: usize) -> Option<&Cluster> {
        self.clusters
            .iter()
            .find(|cluster| cluster.range.start <= ix && ix < cluster.range.end)
    }

    /// The caret x for the boundary before byte `ix`. The caret follows the
    /// character before it, so it stays beside the letter just typed.
    pub(crate) fn x_for_index(&self, ix: usize) -> Pixels {
        if ix == 0 {
            return self.cluster_containing(0).map_or(px(0.), |cluster| {
                if cluster.rtl {
                    cluster.right
                } else {
                    cluster.left
                }
            });
        }
        match self.cluster_containing(ix - 1) {
            Some(cluster) => cluster.x_at(ix),
            None => self.width,
        }
    }

    /// The boundary closest to `x`.
    pub(crate) fn closest_index_for_x(&self, text: &str, x: Pixels) -> usize {
        let (Some(first), Some(last)) = (self.clusters.first(), self.clusters.last()) else {
            return 0;
        };
        let cluster = if x < first.left {
            first
        } else {
            self.clusters
                .iter()
                .find(|cluster| x < cluster.right)
                .unwrap_or(last)
        };

        let width = cluster.right - cluster.left;
        let mut fraction = if width > px(0.) {
            ((x - cluster.left) / width).clamp(0., 1.)
        } else {
            0.
        };
        if cluster.rtl {
            fraction = 1. - fraction;
        }

        let range = cluster.range.clone();
        let target = range.start as f32 + fraction * range.len() as f32;
        text.get(range.clone())
            .into_iter()
            .flat_map(|chars| chars.char_indices().map(|(ix, _)| range.start + ix))
            .chain([range.end])
            .min_by(|a, b| {
                (*a as f32 - target)
                    .abs()
                    .total_cmp(&(*b as f32 - target).abs())
            })
            .unwrap_or(range.start)
    }

    /// The start of the character drawn under `x`, or `None` outside the row.
    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        if x < px(0.) || x >= self.width {
            return None;
        }
        self.clusters
            .iter()
            .find(|cluster| cluster.left <= x && x < cluster.right)
            .map(|cluster| cluster.range.start)
    }

    /// The horizontal extent of the glyphs drawing `range`. In a row that mixes
    /// directions a byte range can be split in two visually; the extent covers
    /// both pieces.
    pub(crate) fn x_span(&self, range: Range<usize>) -> Option<(Pixels, Pixels)> {
        self.clusters
            .iter()
            .filter(|cluster| cluster.range.start < range.end && range.start < cluster.range.end)
            .fold(None, |span, cluster| {
                let (left, right) =
                    if cluster.range.start >= range.start && cluster.range.end <= range.end {
                        (cluster.left, cluster.right)
                    } else {
                        let a = cluster.x_at(range.start.max(cluster.range.start));
                        let b = cluster.x_at(range.end.min(cluster.range.end));
                        (a.min(b), a.max(b))
                    };
                Some(match span {
                    None => (left, right),
                    Some((span_left, span_right)) => (left.min(span_left), right.max(span_right)),
                })
            })
    }
}
