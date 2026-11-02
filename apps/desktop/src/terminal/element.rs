//! Paints the terminal grid.
//!
//! Per frame: lock the grid briefly to copy the visible cells into plain
//! structs, unlock, then build one shaped line per row (glyphs forced to the
//! cell width for exact column alignment) plus merged background rectangles.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point as GridPoint;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{CursorShape, Rgb};
use gpui_kit::*;

use super::colors::{self, BACKGROUND, CURSOR, FOREGROUND, SELECTION};
use super::{GridSize, Layout, TerminalView};

pub struct TerminalElement {
    view: Entity<TerminalView>,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>) -> Self {
        Self { view }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Rgb,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

struct Snapshot {
    /// Per row: (char, style, bg) per cell column.
    rows: Vec<Vec<(char, Style, Rgb)>>,
    cursor: Option<(usize, usize, CursorShape)>,
}

pub struct Frame {
    lines: Vec<(Pixels, ShapedLine)>,
    backgrounds: Vec<PaintQuad>,
    cursor_overlay: Vec<PaintQuad>,
    line_height: Pixels,
}

fn rgb_hsla(c: Rgb) -> Hsla {
    colors::to_hsla(c)
}

impl TerminalElement {
    fn snapshot(view: &TerminalView, focused: bool) -> Snapshot {
        let term = view.shared.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let (rows, cols) = (term.screen_lines(), term.columns());
        let default_style = Style {
            fg: FOREGROUND,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
        };
        let mut grid = vec![vec![(' ', default_style, BACKGROUND); cols]; rows];
        let selection = content.selection;
        let cursor_point = content.cursor.point;
        let cursor_shape = content.cursor.shape;

        for indexed in content.display_iter {
            let GridPoint { line, column } = indexed.point;
            let row = line.0 + offset;
            if row < 0 || row as usize >= rows || column.0 >= cols {
                continue;
            }
            let cell = indexed.cell;
            let mut fg = colors::resolve(cell.fg, content.colors);
            let mut bg = colors::resolve(cell.bg, content.colors);
            let flags = cell.flags;
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = Rgb {
                    r: fg.r / 3 * 2,
                    g: fg.g / 3 * 2,
                    b: fg.b / 3 * 2,
                };
            }
            if flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            if selection.is_some_and(|s| s.contains(indexed.point)) {
                bg = SELECTION;
            }
            let is_cursor = indexed.point == cursor_point && offset == 0;
            if is_cursor && focused && cursor_shape == CursorShape::Block {
                bg = CURSOR;
                fg = BACKGROUND;
            }
            // Wide-char spacers stay as blanks so later columns keep aligned.
            let blank = flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                || cell.c == '\0'
                || cell.c == '\t';
            let ch = if blank { ' ' } else { cell.c };
            let style = Style {
                fg,
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strike: flags.contains(Flags::STRIKEOUT),
            };
            grid[row as usize][column.0] = (ch, style, bg);
        }

        let cursor = (offset == 0 && cursor_shape != CursorShape::Hidden).then(|| {
            let row = cursor_point.line.0.max(0) as usize;
            (row, cursor_point.column.0, cursor_shape)
        });
        Snapshot { rows: grid, cursor }
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<Frame>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = gpui_kit::Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Frame> {
        let (font, font_size, focused) = {
            let v = self.view.read(cx);
            (v.font.clone(), v.font_size, v.focus.is_focused(window))
        };
        let ts = window.text_system().clone();
        let font_id = ts.resolve_font(&font);
        let cell_width = ts
            .advance(font_id, font_size, 'm')
            .map(|s| s.width)
            .unwrap_or(font_size * 0.6);
        let line_height = (font_size * 1.3).round();
        if cell_width <= px(0.0) || line_height <= px(0.0) {
            return None;
        }
        let cols = (bounds.size.width / cell_width).floor().max(1.0) as usize;
        let rows = (bounds.size.height / line_height).floor().max(1.0) as usize;
        let layout = Layout {
            origin: bounds.origin,
            cell_width,
            line_height,
        };
        self.view
            .update(cx, |v, _| v.set_size(GridSize { cols, rows }, layout));

        let snap = Self::snapshot(self.view.read(cx), focused);

        let bold = Font {
            weight: FontWeight::BOLD,
            ..font.clone()
        };
        let italic = Font {
            style: FontStyle::Italic,
            ..font.clone()
        };
        let bold_italic = Font {
            weight: FontWeight::BOLD,
            style: FontStyle::Italic,
            ..font.clone()
        };

        let mut lines = Vec::with_capacity(snap.rows.len());
        let mut backgrounds = Vec::new();
        for (r, row) in snap.rows.iter().enumerate() {
            let y = bounds.origin.y + line_height * r as f32;
            // Backgrounds: merge runs of equal color, skip the default.
            let mut c = 0;
            while c < row.len() {
                let bg = row[c].2;
                let start = c;
                while c < row.len() && row[c].2 == bg {
                    c += 1;
                }
                if bg != BACKGROUND {
                    backgrounds.push(fill(
                        Bounds::new(
                            point(bounds.origin.x + cell_width * start as f32, y),
                            size(cell_width * (c - start) as f32, line_height),
                        ),
                        rgb_hsla(bg),
                    ));
                }
            }
            // Text: one run per change of style; skip rows of pure blanks.
            if row.iter().all(|(ch, ..)| *ch == ' ') {
                continue;
            }
            let mut text = String::with_capacity(row.len());
            let mut runs: Vec<TextRun> = Vec::new();
            let mut current: Option<(Style, usize)> = None;
            let flush = |runs: &mut Vec<TextRun>, style: Style, len: usize| {
                let f = match (style.bold, style.italic) {
                    (true, true) => bold_italic.clone(),
                    (true, false) => bold.clone(),
                    (false, true) => italic.clone(),
                    _ => font.clone(),
                };
                let color = rgb_hsla(style.fg);
                runs.push(TextRun {
                    len,
                    font: f,
                    color,
                    background_color: None,
                    underline: style.underline.then_some(UnderlineStyle {
                        color: Some(color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    strikethrough: style.strike.then_some(StrikethroughStyle {
                        color: Some(color),
                        thickness: px(1.0),
                    }),
                });
            };
            for (ch, style, _) in row {
                let before = text.len();
                text.push(*ch);
                let added = text.len() - before;
                match current.as_mut() {
                    Some((s, len)) if s == style => *len += added,
                    _ => {
                        if let Some((s, len)) = current.take() {
                            flush(&mut runs, s, len);
                        }
                        current = Some((*style, added));
                    }
                }
            }
            if let Some((s, len)) = current {
                flush(&mut runs, s, len);
            }
            let shaped = ts.shape_line(text.into(), font_size, &runs, Some(cell_width));
            lines.push((y, shaped));
        }

        let mut cursor_overlay = Vec::new();
        if let Some((r, c, shape)) = snap.cursor {
            let x = bounds.origin.x + cell_width * c as f32;
            let y = bounds.origin.y + line_height * r as f32;
            let color = rgb_hsla(CURSOR);
            let thin = px(2.0);
            match (shape, focused) {
                (CursorShape::Beam, _) => cursor_overlay.push(fill(
                    Bounds::new(point(x, y), size(thin, line_height)),
                    color,
                )),
                (CursorShape::Underline, _) => cursor_overlay.push(fill(
                    Bounds::new(point(x, y + line_height - thin), size(cell_width, thin)),
                    color,
                )),
                // Focused block is drawn via the cell colors; unfocused or
                // hollow shows an outline.
                (CursorShape::Block, true) => {}
                _ => {
                    let one = px(1.0);
                    cursor_overlay.extend([
                        fill(Bounds::new(point(x, y), size(cell_width, one)), color),
                        fill(
                            Bounds::new(point(x, y + line_height - one), size(cell_width, one)),
                            color,
                        ),
                        fill(Bounds::new(point(x, y), size(one, line_height)), color),
                        fill(
                            Bounds::new(point(x + cell_width - one, y), size(one, line_height)),
                            color,
                        ),
                    ]);
                }
            }
        }
        Some(Frame {
            lines,
            backgrounds,
            cursor_overlay,
            line_height,
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Option<Frame>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        let Some(frame) = frame.take() else { return };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for q in frame.backgrounds {
                window.paint_quad(q);
            }
            for (y, line) in &frame.lines {
                let _ = line.paint(
                    point(bounds.origin.x, *y),
                    frame.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            for q in frame.cursor_overlay {
                window.paint_quad(q);
            }
        });
    }
}
