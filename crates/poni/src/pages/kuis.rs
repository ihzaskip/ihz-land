//! Kuis page, the Kuis add-on's (off until switched on): one JISPA math
//! question at a time from ihz.app (`poni_core::kuis`).
//!
//! - Signed out: the sign-in card.
//! - The question: its topic, the lead line, the formula, five choices.
//!   A click marks it: the right choice green, a wrong pick red, then
//!   ihz.app's explanation and Soal berikutnya.
//! - A question on its way, or why none came with Coba lagi.
//!
//! The formulas are SVG from ihz.app, drawn by Direct2D (`Canvas::svg`).
//! Nothing moves here, and the page asks for a question only when it opens
//! without one or on a button (`Command::KuisNext`).

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::state::{AppState, OyenzaLink};
use crate::ui::{wid, wid_n, PillStyle, Ui, PILL_SMALL};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::kuis::{svg_size, Question};

/// Every state is this tall (under System's 256), so the island never jumps.
const HEIGHT: f32 = 248.0;
const SIGNED_OUT_H: f32 = super::account::HEIGHT;
const ACCENT: Color = palette::BLUE;
/// The question's formula: at most this tall, and at most this much larger
/// than ihz.app drew it (16 px text).
const PROMPT_H: f32 = 64.0;
const PROMPT_UP: f32 = 1.25;
const CHOICE_H: f32 = 50.0;
const CHOICE_GAP: f32 = 8.0;
const LETTERS: [&str; 6] = ["A", "B", "C", "D", "E", "F"];
const SIGN_IN_TITLE: &str = "Kuis matematika JISPA";
const SIGN_IN_BODY: &str = "Soal pilihan ganda JISPA satu per satu, lengkap dengan penjelasannya. Masuk ke ihz.app dulu buat mulai.";

pub fn height(st: &AppState) -> f32 {
    if signed_in(st) {
        HEIGHT
    } else {
        SIGNED_OUT_H
    }
}

fn signed_in(st: &AppState) -> bool {
    matches!(st.oyenza.link, OyenzaLink::Connected { .. })
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    if !signed_in(st) {
        let card = Rect::new(area.x, area.y, area.w, SIGNED_OUT_H);
        super::account::sign_in_card(ui, st, card, SIGN_IN_TITLE, SIGN_IN_BODY, "kuis.sign-in");
        return;
    }
    if st.kuis.wants_first() {
        st.cmd(Command::KuisNext);
    }
    let area = Rect::new(area.x, area.y, area.w, HEIGHT);
    let Some(q) = st.kuis.question.clone() else {
        draw_empty(ui, st, area);
        return;
    };

    // The topic, the lead line and the formula.
    let head = format!("SOAL \u{b7} {}", q.topic_label.to_uppercase());
    ui.c.text(&head, Rect::new(area.x, area.y, area.w, 16.0), &TextStyle::caption().weight(Weight::Bold).color(ACCENT));
    let mut y = area.y + 20.0;
    if let Some(lead) = q.lead.as_deref().filter(|l| !l.is_empty()) {
        ui.c.text(lead, Rect::new(area.x, y, area.w, 18.0), &TextStyle::footnote().color(palette::TEXT_DIM));
        y += 20.0;
    }
    if let Some(size) = svg_size(&q.prompt) {
        let s = (area.w / size.0).min(PROMPT_H / size.1).min(PROMPT_UP);
        let at = Point::new(area.x, y + (PROMPT_H - size.1 * s) / 2.0);
        ui.c.svg(&q.prompt, size, at, s, palette::TEXT);
    }

    // The choices, in one row.
    let top = area.y + 112.0;
    let n = q.choices.len() as f32;
    let w = (area.w - (n - 1.0) * CHOICE_GAP) / n;
    for (i, svg) in q.choices.iter().enumerate() {
        let r = Rect::new(area.x + i as f32 * (w + CHOICE_GAP), top, w, CHOICE_H);
        if choice(ui, st, &q, i, svg, r) {
            st.kuis.picked = Some(i);
        }
    }

    // How it went, ihz.app's explanation, and the next one.
    let foot_y = area.bottom() - PILL_SMALL;
    let busy = st.kuis.loading;
    let label = if busy { "Ngambil soal\u{2026}" } else { "Soal berikutnya" };
    let pw = ui.pill_width(label, false, true);
    let pill = Rect::new(area.right() - pw, foot_y, pw, PILL_SMALL);
    if busy {
        ui.c.fill_rrect(pill, pill.h / 2.0, palette::SURFACE);
        ui.c.text(label, pill, &TextStyle::footnote().weight(Weight::Semibold).color(palette::DISABLED).center());
    } else {
        let style = if st.kuis.picked.is_some() { PillStyle::Primary(ACCENT) } else { PillStyle::Utility };
        if ui.pill(wid("kuis.next"), pill, label, None, style).clicked {
            st.cmd(Command::KuisNext);
        }
    }
    let text_w = area.w - pw - 16.0;
    let y = top + CHOICE_H + 12.0;
    let (line, color) = match (st.kuis.problem, st.kuis.right()) {
        (Some(p), _) => (p.line().to_string(), palette::ORANGE),
        (None, Some(true)) => ("Benar!".to_string(), palette::GREEN),
        (None, Some(false)) => (format!("Belum tepat. Jawabannya {}.", LETTERS.get(q.answer).unwrap_or(&"?")), palette::ORANGE),
        (None, None) => ("Pilih satu jawaban.".to_string(), palette::TEXT_FAINT),
    };
    ui.c.text(&line, Rect::new(area.x, y, area.w, 18.0), &TextStyle::footnote().weight(Weight::Semibold).color(color));
    if let Some(note) = q.note.as_deref().filter(|_| st.kuis.picked.is_some()) {
        let r = Rect::new(area.x, y + 22.0, text_w, area.bottom() - y - 22.0);
        ui.c.clip(r);
        ui.c.text(note, r, &TextStyle::caption().color(palette::TEXT_DIM).wrap().top());
        ui.c.unclip();
    }
}

