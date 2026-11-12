# Notes for coding agents

Codex, Claude Code, Kimi or any other agent working in this repo: read this before you change the app, cut a release or change the website.

## Keep it light

Oyenza Studio sits on screen all day, so how little it costs is part of the product. The maintainer's rule (24 Oct 2026): keep it very light on CPU, memory and download size, and fast, however many features it gets. Every change follows these:

- **Nothing runs at rest unless the person switched it on or its page is open.** No new thread, timer, poll or network call that keeps going while the island is idle. React to Windows' own notifications where there are some. When something has to repeat, give the interval a reason in a comment and stop it as soon as nothing needs it.
- **Anything that repeats rests when nobody is there:** while the session is locked, the display is off or battery saver is on. The app already knows all three (`App::locked`, `App::display_off`, `st.power.saver`), and ambient animation already pauses for the first two.
- **Motion costs a frame every refresh.** Something that moves by itself on the closed island (the equalizer, Oyen's notes) redraws for as long as it shows. Add one only where people watch it, and let it stop.
- **Load heavy Windows parts when the feature first needs them, not at start.** Performance counters, graphics devices and WinRT classes bring their own libraries: the System page's counters alone add about 6 MB once loaded.
- **Big data is an add-on, never part of poni.exe.** Dictionaries, sounds, place lists and models go on a GitHub pre-release and download when first needed, checked against the size and SHA-256 in `poni_core::addons` (`addons/`, `services/addons.rs`). `include_bytes!` is for small things the app can't start without, such as the icon and the Outfit font.
- **A new crate needs a reason and its size.** Build with `CARGO_PROFILE_RELEASE_STRIP=false cargo wbuild` before and after, and compare `x86_64-w64-mingw32-nm --size-sort -S -C` totals per crate. Code that runs for moments (a handshake, a parser, a file format) can be built for size in the workspace `Cargo.toml` (`[profile.release.package.<name>] opt-level = "s"`). Code that runs every frame stays at full speed.
- **Measure CPU in cycles, not milliseconds.** `TotalProcessorTime`, `GetProcessTimes` and Task Manager count in 15.6 ms clock ticks, so a reading that takes a fraction of a millisecond shows as zero. Use `QueryProcessCycleTime` or `QueryThreadCycleTime`.
- **Every PR says what it costs:** download size (the installer in the `oyenza-studio-setup` artifact), memory at rest (`smoke-out/metrics.txt` in the `poni-screens` artifact), and anything new that runs at rest or on every frame. "Nothing new" is a fine answer.

## This is a fork

This repository is **ihz.land**, a personal fork of [lucacadalora/oyenza-studio](https://github.com/lucacadalora/oyenza-studio) (MIT). The Oyenza name, its logo and the Oyen character are not part of that license: none of them may appear in the app, the installer, the website or the README. The character here is **Pulo** (`crates/poni/src/oyen.rs`; `tools/make_brand.py` draws the icon and installer art from the same shapes). [RENCANA-IHZLAND.md](RENCANA-IHZLAND.md) records what the fork changed and why.

The original project's account features are gone (the Oyenza tab, Health, Stocks, Meeting notes, Today's agenda, Oyenza Inference, Studio Pro). What stays of an account is the optional sign-in to **ihz.app** (`poni_core::oyenza`, the device code in `ihz-web/docs/device-login.md`), whose key the IELTS coach sends to **temanielts.com**. Paired phone drop meets at `drop.ihz.app`. Prayer times still reads city lists from `studio.oyenza.com/places` (the owner's decision).

## Names that stay

Only the names people see changed. `poni.exe`, its folders (`Poni`), the uninstall key, the Run value, `Local\Poni.Island`, the Mac bundle id `com.oyenza.poni`, the crates, the module names (`oyenza.rs`, `Theme::Oyen`), the `PONI_*` variables and the sign-in app id `poni` stay as they are, so `git fetch upstream && git merge upstream/main` keeps its conflicts small.

## Website and releases

The website is `site/index.html`, published at https://ihzaskip.github.io/ihz-land/ by the Website workflow (`.github/workflows/pages.yml`) whenever `site/` or `docs/brand/` changes on main, and after every release. Its `<span data-release="version">` names the version it describes: a page naming an unreleased version waits for that release (`tools/ci-site-check.sh`). Every deploy carries `latest.json`, the signed update file every copy of the app reads (`poni_core::update::FEED_URL`, [docs/updates.md](docs/updates.md)); before the first release there is none, and the app reads the 404 as "nothing new". The `UPDATE_FEED_KEY` secret that signs it lives in the `release` environment: never print it or move it. Its public half is in `crates/poni-core/data/update-keys.txt`.

## Cutting a release

Tags and releases happen only when the owner says so. Copy the last "Release x.y.z" commit for where the version lives (`Cargo.toml`, `Cargo.lock` for each workspace crate, `installer/poni.nsi`, `installer/poni.iss`, `crates/poni/res/poni.rc` and `PONI_VERSION` in `ci.yml`), add `docs/release-notes/vX.md` (the release text) and set the website's version. Then push an annotated tag `vX`: the `ci` workflow builds, tests and publishes `ihz-land-Setup-X.exe`, `ihz-land-Setup.exe` (the Download button's name) and `ihz-land-X-portable.zip`, then starts the Website workflow. To try that path without releasing, run `ci` by hand with `release_dry_run` (`gh workflow run ci.yml --ref main -f release_dry_run=true`). The Mac build, the Microsoft Store package, the download stats and the yt-dlp pin are the original project's and run only by hand here.
