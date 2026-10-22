//! A visual row whose offsets remain in source UTF-8 bytes.
use gpui::{App, Pixels, Point, ShapedLine, SharedString, TextAlign, Window, point, px};
use std::ops::Range;

use super::bidi::BidiRow;

pub(crate) struct InlineFragment {
    pub(crate) range: Range<usize>,
    pub(crate) x: Pixels,
    pub(crate) width: Pixels,
    /// The glyphs painted for this fragment: its own source text, or, for an
    /// atomic fragment, the text drawn in place of the source. `None` leaves
    /// the room to an element painted over it.
    pub(crate) text: Option<ShapedLine>,
    /// Whether the fragment is one unit for the caret. Offsets inside it
    /// resolve to its edges, and its glyphs do not index its source.
    pub(crate) atomic: bool,
}

impl InlineFragment {
    /// The shaped source text, when offsets index into it.
    fn source_text(&self) -> Option<&ShapedLine> {
        self.text.as_ref().filter(|_| !self.atomic)
    }
}

pub(crate) struct InputLine {
    pub(crate) len: usize,
    pub(crate) width: Pixels,
    pub(crate) text: SharedString,
    content: Content,
    /// Caret geometry for a plain row holding right-to-left text.
    bidi: Option<BidiRow>,
}
// Keep the ordinary shaped row inline: boxing it would add an allocation to
// every existing plain-text row. Only token rows allocate fragment storage.
#[allow(clippy::large_enum_variant)]
enum Content {
    Text(ShapedLine),
    Inline(Vec<InlineFragment>),
}
impl From<ShapedLine> for InputLine {
    fn from(line: ShapedLine) -> Self {
        Self {
            len: line.len,
            width: line.width,
            text: line.text.clone(),
            bidi: BidiRow::new(&line),
            content: Content::Text(line),
        }
    }
}
impl InputLine {
    pub(crate) fn inline(text: SharedString, fragments: Vec<InlineFragment>) -> Self {
        let width = fragments.last().map_or(px(0.), |f| f.x + f.width);
        Self {
            len: text.len(),
            text,
            width,
            content: Content::Inline(fragments),
            bidi: None,
        }
    }
    pub(crate) fn x_for_index(&self, ix: usize) -> Pixels {
        match &self.content {
            Content::Text(line) => self
                .bidi
                .as_ref()
                .map_or_else(|| line.x_for_index(ix), |bidi| bidi.x_for_index(ix)),
            Content::Inline(fragments) => {
                for f in fragments {
                    if ix < f.range.end {
                        return f.x
                            + f.source_text().map_or(
                                if ix <= f.range.start { px(0.) } else { f.width },
                                |line| shaped_x_for_index(line, ix.saturating_sub(f.range.start)),
                            );
                    }
                }
                self.width
            }
        }
    }

    /// The left and right x of the glyphs drawing `range`, which a selection or
    /// highlight on this row covers.
    pub(crate) fn x_span(&self, range: Range<usize>) -> (Pixels, Pixels) {
        if let Some((left, right)) = self
            .bidi
            .as_ref()
            .and_then(|bidi| bidi.x_span(range.clone()))
        {
            return (left, right);
        }
        let start = self.x_for_index(range.start);
        let end = self.x_for_index(range.end);
        (start.min(end), start.max(end))
    }
    pub(crate) fn closest_index_for_x(&self, x: Pixels) -> usize {
        match &self.content {
            Content::Text(line) => self.bidi.as_ref().map_or_else(
                || line.closest_index_for_x(x),
                |bidi| bidi.closest_index_for_x(&line.text, x),
            ),
            Content::Inline(fragments) => {
                for f in fragments {
                    if x <= f.x + f.width {
                        return f.source_text().map_or(
                            if x - f.x < f.width / 2. {
                                f.range.start
                            } else {
                                f.range.end
                            },
                            |line| {
                                f.range.start
                                    + BidiRow::new(line).map_or_else(
                                        || line.closest_index_for_x(x - f.x),
                                        |bidi| bidi.closest_index_for_x(&line.text, x - f.x),
                                    )
                            },
                        );
                    }
                }
                self.len
            }
        }
    }
    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        match &self.content {
            Content::Text(line) => self
                .bidi
                .as_ref()
                .map_or_else(|| line.index_for_x(x), |bidi| bidi.index_for_x(x)),
            Content::Inline(_) => {
                (x >= px(0.) && x <= self.width).then(|| self.closest_index_for_x(x))
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, false, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_background(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, true, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_pass(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        background: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let paint = |line: &ShapedLine, pos, align, width, window: &mut Window, cx: &mut App| {
            if background {
                let _ = line.paint_background(pos, height, align, width, window, cx);
            } else {
                let _ = line.paint(pos, height, align, width, window, cx);
            }
        };
        match &self.content {
            Content::Text(line) => paint(line, pos, align, width, window, cx),
            Content::Inline(fragments) => {
                let remaining = (width.unwrap_or(self.width) - self.width).max(px(0.));
                let offset = match align {
                    TextAlign::Right => remaining,
                    TextAlign::Center => remaining / 2.,
                    _ => px(0.),
                };
                for f in fragments {
                    if let Some(line) = &f.text {
                        paint(
                            line,
                            pos + point(offset + f.x, px(0.)),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                }
            }
        }
    }
}

/// `x_for_index` for a token row's fragment, which is shaped on its own.
fn shaped_x_for_index(line: &ShapedLine, ix: usize) -> Pixels {
    BidiRow::new(line).map_or_else(|| line.x_for_index(ix), |bidi| bidi.x_for_index(ix))
}
