//! The Quran page: an ayah at a time in large Arabic, the word being recited
//! lit up, Kemenag's translation under it, and the recitation's controls,
//! laid out like the Music tab (`pages::media`). The words and timing come
//! from the Quran add-on (`poni_core::quran`), read by the app (`app_quran`)
//! when the page asks; the recitation streams only while it plays.
//!
//! Nothing runs for it while it is closed. Open and playing, it draws when
//! the word on screen changes ([`next_wake`]), not every frame. The mouse
//! wheel over the surah's tile and name moves to another surah.

use super::media::{play_button, side_button};
use crate::commands::{Command, QuranCmd};
use crate::gfx::TextStyle;
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, Ui};
use poni_core::addons::{Status, QURAN};
use poni_core::geom::{palette, Point, Rect};

const HEIGHT: f32 = 236.0;
/// The surah's tile (its Arabic name), where Music has the cover.
const TILE: f32 = 72.0;
const TILE_RADIUS: f32 = 14.0;
const GAP: f32 = 16.0;
const TITLE_Y: f32 = -4.0;
const TITLE_H: f32 = 22.0;
const RECITER_Y: f32 = 18.0;
const LINE_H: f32 = 18.0;
const BAR_Y: f32 = 46.0;
const BAR_H: f32 = 4.0;
const CONTROLS_CY: f32 = 76.0;
const ARABIC_Y: f32 = 106.0;
const ARABIC_H: f32 = 82.0;
const ARABIC_SIZE: f32 = 28.0;
const MEANING_Y: f32 = 192.0;
const MEANING_H: f32 = 44.0;
/// Never sooner or later than this between frames while it plays: the
/// next word, or the bar's next step.
const WAKE_MS: (u32, u32) = (40, 1000);
pub const RECITER: &str = "Mishary Alafasy";

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

