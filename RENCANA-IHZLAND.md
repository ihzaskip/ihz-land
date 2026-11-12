# Rencana ihz.land

Fork pribadi Oyenza Studio (`ihzaskip/oyenza-studio`) yang diubah menjadi **ihz.land**.
Keputusan user (12 Nov 2026) ada di bawah. Centang tiap langkah setelah selesai.

## Prinsip

- **Nama internal tetap**: `poni.exe`, crate `poni`/`poni-core`, folder data `Poni`,
  variabel `PONI_*`. Yang diganti hanya yang **dilihat orang**. Dengan begitu sync dari
  upstream (`git fetch upstream && git merge upstream/main`) konfliknya sekecil mungkin.
- Merek, logo, dan karakter Oyen **bukan** bagian lisensi MIT. Semuanya harus hilang
  dari aplikasi, installer, situs, dan README.
- Build lokal tanpa admin: toolchain Rust GNU + WinLibs MinGW
  (`cargo +stable-x86_64-pc-windows-gnu wbuild`, MinGW di PATH).

## Langkah

1. [x] Update dari fork sendiri, plus kabar rilis upstream di island (commit 3d9a4ca).
2. [x] **Nama**: semua teks "Oyenza Studio" yang terlihat diganti "ihz.land". Termasuk
       menu, island, Settings, installer NSIS (nama, shortcut, uninstall display name),
       nama file rilis (`ihz-land-Setup-x.exe`), dan User-Agent.
3. [ ] **Ikon & art**: ikon aplikasi, art installer (`installer/art`), `docs/brand`, situs.
4. [ ] **Karakter orisinal** pengganti Oyen (tema dan mood sama: istirahat, musik,
       kursor, malam). Digambar dengan kode, bukan meniru kucing Oyen.
5. [ ] **Buang fitur akun Oyenza**: tab Oyenza (uang masuk/keluar, tagihan, catat
       pengeluaran), Health (langkah), Oyenza Inference di AI usage, Meeting notes
       (server Oyenza), Stocks (oyenza.trade). Pro dibuka lokal untuk semua add-on yang
       tersisa.
6. [ ] **Login ihz.app**: device code seperti TV (island menampilkan kode, disetujui
       di `ihz.app/device` saat sudah login Google). Rute baru di `ihz-web`:
       `POST /api/device/start`, `POST /api/device/token`, halaman `/device`.
       Kunci per perangkat disimpan di Upstash Redis dan bisa dicabut.
7. [ ] **IELTS tertanam** memakai backend temanielts: 5 rute `/island/ielts/*` di
       `temanielts`, diautentikasi dengan kunci perangkat dari langkah 6.
8. [x] **Quran**: tombol "Read Quran" membuka pemutar di `https://work.ihz.app`.
9. [ ] **Phone drop**: `server/drop-server.mjs` dijalankan di VPS Hermes
       (docker compose, seperti Duit) di `https://drop.ihz.app`, lalu default
       `PONI_DROP_SERVER` diganti. TURN Cloudflare opsional.
10. [ ] **Kota sholat**: tetap memakai `studio.oyenza.com/places` (keputusan user).
11. [ ] **Saluran rilis**: GitHub Pages fork (`ihzaskip.github.io/oyenza-studio`),
        environment `release` dengan secret `UPDATE_FEED_KEY`
        (`~/.oyenza-fork-keys/sign.pem`), CI membuat rilis dari tag `v*`.
        Situs `site/` diganti halaman ihz.land sederhana.
