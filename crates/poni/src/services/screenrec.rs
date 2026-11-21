//! Built-in screen recorder: the island's screen to an MP4 in Videos, with
//! no other app. Windows.Graphics.Capture hands over each changed frame as a
//! GPU texture, and Media Foundation's sink writer turns it into H.264 on the
//! graphics card where it can (its own encoder otherwise).
//!
//! Sound: what the PC plays (WASAPI loopback) and, unless switched off in
//! the menu (`RecordSettings::microphone`), the default microphone,
//! opened the way the IELTS page opens it (`mic::open_endpoint`), mixed into
//! 48 kHz stereo and written as AAC next to the video. Each packet goes where
//! Windows' timestamp (QPC, the clock the video frames carry too) says, so a
//! pause in the sound becomes silence and the two stay in step. A device
//! that can't open (no microphone, privacy switch off) is left out.
//!
//! Nothing runs at rest: the capture and the `poni-screenrec-audio` thread
//! exist only between [`start`] and [`Recording::stop`]. Video works only
//! when the screen changes (Windows sends no frame for a still screen), at
//! most 30 frames a second. Windows draws its yellow capture border while
//! it records.

use crate::services::mic;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::{factory, Interface, HSTRING};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
use windows::Win32::Media::Audio::{eCapture, eRender, AUDCLNT_BUFFERFLAGS_SILENT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// 30 frames a second, in Media Foundation's 100 ns units.
const FPS: u32 = 30;
const FRAME: i64 = 10_000_000 / FPS as i64;
const SECOND: i64 = 10_000_000;
/// Sound: 48 kHz stereo, 16-bit into AAC at 192 kbit/s.
const RATE: i64 = 48_000;
/// The sound thread takes what the devices have this often, only while a
/// recording runs (shared-mode WASAPI as `mic.rs` reads it: a 500 ms
/// buffer, so a late wake loses nothing).
const AUDIO_POLL: Duration = Duration::from_millis(20);
/// Sound is written this far behind now, so packets that come a little
/// late still land in their place.
const LAG: i64 = SECOND / 5;
/// A packet more than this many frames (10 ms) from where its timestamp
/// says is moved there: a gap filled with silence, or a drift undone.
const SLACK: i64 = RATE / 100;

pub struct Recording {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    writer: Arc<Mutex<Option<Writer>>>,
    path: PathBuf,
    audio: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

struct Writer {
    sink: IMFSinkWriter,
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    w: u32,
    h: u32,
    /// The recording's zero, on the QPC clock in 100 ns.
    origin: i64,
    last: i64,
    frames: u64,
    /// The last time video was told it has a gap (`SendStreamTick`).
    ticked: i64,
}

// SAFETY: one thread at a time behind the Mutex; the device is free-threaded
// and its context multithread-protected (`SetMultithreadProtected`).
unsafe impl Send for Writer {}

type Shared = Arc<Mutex<Option<Writer>>>;

/// Now on the QPC clock, in 100 ns (the unit of Windows.Graphics.Capture's
/// and WASAPI's timestamps).
fn now_hns() -> i64 {
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    (c as i128 * SECOND as i128 / f.max(1) as i128) as i64
}

/// Start recording the screen `island` is on into `path` (an .mp4), with
/// the microphone (`mic`) and the PC's own sound (`sound`); neither: video only.
pub fn start(island: HWND, path: PathBuf, mic: bool, sound: bool) -> windows::core::Result<Recording> {
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE)? };
    // The sound devices first: whether any opened decides the file's streams.
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<(Shared, i64)>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new().name("poni-screenrec-audio".into()).spawn(move || audio_thread(ready_tx, go_rx, &stop2, mic, sound)).ok();
    let sound = thread.is_some() && ready_rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false);
    match unsafe { begin(island, path, sound) } {
        Ok((mut rec, origin)) => {
            if let (true, Some(thread)) = (sound, thread) {
                let _ = go_tx.send((rec.writer.clone(), origin));
                rec.audio = Some((stop, thread));
            }
            Ok(rec)
        }
        Err(e) => {
            unsafe {
                let _ = MFShutdown();
            }
            Err(e)
        }
    }
    // go_tx dropped unsent: the sound thread lets its devices go and ends.
}