/// While it plays: when the word on screen next changes (the app asks only
/// while the page is open).
pub fn next_wake(st: &AppState) -> Option<f64> {
    let q = &st.quran;
    let s = q.surah.as_ref().filter(|_| q.playing)?;
    let ms = q.ms_at(st.now);
    let next = s.next_change(ms).unwrap_or(ms + WAKE_MS.1);
    Some(st.now + f64::from((next - ms).clamp(WAKE_MS.0, WAKE_MS.1)) / 1000.0)
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let page = Rect::new(area.x, area.y, area.w, HEIGHT);
    let status = st.addon(QURAN.id);
    if status != Status::Ready {
        super::empty_state(ui, page, Icon::Book, "Quran", &status.line(&QURAN, false));
        return;
    }
    let Some(s) = st.quran.surah.as_ref() else {
        if st.quran.error.is_none() {
            let [surah, ayah] = st.settings.quran_at;
            st.cmd(Command::Quran(QuranCmd::Open { surah, ayah }));
        }
        super::empty_state(ui, page, Icon::Book, "Quran", st.quran.error.unwrap_or("Opening\u{2026}"));
        return;
    };
    let (q, accent) = (&st.quran, st.accent);
    let n = s.n;
    let ms = q.ms_at(st.now);
    let k = s.ayah_at(ms);
    let ayah = &s.ayat[k - 1];
    // Before its first word (just opened, or skipped to), nothing is lit.
    let word = if ms > ayah.start() { ayah.word_at(ms) } else { 0 };
    let mut cmds = Vec::new();

    // The surah's tile and name; the wheel over them moves surah.
    let (x0, y0) = (area.x, area.y);
    let tile = Rect::new(x0, y0, TILE, TILE);
    let mid = Rect::new(x0 + TILE + GAP, y0, area.w - TILE - GAP, TILE + 24.0);
    let info = st.quran.surahs.get(n as usize - 1);
    let wheel = ui.wheel_in(Rect::new(x0, y0, TILE + GAP + mid.w, BAR_Y - 4.0));
    if wheel != 0.0 {
        let to = if wheel > 0.0 { n.saturating_sub(1).max(1) } else { (n + 1).min(114) };
        if to != n {
            cmds.push(QuranCmd::Open { surah: to, ayah: 1 });
        }
    }
    ui.c.fill_rrect(tile, TILE_RADIUS, accent.mul_alpha(0.16));
    if let Some(i) = info {
        ui.c.text(&i.arab, tile, &TextStyle::new(20.0).color(accent).center());
    }
    let name = info.map_or_else(|| format!("Surah {n}"), |i| i.nama.clone());
    ui.c.text(&format!("{name} \u{b7} Ayat {k}/{}", s.ayat.len()), Rect::new(mid.x, y0 + TITLE_Y, mid.w, TITLE_H), &TextStyle::headline());
    let (line, color) = match q.error {
        Some(e) => (e.to_string(), palette::ORANGE),
        None => (info.map_or(RECITER.to_string(), |i| format!("{} \u{b7} {RECITER}", i.arti)), palette::TEXT_DIM),
    };
    ui.c.text(&line, Rect::new(mid.x, y0 + RECITER_Y, mid.w, LINE_H), &TextStyle::body().color(color));

    // Where the recitation is in the surah; drag or click to move.
    let end = s.end_ms().max(1);
    let bar = Rect::new(mid.x, y0 + BAR_Y, mid.w, BAR_H);
    let hover = ui.hovered(Rect::new(bar.x, bar.y - 6.0, bar.w, bar.h + 12.0));
    let scrub = ui.progress_bar(wid("quran.seek"), bar, (ms as f32 / end as f32).min(1.0), accent, true);
    if let Some(f) = scrub.released {
        cmds.push(QuranCmd::Seek((f * end as f32) as u32));
    }
    if hover || scrub.held.is_some() {
        let f = scrub.held.unwrap_or(ms as f32 / end as f32).min(1.0);
        ui.c.fill_circle(Point::new(bar.x + bar.w * f, bar.y + bar.h / 2.0), 6.0, palette::TEXT);
    }

    // Previous and next ayah (on into the surah before or after), play.
    let (cx, cy) = (mid.x + mid.w / 2.0, y0 + CONTROLS_CY);
    let spacing = ((mid.w - 38.0) / 2.0).clamp(40.0, 60.0);
    if side_button(ui, wid("quran.prev"), Point::new(cx - spacing, cy), Icon::Prev, k > 1 || n > 1) {
        cmds.push(if k > 1 { QuranCmd::Seek(s.ayat[k - 2].start()) } else { QuranCmd::Open { surah: n - 1, ayah: 1 } });
    }
    if play_button(ui, Point::new(cx, cy), q.playing, true) {
        cmds.push(QuranCmd::PlayPause);
    }
    if side_button(ui, wid("quran.next"), Point::new(cx + spacing, cy), Icon::Next, k < s.ayat.len() || n < 114) {
        cmds.push(if k < s.ayat.len() { QuranCmd::Seek(s.ayat[k].start()) } else { QuranCmd::Open { surah: n + 1, ayah: 1 } });
    }

    // The ayah: the word being recited in the accent, the words still to
    // come dimmer, the ones said in full white.
    let text = ayah.text();
    let mut spans = Vec::new();
    if let Some((at, len)) = ayah.word_span(word) {
        spans.push((at, len, accent));
        spans.push((at + len, (text.encode_utf16().count() as u32).saturating_sub(at + len), palette::TEXT_DIM));
    }
    ui.c.quran_text(&text, Rect::new(area.x, y0 + ARABIC_Y, area.w, ARABIC_H), ARABIC_SIZE, palette::TEXT, &spans);

    // Its meaning, smaller when long; what still doesn't fit is cut.
    let meaning = Rect::new(area.x, y0 + MEANING_Y, area.w, MEANING_H);
    let mut ts = TextStyle::footnote().color(palette::TEXT_DIM).center().wrap().top();
    if ui.c.measure(&ayah.translation, &ts, meaning.w).1 > meaning.h {
        ts = TextStyle::caption().color(palette::TEXT_DIM).center().wrap().top();
    }
    ui.c.clip(meaning);
    ui.c.text(&ayah.translation, meaning, &ts);
    ui.c.unclip();

    if scrub.held.is_some() {
        ui.request_anim();
    }
    for c in cmds {
        st.cmd(Command::Quran(c));
    }
}
