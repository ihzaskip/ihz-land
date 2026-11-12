"""Builds addons/quran/quran.bin, the Quran add-on's text, translation and
word timing (poni_core::quran::Pack reads it).

    python tools/make_quran_pack.py E:/Projects/work/public/quran

The source is the folder scripts/unduh-quran.mjs fills from api.quran.com:
surah.json, teks/<n>.json ([words, Kemenag translation] per ayah) and
waktu/7/<n>.json (Mishary Alafasy's audio file and per-word timing).

The file: b"QRN1", a u32 count (115), count + 1 u32 offsets from the start
of the file, then the frames, each one zstd frame (level 19, so the same
input gives the same bytes): frame 0 is surah.json, frame n is surah n as
[teks, waktu]. One surah is read without unpacking the rest.
"""

import json
import struct
import subprocess
import sys
from pathlib import Path

RECITER = "7"  # quran.com's id for Mishary Alafasy
OUT = Path(__file__).resolve().parent.parent / "addons" / "quran" / "quran.bin"


def zstd(data: bytes) -> bytes:
    return subprocess.run(["zstd", "-19", "-q", "-c", "--no-check"], input=data, capture_output=True, check=True).stdout


def compact(x) -> bytes:
    return json.dumps(x, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def main(src: Path) -> None:
    surahs = json.loads((src / "surah.json").read_text(encoding="utf-8"))
    assert len(surahs) == 114
    frames = [zstd(compact(surahs))]
    for s in surahs:
        n = s["id"]
        teks = json.loads((src / "teks" / f"{n}.json").read_text(encoding="utf-8"))
        waktu = json.loads((src / "waktu" / RECITER / f"{n}.json").read_text(encoding="utf-8"))
        assert len(teks) == len(waktu["ayat"]) == s["jumlah"], n
        frames.append(zstd(compact([teks, waktu])))
    head = 4 + 4 + 4 * (len(frames) + 1)
    offsets, at = [], head
    for f in frames:
        offsets.append(at)
        at += len(f)
    offsets.append(at)
    out = b"QRN1" + struct.pack("<I", len(frames)) + struct.pack(f"<{len(offsets)}I", *offsets) + b"".join(frames)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(out)
    print(f"{OUT}: {len(out):,} bytes")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