unsafe fn begin(island: HWND, path: PathBuf, sound: bool) -> windows::core::Result<(Recording, i64)> {
    let mut device = None;
    let mut ctx = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        Some(&mut ctx),
    )?;
    let (device, ctx): (ID3D11Device, ID3D11DeviceContext) = (device.unwrap(), ctx.unwrap());
    // The capture's thread copies while the encoder works on the same device.
    let _ = device.cast::<ID3D11Multithread>()?.SetMultithreadProtected(true);
    let rt_device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&device.cast::<IDXGIDevice>()?)?.cast()?;

    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = interop.CreateForMonitor(MonitorFromWindow(island, MONITOR_DEFAULTTONEAREST))?;
    let size = item.Size()?;
    // H.264 wants even sides.
    let (w, h) = (size.Width as u32 & !1, size.Height as u32 & !1);

    let sink = sink_writer(&device, &path, w, h, sound)?;
    let origin = now_hns();
    let writer = Arc::new(Mutex::new(Some(Writer { sink, device, ctx, w, h, origin, last: 0, frames: 0, ticked: 0 })));

    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&rt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
    let shared = writer.clone();
    pool.FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, windows::core::IInspectable>::new(move |pool, _| {
        let Some(pool) = pool.as_ref() else { return Ok(()) };
        let frame = pool.TryGetNextFrame()?;
        if let Some(w) = shared.lock().unwrap().as_mut() {
            if let Err(e) = w.write(&frame) {
                crate::pwarn!("screen recorder: a frame was lost ({e})");
            }
        }
        Ok(())
    }))?;
    let session = pool.CreateCaptureSession(&item)?;
    session.StartCapture()?;
    Ok((Recording { session, pool, writer, path, audio: None }, origin))
}

/// Stream 0 is the video; stream 1, with `sound`, the AAC sound.
unsafe fn sink_writer(device: &ID3D11Device, path: &Path, w: u32, h: u32, sound: bool) -> windows::core::Result<IMFSinkWriter> {
    let mut token = 0;
    let mut manager = None;
    MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
    let manager = manager.unwrap();
    manager.ResetDevice(device, token)?;
    let mut attrs = None;
    MFCreateAttributes(&mut attrs, 2)?;
    let attrs = attrs.unwrap();
    attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
    attrs.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)?;
    let sink = MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, &attrs)?;

    let video = |subtype: &windows::core::GUID| -> windows::core::Result<IMFMediaType> {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        t.SetUINT64(&MF_MT_FRAME_SIZE, (u64::from(w) << 32) | u64::from(h))?;
        t.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(FPS) << 32) | 1)?;
        t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)?;
        Ok(t)
    };
    let out = video(&MFVideoFormat_H264)?;
    // About 8 Mbit/s for 1080p: sharp text, about 1 GB a quarter hour.
    out.SetUINT32(&MF_MT_AVG_BITRATE, w * h * 4)?;
    let stream = sink.AddStream(&out)?;
    sink.SetInputMediaType(stream, &video(&MFVideoFormat_ARGB32)?, None)?;

    if sound {
        let audio = |subtype: &windows::core::GUID| -> windows::core::Result<IMFMediaType> {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
            t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32)?;
            t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            Ok(t)
        };
        let out = audio(&MFAudioFormat_AAC)?;
        out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
        let stream = sink.AddStream(&out)?;
        let input = audio(&MFAudioFormat_PCM)?;
        input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
        input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE as u32 * 4)?;
        sink.SetInputMediaType(stream, &input, None)?;
    }
    sink.BeginWriting()?;
    Ok(sink)
}

