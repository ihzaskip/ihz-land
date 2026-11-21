//! Text page, the Text extractor add-on's (off until switched on): Extract
//! text (the island closes, then the screen freezes for a box to be drawn;
//! `services::textgrab`), how to do the same from any app with
//! Alt+Shift+T, and the last few texts read (`Store::text_history`), newest
//! first: a click copies one again.
//!
//! Nothing here moves by itself; the page draws only when the app does.

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, wid_n, PillStyle, Ui, PILL_REGULAR, PILL_SMALL};
use poni_core::geom::{palette, Rect};

/// How many texts the page keeps (and shows).
pub const HISTORY: usize = 5;
/// A text longer than this is kept cut short (the clipboard got it whole).
pub const KEEP_CHARS: usize = 4000;
const ROW_H: f32 = 30.0;
const LIST_Y: f32 = 78.0;
const HEIGHT: f32 = LIST_Y + HISTORY as f32 * ROW_H;
const KEYS: [&str; 3] = ["Alt", "Shift", "T"];

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    // Extract text, then the shortcut as keycaps and how to cancel.
    let label = "Extract text";
    let bw = ui.pill_width(label, true, false);
    if ui.pill(wid("text.extract"), Rect::new(area.x, area.y, bw, PILL_REGULAR), label, Some(Icon::Area), PillStyle::Primary(palette::BLUE)).clicked {
        st.cmd(Command::ExtractText);
    }
    let cap = TextStyle::caption().weight(Weight::Semibold).color(palette::TEXT);
    let dim = TextStyle::caption().color(palette::TEXT_DIM);
    let mut x = area.x + bw + 20.0;
    let key_y = area.y + (PILL_REGULAR - 20.0) / 2.0;
    ui.c.text("or press", Rect::new(x, key_y, 60.0, 20.0), &dim);
    x += ui.c.measure("or press", &dim, 200.0).0 + 8.0;
    for (i, k) in KEYS.iter().enumerate() {
        let w = ui.c.measure(k, &cap, 100.0).0 + 14.0;
        let r = Rect::new(x, key_y, w, 20.0);
        ui.c.fill_rrect(r, 5.0, palette::SURFACE_HOVER);
        ui.c.text(k, r, &cap.clone().center());
        x += w;
        if i + 1 < KEYS.len() {
            ui.c.text("+", Rect::new(x, key_y, 14.0, 20.0), &dim.clone().center());
            x += 14.0;
        }
    }
    ui.c.text(
        "in any app, then drag a box over the text. Esc cancels.",
        Rect::new(x + 8.0, key_y, (area.right() - x - 8.0).max(0.0), 20.0),
        &dim,
    );

    // The last texts read, a click copies one again.
    let head_y = area.y + LIST_Y - 30.0;
    ui.c.text("RECENT", Rect::new(area.x, head_y, 120.0, PILL_SMALL), &TextStyle::caption().weight(Weight::Bold).color(palette::TEXT_DIM));
    if st.store.text_history.is_empty() {
        let line = "Text you extract shows here, newest first. Click one to copy it again.";
        ui.c.text(line, Rect::new(area.x, area.y + LIST_Y, area.w, ROW_H), &TextStyle::footnote().color(palette::TEXT_FAINT));
        return;
    }
    let cw = ui.pill_width("Clear", false, true);
    if ui.pill(wid("text.clear"), Rect::new(area.right() - cw, head_y, cw, PILL_SMALL), "Clear", None, PillStyle::Utility).clicked {
        st.store.text_history.clear();
        st.save();
        return;
    }
    let mut copy = None;
    for (i, text) in st.store.text_history.iter().enumerate().take(HISTORY) {
        let r = Rect::new(area.x - 8.0, area.y + LIST_Y + i as f32 * ROW_H, area.w + 16.0, ROW_H - 2.0);
        let resp = ui.button(wid_n("text.row", i as u64), r);
        if resp.hovered || resp.pressed {
            ui.c.fill_rrect(r, 8.0, if resp.pressed { palette::SURFACE_PRESSED } else { palette::SURFACE_HOVER });
        }
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
        let side = if resp.hovered { "Copy".to_string() } else if lines > 1 { format!("{lines} lines") } else { String::new() };
        let side_w = if side.is_empty() { 0.0 } else { 70.0 };
        ui.c.text(first, Rect::new(r.x + 8.0, r.y, r.w - 24.0 - side_w, r.h), &TextStyle::footnote().color(palette::TEXT));
        if !side.is_empty() {
            let color = if resp.hovered { palette::BLUE } else { palette::TEXT_FAINT };
            ui.c.text(&side, Rect::new(r.right() - 8.0 - side_w, r.y, side_w, r.h), &TextStyle::caption().weight(Weight::Semibold).color(color).right());
        }
        if resp.clicked {
            copy = Some(i);
        }
    }
    if let Some(i) = copy {
        st.cmd(Command::CopyText(i));
    }
}
