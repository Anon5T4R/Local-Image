<p align="center">
  <img src="assets/app-icon/hicolor/128x128/apps/local-image.png" width="96" height="96" alt="Local Image icon">
</p>

<h1 align="center">Local Image</h1>

<p align="center">
  An image viewer for Windows and Linux. Open a folder, move through the photos, zoom and rotate, read the EXIF,
  and save a copy without the metadata. No account, no photo library, written in Rust.<br>
  <a href="https://github.com/Anon5T4R/Local-Image/releases/latest">Download</a> ·
  <a href="ROADMAP.md">Roadmap and status</a> ·
  <a href="#where-it-comes-from">Where it comes from</a> ·
  <a href="#how-it-is-made">How it is made</a>
</p>

<!-- screenshot: a real folder open with one photo on screen and the EXIF panel; dark and light -->
<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/readme/main-dark.png">
    <img src="docs/images/readme/main-light.png" width="820" alt="Local Image showing a photo with the EXIF panel">
  </picture>
</p>

## What it does

- Open a folder with the native picker, or type or paste its path in the footer. Images and videos are listed in
  name order, and `←` / `→` move through them.
- Zoom with the mouse wheel, anchored at the pointer; drag to pan. `0` (or a double-click) fits the image to the
  window, `C` centers it keeping the zoom, `F` goes full screen (filling the screen when entering, back to fit when
  leaving), and `R` resizes the window to the image.
- Rotate the view in 90° steps with `[` and `]`; the export uses the same angle.
- `I` opens the EXIF panel: camera and lens, date taken, exposure, aperture, ISO and focal length, when the file has
  them. The app only reads metadata; it never writes any.
- Export clean: the image is decoded and written again, so EXIF and every other metadata block are gone. The copy
  is saved next to the original as `name.clean.png`, with the rotation you applied. The original is never
  modified.
- Videos in the folder are listed (they would otherwise vanish from the sequence) but not decoded: the app opens
  them in the system player and moves on to the next image, when there is one.
- `Ctrl+K` opens the command palette. Settings hold the language (English, Spanish, Portuguese (Brazil)), the theme
  (Automatic, Light, Dark) and the update check.
- Clicking the image hides or shows the interface. Opened from the file manager ("Open with") or the command line,
  the window starts with the interface hidden, at the image's size when it fits and never smaller than the minimum
  window; a click reveals the interface, and the status line then shows the image's dimensions and file size.
- The same commands are exposed to scripts and agents: `local-image mcp` (or `commands` / `run --script`) in a
  headless run, and a live channel with `--control` for the commands that need the window ([AGENTS.md](AGENTS.md)).

What works, what is missing and what is out of scope is in the [roadmap](ROADMAP.md).

## What it opens and writes

Images: PNG, JPEG (`.jpg`, `.jpeg`), WebP, GIF, BMP, TIFF (`.tif`, `.tiff`), ICO and JPEG XL (`.jxl`).

Videos (MP4, MOV, MKV, WebM, AVI, M4V, WMV, FLV, MPG, MPEG, M2TS, TS) are listed and handed to the system player;
the app does not decode them.

The destination decides the format: a destination ending in `.png` writes PNG; anything else is written as JPEG
under the name you gave (a destination with no extension gets `.jpg`). Either way the file is re-encoded, never a
copy of the original bytes.

AVIF is not read: the `image` crate decodes it only through dav1d, a C library, and the app does not build a C
image decoder. HEIC is not read either.

## Download

