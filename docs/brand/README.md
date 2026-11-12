# ihz.land brand files

ihz.land's character is **Pulo**, a small green island hill with a sprout on top. Pulo is an original drawing
made for this fork; the app draws it in code (`pulo_paths` in `crates/poni/src/oyen.rs`), with the same moods the
island's character has always had (resting, music, the cursor, the night).

| File | What |
|---|---|
| `pulo-logo.svg`, `pulo-logo-512.png` | The logo: Pulo on a near-black rounded tile, the same picture the app draws as its own icon |
| `social-preview.png` | The card GitHub shows when the repo link is shared (Settings → General → Social preview) |

`python tools/make_brand.py` redraws the logo, the share card, `crates/poni/res/poni.ico`, everything in
`installer/art` and the Mac DMG's background (`installer/mac/dmg`) from the same shapes the app uses.

Colours: hill `#1D9E75`, stem `#0F6E56`, leaves `#5DCAA5`, eyes `#04342C`, tile `#171717`.

ihz.land is a fork of [Oyenza Studio](https://github.com/lucacadalora/oyenza-studio) (MIT). The Oyenza name, its
logo and the Oyen character are not part of that license and are not used here.
