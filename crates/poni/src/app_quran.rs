//! The Quran page's commands (`commands::QuranCmd`) and its recitation
//! (`services::quran`). A child of `app`. The surah list and a surah's words
//! and timing are read from the add-on's pack only when the page asks (one
//! zstd frame, a few ms), and its font with the first one. While it plays,
//! the position is read from the player on frames the island draws anyway.

use super::App;
use crate::commands::{Command, QuranCmd};
use crate::services;
use poni_core::addons::{Status, QURAN, QURAN_FONT_FILE, QURAN_PACK_FILE};
use poni_core::quran::{Pack, Surah, SurahInfo};

const UNREAD: &str = "The Quran's files didn't read. Switch it off and on in Add-ons.";
const OFFLINE: &str = "The recitation didn't play. Are you online?";

/// Surah `n` from the add-on's pack, and the surah list when `list`.
fn read(n: u16, list: bool) -> std::io::Result<(Option<Vec<SurahInfo>>, Surah)> {
    let mut pack = Pack::open(std::fs::File::open(services::addons::file_path(&QURAN, QURAN_PACK_FILE))?)?;
    let l = if list { Some(pack.surahs()?) } else { None };
    Ok((l, pack.surah(n)?))
}

impl App {
    pub(super) fn quran_cmd(&mut self, c: QuranCmd) {
        match c {
            QuranCmd::Open { surah, ayah } => self.quran_open(surah, ayah),
            QuranCmd::PlayPause if self.st.quran.playing => self.quran_pause(),
            QuranCmd::PlayPause => self.quran_play(),
            QuranCmd::Seek(ms) => {
                self.st.quran.pos_ms = ms;
                self.st.quran.pos_at = self.st.now;
                self.quran.seek(ms);
                self.quran_remember();
            }
        }
    }

    /// Show surah `n` from ayah `k`; playing, it plays on from there.
    fn quran_open(&mut self, n: u16, k: u16) {
        let n = n.clamp(1, 114);
        let q = &mut self.st.quran;
        let list = q.surahs.is_empty();
        if list || q.surah.as_ref().map(|s| s.n) != Some(n) {
            match read(n, list) {
                Ok((l, s)) => {
                    if let Some(l) = l {
                        q.surahs = l;
                    }
                    q.surah = Some(s);
                    q.error = None;
                }
                Err(e) => {
                    pwarn!("quran: surah {n} didn't read ({e})");
                    q.error = Some(UNREAD);
                    return;
                }
            }
        }
        if !self.gfx.has_quran_font() {
            match std::fs::read(services::addons::file_path(&QURAN, QURAN_FONT_FILE)) {
                Ok(bytes) => self.gfx.set_quran_font(Some(&bytes)),
                Err(e) => pwarn!("quran: its font didn't read ({e})"),
            }
        }
        let Some(s) = self.st.quran.surah.as_ref() else { return };
        let k = (k as usize).clamp(1, s.ayat.len());
        self.st.quran.pos_ms = s.ayat[k - 1].start();
        self.st.quran.pos_at = self.st.now;
        self.quran_remember();
        if self.st.quran.playing {
            self.quran_play();
        }
    }

    fn quran_play(&mut self) {
        let Some(s) = self.st.quran.surah.as_ref() else { return };
        let (url, n, ms) = (s.audio.clone(), s.n, self.st.quran.pos_ms);
        let q = &mut self.st.quran;
        match self.quran.play(&url, ms, &self.sink) {
            Ok(()) => {
                pinfo!("quran: playing surah {n} from {ms} ms");
                q.playing = true;
                q.error = None;
                q.pos_at = self.st.now;
            }
            Err(e) => {
                pwarn!("quran: {e}");
                q.playing = false;
                q.error = Some(OFFLINE);
            }
        }
    }

    fn quran_pause(&mut self) {
        self.quran_tick();
        self.quran.pause();
        self.st.quran.playing = false;
        self.quran_remember();
    }

    /// Keep the ayah on the page in settings, so the page opens there again.
    fn quran_remember(&mut self) {
        let q = &self.st.quran;
        let Some(s) = q.surah.as_ref() else { return };
        let at = [s.n, s.ayah_at(q.pos_ms) as u16];
        if self.st.settings.quran_at != at {
            self.st.settings.quran_at = at;
            self.st.cmd(Command::SaveSettings);
        }
    }

    /// Each frame (`App::tick`): while it plays, where the player is.
    pub(super) fn quran_tick(&mut self) {
        if !self.st.quran.playing {
            return;
        }
        if let Some(ms) = self.quran.position_ms() {
            self.st.quran.pos_ms = ms;
            self.st.quran.pos_at = self.st.now;
        }
    }

    /// The surah's recording ended: on to the next, and after An-Nas, stop.
    pub(super) fn quran_ended(&mut self) {
        let n = self.st.quran.surah.as_ref().map_or(114, |s| s.n);
        if n < 114 && self.st.quran.playing {
            self.quran_open(n + 1, 1);
        } else {
            self.quran.stop();
            self.st.quran.playing = false;
        }
        self.request_frame();
    }

    pub(super) fn quran_failed(&mut self) {
        pwarn!("quran: the recitation failed to play");
        self.quran.stop();
        self.st.quran.playing = false;
        self.st.quran.error = Some(OFFLINE);
        self.request_frame();
    }

    /// Self-test, the Quran page's scene (`on`): Al-Fatihah from the
    /// add-on's files in PONI_SELFTEST_QURAN, the third word of its second
    /// ayah being recited (paused, so it asks for no frames); without them,
    /// its download under way. Off in the other scenes.
    pub fn selftest_quran(&mut self, on: bool) {
        let dir = std::env::var_os("PONI_SELFTEST_QURAN").map(std::path::PathBuf::from).filter(|_| on);
        let status = match dir {
            Some(dir) => {
                if self.st.quran.surah.is_none() {
                    let read = || -> std::io::Result<_> {
                        let mut pack = Pack::open(std::fs::File::open(dir.join(QURAN_PACK_FILE))?)?;
                        Ok((pack.surahs()?, pack.surah(1)?, std::fs::read(dir.join(QURAN_FONT_FILE))?))
                    };
                    match read() {
                        Ok((list, s, font)) => {
                            self.st.quran.surahs = list;
                            self.st.quran.surah = Some(s);
                            self.gfx.set_quran_font(Some(&font));
                        }
                        Err(e) => pwarn!("selftest: the Quran's files didn't read ({e})"),
                    }
                }
                if let Some(a) = self.st.quran.surah.as_ref().map(|s| &s.ayat[1]) {
                    self.st.quran.pos_ms = a.segs[2][1] + 100;
                }
                Status::Ready
            }
            None if on => Status::Downloading { done: 512_000, total: QURAN.size() },
            None => Status::Off,
        };
        self.st.quran.playing = false;
        self.st.addons.insert(QURAN.id, status);
    }

    /// The add-on went off: the recitation stops and its data and font go.
    pub(super) fn quran_off(&mut self) {
        self.quran.stop();
        self.st.quran = Default::default();
        self.gfx.set_quran_font(None);
    }
}
