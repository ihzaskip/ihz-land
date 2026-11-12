# Quran add-on

The Quran page (`crates/poni/src/pages/quran.rs`): one ayah at a time in
large Arabic with the Kemenag translation under it, Mishary Alafasy's
recitation streaming from quranicaudio.com, and the word being recited lit
up. The add-on switches on the first time Prayer's "Read Quran" is clicked
(or from the Add-ons tab) and downloads these files; switching it off
deletes them. Windows only.

- `quran.bin`: text, translation and word timing for all 114 surahs, one
  zstd frame each (`tools/make_quran_pack.py`, `poni_core::quran::Pack`).
- `AmiriQuran.ttf`: the font the Arabic is drawn in. Windows' own fonts
  lack some of the Uthmani script's small marks.
- `NOTICE.txt`: where they come from, and the font's license.

They are published, byte for byte, on the pre-release `addon-quran-1` of
this repository. Each one's size and SHA-256 are built into the app
(`poni_core::addons::QURAN`), and `cargo test -p poni-core addons` checks
that they match the files here. Changing a file means a new tag
(`addon-quran-2`), new numbers, and an app release.
