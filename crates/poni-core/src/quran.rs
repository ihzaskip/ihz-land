//! The Quran add-on's data and the lookups the Quran page draws from: which
//! ayah and which word the recitation is at. The same rules as the ihz.app
//! web player (`lib/quran.ts` in the work project).
//!
//! `quran.bin` (tools/make_quran_pack.py) is b"QRN1", a u32 frame count, the
//! frames' offsets (count + 1 u32s, from the start of the file), then one
//! zstd frame each: frame 0 the surah list, frame n surah n. Opening a surah
//! reads and unpacks only its own frame.

use serde::Deserialize;
use std::io::{self, Read, Seek, SeekFrom};

/// One surah in the list (frame 0).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SurahInfo {
    pub id: u16,
    /// Its Latin name, as Kemenag writes it ("Al-Fatihah").
    pub nama: String,
    /// Its Arabic name.
    pub arab: String,
    /// Its meaning, in Indonesian ("Pembukaan").
    pub arti: String,
    /// How many ayat.
    pub jumlah: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ayah {
    /// The words, Uthmani script.
    pub words: Vec<String>,
    /// Kemenag's Indonesian translation.
    pub translation: String,
    /// Recited from..to, in ms from the start of the surah's recording.
    pub from: u32,
    pub to: u32,
    /// [word (1..), from, to] in ms, in time order: a word recited twice
    /// shows twice.
    pub segs: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Surah {
    pub n: u16,
    /// The recitation, one file for the whole surah.
    pub audio: String,
    pub ayat: Vec<Ayah>,
}

impl Ayah {
    /// Where its recitation starts: some reciters begin the first word a
    /// little before the ayah's `from`.
    pub fn start(&self) -> u32 {
        self.segs.first().map_or(self.from, |s| s[1].min(self.from))
    }

    /// The word (1-based) being recited at `ms`, or the last one said; 0
    /// before the first. Follows a word recited again; a position past the
    /// ayah's words (broken data) is passed over.
    pub fn word_at(&self, ms: u32) -> usize {
        let n = self.words.len();
        let mut k = 0;
        for s in &self.segs {
            if s[1] > ms {
                break;
            }
            if (1..=n).contains(&(s[0] as usize)) {
                k = s[0] as usize;
            }
        }
        k
    }

    /// The words joined by spaces, as the page draws them.
    pub fn text(&self) -> String {
        self.words.join(" ")
    }

    /// Word `k` (1-based) in [`Ayah::text`]: its start and length in UTF-16
    /// units, as DirectWrite counts. None for 0 or past the last word.
    pub fn word_span(&self, k: usize) -> Option<(u32, u32)> {
        let w = self.words.get(k.checked_sub(1)?)?;
        let start: usize = self.words[..k - 1].iter().map(|w| w.encode_utf16().count() + 1).sum();
        Some((start as u32, w.encode_utf16().count() as u32))
    }
}

impl Surah {
    /// The ayah (1-based) being recited at `ms`; between two, the one before.
    pub fn ayah_at(&self, ms: u32) -> usize {
        self.ayat.partition_point(|a| a.start() <= ms).max(1)
    }

    /// The next time after `ms` the ayah or the word on screen changes, so the
    /// page draws then and not every frame. None after the last word.
    pub fn next_change(&self, ms: u32) -> Option<u32> {
        let k = self.ayah_at(ms);
        self.ayat[k - 1..].iter().take(2).flat_map(|a| std::iter::once(a.start()).chain(a.segs.iter().map(|s| s[1]))).filter(|&t| t > ms).min()
    }

    /// Where the recitation of the last ayah ends.
    pub fn end_ms(&self) -> u32 {
        self.ayat.last().map_or(0, |a| a.to)
    }
}

/// What the pack holds for one surah: [[words, translation]…] and its timing.
#[derive(Deserialize)]
struct Waktu {
    audio: String,
    ayat: Vec<(u32, u32, Vec<[u32; 3]>)>,
}

/// `quran.bin`, opened: its offsets read, its frames read when asked.
pub struct Pack<R> {
    r: R,
    offsets: Vec<u32>,
}

fn bad(why: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

impl<R: Read + Seek> Pack<R> {
    pub fn open(mut r: R) -> io::Result<Self> {
        let mut head = [0u8; 8];
        r.read_exact(&mut head)?;
        if &head[..4] != b"QRN1" {
            return Err(bad("not a Quran pack"));
        }
        let count = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        if count != 115 {
            return Err(bad(format!("{count} frames")));
        }
        let mut raw = vec![0u8; 4 * (count + 1)];
        r.read_exact(&mut raw)?;
        let offsets: Vec<u32> = raw.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        if offsets.windows(2).any(|w| w[0] > w[1]) {
            return Err(bad("offsets out of order"));
        }
        Ok(Self { r, offsets })
    }

    fn frame(&mut self, i: usize) -> io::Result<Vec<u8>> {
        let (a, b) = (self.offsets[i], self.offsets[i + 1]);
        self.r.seek(SeekFrom::Start(a.into()))?;
        let mut packed = vec![0u8; (b - a) as usize];
        self.r.read_exact(&mut packed)?;
        let mut out = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(&packed[..]).map_err(bad)?.read_to_end(&mut out)?;
        Ok(out)
    }

    /// The 114 surahs' names and sizes.
    pub fn surahs(&mut self) -> io::Result<Vec<SurahInfo>> {
        serde_json::from_slice(&self.frame(0)?).map_err(bad)
    }

    /// Surah `n` (1..=114): its words, translation and timing.
    pub fn surah(&mut self, n: u16) -> io::Result<Surah> {
        if !(1..=114).contains(&n) {
            return Err(bad(format!("no surah {n}")));
        }
        let (teks, waktu): (Vec<(Vec<String>, String)>, Waktu) = serde_json::from_slice(&self.frame(n as usize)?).map_err(bad)?;
        if teks.len() != waktu.ayat.len() || teks.is_empty() {
            return Err(bad(format!("surah {n}: {} ayat, {} timed", teks.len(), waktu.ayat.len())));
        }
        let ayat = teks.into_iter().zip(waktu.ayat).map(|((words, translation), (from, to, segs))| Ayah { words, translation, from, to, segs }).collect();
        Ok(Surah { n, audio: waktu.audio, ayat })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ayah(words: usize, from: u32, to: u32, segs: &[[u32; 3]]) -> Ayah {
        Ayah { words: (0..words).map(|i| format!("w{i}")).collect(), translation: String::new(), from, to, segs: segs.to_vec() }
    }

    #[test]
    fn the_recitation_finds_its_ayah_and_word() {
        // Between ayat it stays on the one before; a reciter who starts the
        // first word before `from` moves on then.
        let s = Surah { n: 1, audio: String::new(), ayat: vec![ayah(1, 0, 5000, &[[1, 0, 2000]]), ayah(1, 5200, 9000, &[[1, 5100, 6000]]), ayah(0, 9000, 12000, &[])] };
        for (ms, k) in [(0, 1), (5050, 1), (5150, 2), (11000, 3), (99999, 3)] {
            assert_eq!(s.ayah_at(ms), k, "{ms} ms");
        }
        assert_eq!(s.next_change(5050), Some(5100));
        assert_eq!(s.next_change(6000), Some(9000));
        assert_eq!(s.next_change(11000), None);
        // 18:14, Alafasy: words 11 and 12 are recited twice; 99 is no word.
        let a = ayah(12, 0, 600, &[[10, 0, 100], [11, 100, 200], [12, 200, 300], [11, 300, 400], [12, 400, 500], [99, 500, 600]]);
        for (ms, k) in [(0, 10), (150, 11), (350, 11), (550, 12)] {
            assert_eq!(a.word_at(ms), k, "{ms} ms");
        }
        assert_eq!(ayah(2, 50, 90, &[[1, 50, 60]]).word_at(10), 0, "not started");
    }

    #[test]
    fn the_pack_opens_one_surah_at_a_time() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../addons/quran/quran.bin");
        let mut pack = Pack::open(std::fs::File::open(path).unwrap()).unwrap();
        let list = pack.surahs().unwrap();
        assert_eq!(list.len(), 114);
        assert_eq!((list[0].nama.as_str(), list[0].arab.as_str(), list[0].jumlah), ("Al-Fatihah", "الفاتحة", 7));
        assert_eq!(list[113].nama, "An-Nas");
        let f = pack.surah(1).unwrap();
        assert_eq!(f.ayat.len(), 7);
        assert!(f.audio.ends_with("/mishari_al_afasy/murattal/1.mp3"), "{}", f.audio);
        let a2 = &f.ayat[1];
        // Its marks in the data's order (shadda before the vowel), so spelled out.
        assert_eq!(a2.text(), "\u{671}\u{644}\u{652}\u{62d}\u{64e}\u{645}\u{652}\u{62f}\u{64f} \u{644}\u{650}\u{644}\u{651}\u{64e}\u{647}\u{650} \u{631}\u{64e}\u{628}\u{651}\u{650} \u{671}\u{644}\u{652}\u{639}\u{64e}\u{640}\u{670}\u{644}\u{64e}\u{645}\u{650}\u{64a}\u{646}\u{64e}");
        assert_eq!(a2.translation, "Segala puji bagi Allah, Tuhan seluruh alam,");
        // رَبِّ is word 3: after ٱلْحَمْدُ (9 units) and لِلَّهِ (7), each with its space.
        assert_eq!(a2.word_span(3), Some((18, 5)));
        assert_eq!((a2.word_span(0), a2.word_span(5)), (None, None));
        assert_eq!(f.ayah_at(a2.start() + 1), 2);
        assert_eq!(a2.word_at(a2.segs[2][1]), 3);
        assert_eq!(pack.surah(114).unwrap().ayat.len(), 6);
        assert_eq!(pack.surah(2).unwrap().ayat.len(), 286);
        assert!(pack.surah(0).is_err() && pack.surah(115).is_err());
        assert!(Pack::open(io::Cursor::new(b"QRN2".to_vec())).is_err());
    }
}