impl Writer {
    unsafe fn write(&mut self, frame: &Direct3D11CaptureFrame) -> windows::core::Result<()> {
        let at = (frame.SystemRelativeTime()?.Duration - self.origin).max(0);
        if self.frames > 0 && at - self.last < FRAME {
            return Ok(()); // over 30 a second
        }
        let src: ID3D11Texture2D = frame.Surface()?.cast::<IDirect3DDxgiInterfaceAccess>()?.GetInterface()?;
        let mut sd = D3D11_TEXTURE2D_DESC::default();
        src.GetDesc(&mut sd);
        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.w,
            Height: self.h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            ..Default::default()
        };
        // ponytail: a new texture per frame (the encoder may still hold the
        // last one); a small ring of them if allocation ever shows in a profile.
        let mut tex = None;
        self.device.CreateTexture2D(&desc, None, Some(&mut tex))?;
        let tex = tex.unwrap();
        // A screen that changed size mid-recording: its top-left corner.
        let area = D3D11_BOX { left: 0, top: 0, front: 0, right: self.w.min(sd.Width), bottom: self.h.min(sd.Height), back: 1 };
        self.ctx.CopySubresourceRegion(&tex, 0, 0, 0, 0, &src, 0, Some(&area));

        let buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &tex, 0, false)?;
        buf.SetCurrentLength(buf.cast::<IMF2DBuffer>()?.GetContiguousLength()?)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buf)?;
        sample.SetSampleTime(at)?;
        sample.SetSampleDuration(FRAME)?;
        self.sink.WriteSample(0, &sample)?;
        self.last = at;
        self.frames += 1;
        Ok(())
    }

    /// 16-bit stereo `pcm` starting at frame `at` of the sound.
    unsafe fn write_audio(&mut self, pcm: &[u8], at: i64) -> windows::core::Result<()> {
        let frames = (pcm.len() / 4) as i64;
        let buf = MFCreateMemoryBuffer(pcm.len() as u32)?;
        let mut p = std::ptr::null_mut();
        buf.Lock(&mut p, None, None)?;
        std::ptr::copy_nonoverlapping(pcm.as_ptr(), p, pcm.len());
        buf.Unlock()?;
        buf.SetCurrentLength(pcm.len() as u32)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buf)?;
        sample.SetSampleTime(at * SECOND / RATE)?;
        sample.SetSampleDuration(frames * SECOND / RATE)?;
        self.sink.WriteSample(1, &sample)?;
        // A still screen sends no frames: say so, or the writer holds the
        // sound in memory waiting for video to interleave it with.
        let now = (at + frames) * SECOND / RATE;
        if now - self.last > SECOND && now - self.ticked > SECOND {
            self.sink.SendStreamTick(0, now)?;
            self.ticked = now;
        }
        Ok(())
    }
}

impl Recording {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stop and finish the file. Err: nothing worth keeping was written
    /// (the file is then removed).
    pub fn stop(mut self) -> windows::core::Result<PathBuf> {
        let _ = self.session.Close();
        let _ = self.pool.Close();
        if let Some((stop, thread)) = self.audio.take() {
            stop.store(true, Ordering::SeqCst);
            let _ = thread.join();
        }
        let writer = self.writer.lock().unwrap().take();
        let done = match writer {
            Some(w) if w.frames > 0 => unsafe { w.sink.Finalize() },
            _ => Err(windows::Win32::Foundation::E_FAIL.into()),
        };
        unsafe {
            let _ = MFShutdown();
        }
        if done.is_err() {
            let _ = std::fs::remove_file(&self.path);
        }
        done.map(|()| self.path)
    }
}

// ------------------------------------------------------------------ sound

/// One sound device: what it gave, at 48 kHz stereo, waiting to be mixed.
struct Source {
    dev: mic::Device,
    what: &'static str,
    resample: Resample,
    fifo: VecDeque<[f32; 2]>,
    /// The sound's frame number of `fifo`'s first frame.
    start: i64,
}