Windows 10/11 (x64): `.msi` per-machine installer or portable `.zip`. Linux (x86_64): `.AppImage`, `.deb`, `.rpm`,
`.tar.gz` or Arch `.pkg.tar.zst`. All in
[Releases](https://github.com/Anon5T4R/Local-Image/releases/latest). macOS is not published.

Releases ship a `SHA256SUMS.txt`. The release workflow also signs the files with
[minisign](https://jedisct1.github.io/minisign/) when CI has the signing key; it is the same key as every Local app.
To check a file:

```sh
minisign -Vm <file> -P RWTqN+hTTCFbDCGCA1cEJRLp2FSluxIUiJfqdoAYwkwNNVH5MTYH8qSN
```

The public key is also in [packaging/minisign.pub](packaging/minisign.pub).

## Network and privacy

- Your photos never leave the computer. There is no upload or telemetry.
- On start, and from Settings, the app asks GitHub for the newest release. It downloads and installs one only if you
  accept, and there is no setting to turn the check off yet. The updater verifies the downloaded file against its
  SHA-256 (GitHub's per-file digest or the release's `SHA256SUMS.txt`) and checks the suite's minisign signature
  when the asset has one; a missing or invalid signature is logged rather than blocking the update while older
  releases are still being signed.
- No AI: there is no assistant, model or key in this app.
- TLS is pure Rust (rustls with RustCrypto, through the vendored
  [`libs/rustls-rustcrypto`](libs/rustls-rustcrypto/README.md)); no `ring`, `aws-lc` or OpenSSL is in the program
  you install.
- Releases are built by GitHub Actions from a tagged commit with locked dependencies, checked by `cargo-deny`, and
  no dependency comes from a git URL.
- The app's own code (`src/`) has no `unsafe` blocks except the calls to the Windows API it needs: opening a link or
  a file with the system app, reading the work area, and attaching to the console for the command line.

## Where it comes from

Local Image is our own app, not a fork of someone else's viewer. It started as an internal pilot: the interface
lived in a `lab-ui` crate, and the image reading was ported from the `img.rs` of the legacy LocalImage app. This
repository is that viewer rebuilt on the shell every Local app shares; the pilot's UI crate was vendored here and
later removed, when languages, preferences and theme moved into the app.

Decoding comes from Rust crates: the `image` crate reads the formats, `jxl-rs` (through
`jxl-image-rs-integration`) adds JPEG XL, and `kamadak-exif` reads the metadata panel. None of them downloads
anything at runtime.

The updater is ours. It is also where the suite tries TLS without C: the update path uses a vendored fork of
[rustls-rustcrypto](https://github.com/RustCrypto/rustls-rustcrypto) (MIT or Apache-2.0) in
`libs/rustls-rustcrypto`, whose changes are limited to dropping the `rustls-webpki 0.102` dependency (the same
signature-algorithm constants come from `rustls-pki-types`) and the workspace and import adjustments that follow;
the provider stays the only one in the path, with no fallback to `ring` or `aws-lc`.

**What changed from the pilot:** the name, icon and identifiers; the shell shared by the suite (palette, themes,
settings, update card); app-local languages and preferences; and the updater with its Windows (MSI and portable)
and Linux (AppImage, deb, rpm and tarball) installs.

## How the app is built

| Part | What it holds |
|---|---|
| `src/` | the viewer: folder listing, decode, zoom/pan/rotation, EXIF panel, clean export, native picker, preferences, translations, theme, the suite shell (palette, settings, update card) and the automation (MCP server, `--control` channel, CLI) |
| `libs/egui`, `libs/winit` | the suite's vendored forks of egui and winit |
| `libs/craft-libs` | mirrors of the shared crates (i18n, update, icons, palette, desktop theme, MCP); the source is `local-libs` |
| `libs/rustls-rustcrypto` | the pure-Rust TLS provider fork used by the updater |

## How it is made

Local Image follows the same rules as every Local app: local files and no account, Rust only (including the UI and
the cryptography), one self-contained repository per app, governed dependencies, releases built from a tag, and
MCP so agents can use the app. It has no AI features: a viewer does not need them. The reasons for each decision:
[How we make the Local apps](https://github.com/Styx-Studios/.github/blob/main/profile/README.md#how-we-make-the-local-apps).

## Build from source

```sh
git clone https://github.com/Anon5T4R/Local-Image
cd Local-Image
cargo run --release    # the app
cargo xtask ci         # all local checks
```

Rust 1.95 or newer.

## Keyboard shortcuts

| Input | Action |
|---|---|
| `←` / `→` | previous / next file in the folder |
| Mouse wheel | zoom, anchored at the pointer |
| Drag | pan |
| `0`, double-click | fit the image to the window |
| `C` | center the image, keeping the zoom |
| `F` | full screen |
| `Esc` | close the EXIF panel (interface visible); otherwise leave full screen |
| `R` | resize the window to the image |
| `[` / `]` | rotate 90° counterclockwise / clockwise |
| `I` | EXIF panel |
| `Ctrl+K` | command palette |
| Click on the image | hide / show the interface |

## The Local apps

[Local Word](https://github.com/Anon5T4R/local-office) ·
[Local Sheets](https://github.com/Anon5T4R/local-sheets) ·
[Local Slides](https://github.com/Anon5T4R/local-slides) ·
[Local PDF](https://github.com/Anon5T4R/local-pdf) ·
[Local Keys](https://github.com/Anon5T4R/Local-Keys) ·
[Local Image](https://github.com/Anon5T4R/Local-Image) ·
[Local Photo](https://github.com/Anon5T4R/Local-Photo) ·
[Local Paint](https://github.com/Anon5T4R/Local-Paint) ·
[Local Vector](https://github.com/Anon5T4R/Local-Vector) ·
[Local Player](https://github.com/Anon5T4R/Local-Player) ·
[Local Notebook](https://github.com/Anon5T4R/Local-Obsidian) ·
[Local Mindmap](https://github.com/Anon5T4R/Local-Mind) ·
[Local Utils](https://github.com/Anon5T4R/Local-Utils)

## License and credits

MIT or Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)). The vendored
forks keep their own licenses in `libs/`, and the shared crates keep their notices in
[libs/craft-libs/NOTICE](libs/craft-libs/NOTICE).
