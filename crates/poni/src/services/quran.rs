//! The Quran page's recitation: a surah's file streamed from
//! quranicaudio.com with WinRT's `MediaPlayer`, only while it plays. Like
//! the adzan it has no media session: it isn't a song, so neither the Music
//! page (and its lyrics search) nor Windows' volume flyout picks it up.
//!
//! `AppEvent::QuranEnded` comes when a surah's recording ends,
//! `AppEvent::QuranFailed` when it can't play (offline), from a thread-pool
//! thread. Nothing of it runs at rest: no player until the first Play, and
//! a paused one only holds its buffer.

use crate::events::{AppEvent, EventSink};
use windows::core::HSTRING;
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};

/// TimeSpan counts 100 ns.
const TICKS_PER_MS: i64 = 10_000;

#[derive(Default)]
pub struct QuranPlayer {
    player: Option<MediaPlayer>,
    /// The recording it holds.
    url: String,
}

impl QuranPlayer {
    /// Play `url` from `ms`: a new recording from there, or the one it holds
    /// on from where it is (moved to `ms` when that is more than a moment off).
    pub fn play(&mut self, url: &str, ms: u32, sink: &EventSink) -> Result<(), String> {
        if self.player.is_none() || self.url != url {
            self.open(url, ms, sink)?;
        } else if self.position_ms().is_some_and(|at| at.abs_diff(ms) > 400) {
            self.seek(ms);
        }
        self.player.as_ref().map_or(Ok(()), |p| p.Play().map_err(|e| format!("can't play ({e})")))
    }

    fn open(&mut self, url: &str, ms: u32, sink: &EventSink) -> Result<(), String> {
        self.stop();
        let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(|e| format!("bad address ({e})"))?;
        let player = MediaPlayer::new().map_err(|e| format!("no media player ({e})"))?;
        if let Ok(cm) = player.CommandManager() {
            let _ = cm.SetIsEnabled(false);
        }
        let source = MediaSource::CreateFromUri(&uri).map_err(|e| format!("can't open the recording ({e})"))?;
        // From `ms` once it opens. (A playback item's start time would trim
        // the recording, and its position would count from there.)
        if ms > 0 {
            let _ = player.MediaOpened(&TypedEventHandler::new(move |p: &Option<MediaPlayer>, _| {
                if let Some(s) = p.as_ref().and_then(|p| p.PlaybackSession().ok()) {
                    let _ = s.SetPosition(TimeSpan { Duration: ms as i64 * TICKS_PER_MS });
                }
                Ok(())
            }));
        }
        player.SetSource(&source).map_err(|e| format!("can't open the recording ({e})"))?;
        let ended = sink.clone();
        let _ = player.MediaEnded(&TypedEventHandler::new(move |_, _| {
            ended.send(AppEvent::QuranEnded);
            Ok(())
        }));
        let failed = sink.clone();
        let _ = player.MediaFailed(&TypedEventHandler::new(move |_, _| {
            failed.send(AppEvent::QuranFailed);
            Ok(())
        }));
        self.player = Some(player);
        self.url = url.to_string();
        Ok(())
    }

    pub fn pause(&self) {
        if let Some(p) = &self.player {
            let _ = p.Pause();
        }
    }

    pub fn seek(&self, ms: u32) {
        if let Some(s) = self.player.as_ref().and_then(|p| p.PlaybackSession().ok()) {
            let _ = s.SetPosition(TimeSpan { Duration: ms as i64 * TICKS_PER_MS });
        }
    }

    /// Where the recording is, in ms (None without one, or while it opens:
    /// it reads 0 until it has moved to where it was asked to start).
    pub fn position_ms(&self) -> Option<u32> {
        let s = self.player.as_ref()?.PlaybackSession().ok()?;
        if matches!(s.PlaybackState().ok()?, MediaPlaybackState::None | MediaPlaybackState::Opening) {
            return None;
        }
        let at = s.Position().ok()?;
        Some((at.Duration / TICKS_PER_MS).clamp(0, u32::MAX as i64) as u32)
    }

    /// Let the recording go (the add-on off, it failed, the last surah ended).
    pub fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            let _ = p.Pause();
            let _ = p.Close();
        }
        self.url.clear();
    }
}