/// `poni-screenrec-audio`: open the devices, say whether any opened, then
/// (once the file is ready) mix until told to stop.
fn audio_thread(ready: mpsc::Sender<bool>, go: mpsc::Receiver<(Shared, i64)>, stop: &AtomicBool, with_mic: bool, with_sound: bool) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut sources: Vec<Source> = [(eRender, "screen recorder: system sound"), (eCapture, "screen recorder: microphone")]
        .into_iter()
        .filter(|&(flow, _)| if flow == eCapture { with_mic } else { with_sound })
        .filter_map(|(flow, what)| match mic::open_endpoint(flow, what) {
            Ok(dev) => Some(Source { resample: Resample::new(&dev), dev, what, fifo: VecDeque::new(), start: 0 }),
            Err(e) => {
                crate::pwarn!("{what}: left out ({e})");
                None
            }
        })
        .collect();
    let _ = ready.send(!sources.is_empty());
    if let Ok((writer, origin)) = go.recv() {
        for s in &sources {
            unsafe {
                let _ = s.dev.client.Start();
            }
        }
        mix(&mut sources, &writer, origin, stop);
        for s in &sources {
            unsafe {
                let _ = s.dev.client.Stop();
            }
        }
    }
    drop(sources);
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
}

fn mix(sources: &mut Vec<Source>, writer: &Mutex<Option<Writer>>, origin: i64, stop: &AtomicBool) {
    let mut written: i64 = 0;
    let mut pcm: Vec<u8> = Vec::new();
    loop {
        let last = stop.load(Ordering::SeqCst);
        let now = now_hns() - origin;
        sources.retain_mut(|s| match s.drain(origin, now) {
            Ok(()) => true,
            Err(e) => {
                crate::pwarn!("{}: stopped ({e})", s.what);
                false
            }
        });
        // At the end, everything up to now.
        let target = (if last { now } else { now - LAG }).max(0) * RATE / SECOND;
        if target > written {
            pcm.clear();
            for s in sources.iter_mut() {
                // What came too late for its place is dropped.
                let late = ((written - s.start).max(0) as usize).min(s.fifo.len());
                s.fifo.drain(..late);
                s.start = s.start.max(written);
            }
            for i in written..target {
                let mut f = [0f32; 2];
                for s in sources.iter_mut().filter(|s| s.start <= i) {
                    if let Some(x) = s.fifo.pop_front() {
                        f[0] += x[0];
                        f[1] += x[1];
                    }
                    s.start = i + 1;
                }
                for c in f {
                    pcm.extend_from_slice(&((c.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
                }
            }
            if let Some(w) = writer.lock().unwrap().as_mut() {
                if let Err(e) = unsafe { w.write_audio(&pcm, written) } {
                    crate::pwarn!("screen recorder: sound was lost ({e})");
                }
            }
            written = target;
        }
        if last {
            return;
        }
        std::thread::sleep(AUDIO_POLL);
    }
}

impl Source {
    /// Take every packet the device has, each placed where its timestamp says.
    fn drain(&mut self, origin: i64, now: i64) -> windows::core::Result<()> {
        loop {
            if unsafe { self.dev.capture.GetNextPacketSize()? } == 0 {
                return Ok(());
            }
            let (mut data, mut frames, mut flags, mut qpc) = (std::ptr::null_mut::<u8>(), 0u32, 0u32, 0u64);
            unsafe { self.dev.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))? };
            if qpc != 0 {
                let at = (qpc as i64 - origin).min(now) * RATE / SECOND;
                let end = self.start + self.fifo.len() as i64;
                if at > end + SLACK {
                    // ponytail: at most a second of silence at once; a packet
                    // further ahead than that is a broken timestamp.
                    self.fifo.extend(std::iter::repeat([0.0; 2]).take((at - end).min(RATE) as usize));
                } else if at < end - SLACK {
                    let cut = ((end - at) as usize).min(self.fifo.len());
                    self.fifo.truncate(self.fifo.len() - cut);
                    if self.fifo.is_empty() {
                        self.start = at;
                    }
                }
            }
            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                self.resample.silence(frames as usize, &mut self.fifo);
            } else {
                let bytes = unsafe { std::slice::from_raw_parts(data, frames as usize * self.dev.block) };
                self.resample.push(bytes, &mut self.fifo);
            }
            unsafe { self.dev.capture.ReleaseBuffer(frames)? };
        }
    }
}

