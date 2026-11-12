//! Signing in to the account (`poni_core::oyenza`), where a page needs it:
//! a card that says what signing in brings with its button, and, while a
//! code waits, the code itself with Open and Cancel. The IELTS page shows
//! them signed out; the account row in Settings is `settings::draw_account`.

use crate::commands::{Command, OyenzaCmd};
use crate::gfx::TextStyle;
use crate::icons::Icon;
use crate::state::{AppState, OyenzaLink};
use crate::ui::{wid, PillStyle, Ui, PILL_GAP, PILL_SMALL};
use poni_core::geom::{palette, Point, Rect};
use poni_core::oyenza;

/// How tall the card and the code are, so starting to sign in doesn't move
/// the island.
pub const HEIGHT: f32 = 156.0;

/// The app's icon beside what signing in brings, and the button; while a
/// code waits, the code in its place ([`draw_code`]).
pub(crate) fn sign_in_card(ui: &mut Ui, st: &mut AppState, area: Rect, title: &str, body: &str, id: &str) {
    if let OyenzaLink::Connecting { code, url, until } = st.oyenza.link.clone() {
        draw_code(ui, st, area, &code, &url, until);
        return;
    }
    const ICON: f32 = 64.0;
    const GAP: f32 = 20.0;
    let x = area.x + ICON + GAP;
    let w = (area.w - ICON - GAP).clamp(0.0, 420.0);
    let title_st = TextStyle::headline().wrap().top();
    let body_st = TextStyle::footnote().color(palette::TEXT_DIM).wrap().top();
    let (_, title_h) = ui.c.measure(title, &title_st, w);
    let (_, body_h) = ui.c.measure(body, &body_st, w);
    // Text and button as one block, centred on the page like the icon.
    let block = title_h + 3.0 + body_h + 14.0 + PILL_SMALL;
    let mut y = area.y + ((area.h - block) / 2.0).max(0.0);
    crate::oyen::app_icon(ui.c, Point::new(area.x + ICON / 2.0, area.y + area.h / 2.0), ICON);
    ui.c.text(title, Rect::new(x, y, w, title_h + 1.0), &title_st);
    y += title_h + 3.0;
    ui.c.text(body, Rect::new(x, y, w, body_h + 1.0), &body_st);
    y += body_h + 14.0;
    let label = format!("Sign in to {}", oyenza::NAME);
    let bw = ui.pill_width(&label, false, true).min(w);
    if ui.pill(wid(id), Rect::new(x, y, bw, PILL_SMALL), &label, None, PillStyle::Primary(st.accent)).clicked {
        st.cmd(Command::Oyenza(OyenzaCmd::SignIn));
    }
}

/// The 8-letter code to approve on the web, Open (the browser opens the
/// approval page with the code filled in), Cancel, and how long it works.
pub(crate) fn draw_code(ui: &mut Ui, st: &mut AppState, area: Rect, code: &str, url: &str, until: f64) {
    ui.c.text(&format!("Sign in to {}", oyenza::NAME), Rect::new(area.x, area.y, area.w, 20.0), &TextStyle::headline());
    let device = if cfg!(windows) { "this PC" } else { "this Mac" };
    let line = format!("On your phone or {device}, open {} and approve this code.", oyenza::APPROVE_PAGE);
    ui.c.text(&line, Rect::new(area.x, area.y + 22.0, area.w, 18.0), &TextStyle::footnote().color(palette::TEXT_DIM));
    let code_r = Rect::new(area.x, area.y + 50.0, area.w, 64.0);
    ui.c.fill_rrect(code_r, 14.0, palette::SURFACE);
    ui.c.text(code, code_r, &TextStyle::display().tabular().center());

    let y = area.y + 128.0;
    let open_label = format!("Open {}", oyenza::NAME);
    let open_w = ui.pill_width(&open_label, true, true);
    let open = Rect::new(area.x, y, open_w, PILL_SMALL);
    if ui.pill(wid("account.open"), open, &open_label, Some(Icon::Open), PillStyle::Primary(st.accent)).clicked {
        st.cmd(Command::OpenUrl(url.to_string()));
    }
    let cancel_w = ui.pill_width("Cancel", false, true);
    let cancel = Rect::new(open.right() + PILL_GAP, y, cancel_w, PILL_SMALL);
    if ui.pill(wid("account.cancel"), cancel, "Cancel", None, PillStyle::Utility).clicked {
        st.cmd(Command::Oyenza(OyenzaCmd::Cancel));
    }
    let left = format!("Code works for {}", oyenza::code_time_left(until - st.now));
    let x = cancel.right() + 12.0;
    ui.c.text(&left, Rect::new(x, y, (area.right() - x).max(0.0), PILL_SMALL), &TextStyle::footnote().color(palette::TEXT_DIM).right());
}

/// The code's countdown moves each second while one waits.
pub fn next_wake(st: &AppState) -> Option<f64> {
    matches!(st.oyenza.link, OyenzaLink::Connecting { .. }).then(|| st.now.floor() + 1.0)
}