/// One choice: its letter and formula; marked once something is picked.
/// Returns true when clicked (only before a pick).
fn choice(ui: &mut Ui, st: &AppState, q: &Question, i: usize, svg: &str, r: Rect) -> bool {
    let picked = st.kuis.picked;
    let resp = if picked.is_none() { ui.button(wid_n("kuis.choice", i as u64), r) } else { Default::default() };
    let (fill, ink) = match picked {
        Some(_) if i == q.answer => (palette::GREEN.with_alpha(0.22), palette::GREEN),
        Some(p) if p == i => (palette::RED.with_alpha(0.22), palette::RED),
        Some(_) => (palette::SURFACE, palette::TEXT_FAINT),
        None if resp.pressed => (palette::SURFACE_PRESSED, palette::TEXT_DIM),
        None if resp.hovered => (palette::SURFACE_HOVER, palette::TEXT_DIM),
        None => (palette::SURFACE, palette::TEXT_FAINT),
    };
    ui.c.fill_rrect(r, 12.0, fill);
    let letter = LETTERS.get(i).copied().unwrap_or("");
    ui.c.text(letter, Rect::new(r.x + 9.0, r.y + 5.0, 14.0, 13.0), &TextStyle::caption().weight(Weight::Bold).color(ink));
    if let Some(size) = svg_size(svg) {
        // Right of the letter, the same gap on the other side.
        let room = Rect::new(r.x + 22.0, r.y + 4.0, r.w - 44.0, r.h - 8.0);
        let s = (room.w / size.0).min(room.h / size.1).min(1.0);
        let at = Point::new(room.center().x - size.0 * s / 2.0, room.center().y - size.1 * s / 2.0);
        let color = if picked.is_some_and(|p| p != i) && i != q.answer { palette::TEXT_DIM } else { palette::TEXT };
        ui.c.svg(svg, size, at, s, color);
    }
    resp.clicked
}

/// No question yet: one on its way, or why none came.
fn draw_empty(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let mid = Rect::new(area.x, area.y + 70.0, area.w, 20.0);
    match st.kuis.problem {
        Some(p) if !st.kuis.loading => {
            ui.c.text(p.line(), mid, &TextStyle::footnote().color(palette::ORANGE).center());
            let w = ui.pill_width("Coba lagi", false, true);
            let r = Rect::new(area.center().x - w / 2.0, mid.bottom() + 14.0, w, PILL_SMALL);
            if ui.pill(wid("kuis.retry"), r, "Coba lagi", None, PillStyle::Primary(ACCENT)).clicked {
                st.cmd(Command::KuisNext);
            }
        }
        _ => ui.c.text("Ngambil soal\u{2026}", mid, &TextStyle::footnote().color(palette::TEXT_DIM).center()),
    }
}
