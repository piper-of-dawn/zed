//! Jump-label overlay rendering. Self-contained so the upstream `editor.rs`
//! and `element.rs` only need a single `mod`/use line each — keeps merges with
//! upstream clean.

use gpui::{
    App, Bounds, Corners, Edges, Entity, Hsla, PaintQuad, Pixels, Point, TextAlign, TextRun,
    Window, fill, point, px, size, transparent_black,
};
use theme::ActiveTheme as _;
use util::ResultExt as _;

use std::ops::Range;
use std::rc::Rc;

use crate::{
    DisplayPoint, DisplayRow, Editor, EditorStyle, RowExt, element::PositionMap,
    scroll::ScrollPixelOffset,
};

pub const JUMP_TOGGLE_OVERLAY_OPACITY: f32 = 0.4;

#[derive(Debug, Clone, PartialEq)]
pub struct JumpLabel {
    pub position: DisplayPoint,
    pub label: String,
    pub match_length: usize,
}

pub fn paint_jump_labels(
    editor: &Entity<Editor>,
    style: &EditorStyle,
    position_map: &Rc<PositionMap>,
    visible_display_row_range: &Range<DisplayRow>,
    content_origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let jump_labels = editor.read(cx).jump_labels.clone();
    if jump_labels.is_empty() {
        return;
    }

    let font_id = style.text.font();
    let font_size = style.text.font_size.to_pixels(window.rem_size());
    let line_height = position_map.line_height;
    let scroll_position = position_map.scroll_position;
    let scroll_pixel_position = position_map.scroll_pixel_position;

    let theme = cx.theme().colors();
    let text_bounds = position_map.text_hitbox.bounds;
    let mut overlay = theme.panel_overlay_hover;
    overlay.fade_out(JUMP_TOGGLE_OVERLAY_OPACITY);
    window.paint_quad(fill(text_bounds, overlay));

    let primary_fg: Hsla = theme.text;
    let label_bg: Hsla = theme.text_accent;
    let label_fg: Hsla = theme.editor_background;

    for jump_label in &jump_labels {
        let row = jump_label.position.row();
        let column = jump_label.position.column();

        if row < visible_display_row_range.start || row >= visible_display_row_range.end {
            continue;
        }

        let line_layout = &position_map.line_layouts
            [(row.0 - visible_display_row_range.start.0) as usize];

        let match_start_x = line_layout.x_for_index(column as usize);
        let match_end_x = line_layout.x_for_index(column as usize + jump_label.match_length);
        let y: Pixels = ((row.as_f64() - scroll_position.y)
            * ScrollPixelOffset::from(line_height))
        .into();
        let match_start_screen_x =
            Pixels::from(ScrollPixelOffset::from(match_start_x) - scroll_pixel_position.x);
        let match_screen_width =
            Pixels::from(ScrollPixelOffset::from(match_end_x - match_start_x));

        let outline_inset = px(1.0);
        let outline_origin =
            content_origin + point(match_start_screen_x - outline_inset, y + outline_inset);
        let outline_size = size(
            match_screen_width + outline_inset * 2.0,
            line_height - outline_inset * 2.0,
        );
        window.paint_quad(PaintQuad {
            bounds: Bounds {
                origin: outline_origin,
                size: outline_size,
            },
            corner_radii: Corners::all(px(2.0)),
            background: transparent_black().into(),
            border_widths: Edges::all(px(1.0)),
            border_color: primary_fg,
            border_style: gpui::BorderStyle::Solid,
        });

        let label_run = TextRun {
            len: jump_label.label.len(),
            font: font_id.clone(),
            color: label_fg,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let shaped_label = window.text_system().shape_line(
            jump_label.label.clone().into(),
            font_size,
            &[label_run],
            None,
        );

        let label_text_width = shaped_label.width;
        let label_padding_x = px(3.0);
        let label_box_width = label_text_width + label_padding_x * 2.0;
        let label_box_height = line_height * 0.65;

        // Float the label at the top-right corner of the match, sticking up
        // into the line above so it doesn't obscure the underlying text.
        let label_bg_origin = content_origin
            + point(
                match_start_screen_x + match_screen_width - label_box_width * 0.5,
                y - label_box_height * 0.5,
            );
        let label_origin = label_bg_origin + point(label_padding_x, (label_box_height - line_height) * 0.5);

        window.paint_quad(PaintQuad {
            bounds: Bounds {
                origin: label_bg_origin,
                size: size(label_box_width, label_box_height),
            },
            corner_radii: Corners::all(px(2.0)),
            background: label_bg.into(),
            border_widths: Edges::default(),
            border_color: transparent_black(),
            border_style: gpui::BorderStyle::Solid,
        });

        shaped_label
            .paint(label_origin, line_height, TextAlign::Left, None, window, cx)
            .log_err();
    }
}