/// The device's own frames to 48 kHz stereo: its first two channels (one
/// twice for mono), linear interpolation between frames.
struct Resample {
    format: poni_core::ielts::SampleFormat,
    stereo: bool,
    block: usize,
    /// Device frames per 48 kHz frame.
    step: f64,
    pos: f64,
    prev: [f32; 2],
}

impl Resample {
    fn new(d: &mic::Device) -> Resample {
        Resample::with(d.format, d.channels, d.block, d.rate)
    }

    fn with(format: poni_core::ielts::SampleFormat, channels: u16, block: usize, rate: u32) -> Resample {
        Resample { format, stereo: channels > 1, block, step: f64::from(rate.max(1)) / RATE as f64, pos: 1.0, prev: [0.0; 2] }
    }

    fn push(&mut self, bytes: &[u8], out: &mut VecDeque<[f32; 2]>) {
        let size = self.format.bytes();
        for f in bytes.chunks_exact(self.block) {
            let l = self.format.read(&f[..size]);
            let r = if self.stereo { self.format.read(&f[size..2 * size]) } else { l };
            self.feed([l, r], out);
        }
    }

    fn silence(&mut self, frames: usize, out: &mut VecDeque<[f32; 2]>) {
        for _ in 0..frames {
            self.feed([0.0; 2], out);
        }
    }

    fn feed(&mut self, x: [f32; 2], out: &mut VecDeque<[f32; 2]>) {
        while self.pos <= 1.0 {
            let t = self.pos as f32;
            out.push_back([self.prev[0] + (x[0] - self.prev[0]) * t, self.prev[1] + (x[1] - self.prev[1]) * t]);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = x;
    }
}

#[cfg(test)]
mod tests {
    use super::Resample;
    use poni_core::ielts::SampleFormat;
    use std::collections::VecDeque;

    #[test]
    fn resamples_to_48k_stereo() {
        // A second of 44.1 kHz mono 16-bit comes out as a second at 48 kHz, both sides alike.
        let mut r = Resample::with(SampleFormat::I16, 1, 2, 44_100);
        let mut out = VecDeque::new();
        r.push(&vec![0x40u8, 0x00].repeat(44_100), &mut out);
        assert!((out.len() as i64 - 48_000).abs() <= 1, "{}", out.len());
        assert!(out.iter().skip(2).all(|f| f[0] == f[1] && (f[0] - 64.0 / 32768.0).abs() < 1e-6));
    }

    /// Records the primary screen with sound for two seconds. Needs a
    /// desktop and a GPU: `cargo test -p poni screenrec -- --ignored`.
    #[test]
    #[ignore]
    fn records_an_mp4() {
        let path = std::env::temp_dir().join("poni-screenrec-test.mp4");
        let r = super::start(windows::Win32::Foundation::HWND::default(), path.clone(), true, true).expect("start");
        let sound = r.audio.is_some();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let path = r.stop().expect("stop");
        let file = std::fs::read(&path).unwrap();
        assert!(file.len() > 1000 && &file[4..8] == b"ftyp", "{} bytes", file.len());
        // An AAC track ("mp4a") whenever a sound device opened.
        assert_eq!(file.windows(4).any(|w| w == b"mp4a"), sound);
        println!("{} bytes, sound: {sound}", file.len());
        let _ = std::fs::remove_file(path);
    }
}
