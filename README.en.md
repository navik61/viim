# Viim

[Česky](README.md) · **English**

A minimalist transparent image viewer for **niri (Wayland)**, written in Rust + GTK4. The
borderless window is sized exactly to the image (no cropping), supports PNG/SVG transparency,
zooming at the cursor, browsing a directory with background loading, and a slideshow.

> Error messages and `--help` are in Czech. The behaviour is described below.

## Purpose
- Show PNG/SVG with transparency in a borderless window, **without cropping** (the window size is
  known in advance).
- Briefly show an image from a script, e.g. a reminder: `viim -t 3 image.png` (closes after 3 s).
- Everyday browsing of a directory with zoom at the cursor.
- Image previews for [fzfex](https://github.com/navik61/fzfex) (window size via `-W`/`-H`).

## Installation
Dependencies: `gtk4` (≥ 4.14) and Rust ≥ 1.85 (edition 2024). On Arch Linux:
```sh
sudo pacman -S --needed gtk4 rust
git clone https://github.com/navik61/viim.git
cd viim
cargo build --release
install -Dm755 target/release/viim ~/.local/bin/viim
install -Dm644 viim.desktop ~/.local/share/applications/viim.desktop   # optional: "Open with"
```
Default image viewer (optional):
```sh
for t in png jpeg webp gif bmp tiff svg+xml; do xdg-mime default viim.desktop image/$t; done
```
Rule for `~/.config/niri/config.kdl` (floating borderless window that doesn't take focus):
```kdl
window-rule {
    match app-id=r"^viim$"
    open-floating true
    open-focused false
    focus-ring { off; }
    shadow { off; }
    border { off; }
}
```
If you want the window to get focus when it opens, drop `open-focused false`.

## Quick start
```sh
viim                     # file chooser dialog
viim photo.png           # n/b browse the other images in the same directory
viim ~/Pictures          # all images in the directory
viim a.png b.svg         # only the given files
viim -t 3 reminder.png   # closes after 3 s
viim -s 5 ~/Pictures     # slideshow, one image every 5 s
```

## Options
| Option | Default | Meaning |
|---|---|---|
| `path …` | – (opens a dialog) | file(s) or a directory; a single file → its whole directory is browsable |
| `-t`, `--timeout SEC` | `0` (don't close) | close after N s; any action (key, scroll, drag) cancels it |
| `-s`, `--slideshow SEC` | off (interval 3 s) | start a slideshow with interval SEC; the `s` key then uses this interval |
| `-H`, `--max-height PX` | 90 % of the lowest monitor | max. window height (logical px) |
| `-W`, `--max-width PX` | 90 % of the narrowest monitor | max. window width (logical px) |
| `--app-id ID` | `viim` | app-id for niri `window-rule` |
| `-h`, `--help` | | help |

Supported formats: PNG, JPEG, WebP, GIF (first frame), BMP, TIFF, SVG.

Environment variables:
- `VIIM_AMD=0`: don't force rendering on an AMD GPU (see *How it works*).
- `VIIM_DEBUG=1`: print the cursor position on scroll and the size of every new texture.
- `VIIM_SKRIPT="n n b q"`: test hook, "presses" the given keys after display (every 700 ms, GDK
  key names such as `plus`, `Left`).

## Controls
| Key / mouse | Action |
|---|---|
| scroll up / down | zoom in / out at the cursor |
| `+` / `-` | zoom in / out at the center |
| `0` | back to the original size |
| arrows (bigger step with `Shift`) | pan the image |
| mouse drag | pan the image; when the whole image is visible, moves the window |
| `n` / `b` | next / previous image |
| `s` | slideshow on / off (`n`/`b` reset the countdown) |
| `o` / `O` (Shift+o) | open a file / directory (dialog; directory → starts with the first image) |
| `q` / `Esc` | close |

## Output
Only the window; errors go to stderr. Exit code: `0` OK, `1` nothing could be displayed, `2` invalid
arguments.

## How it works
- **Window size** is computed from the image dimensions (only shrunk to the limit, never enlarged)
  and the window is shown only after the first image has loaded. `resizable=false` → niri opens it
  as floating.
- **Zoom and pan** happen inside the window (`src/platno.rs`); the window itself doesn't change.
  Rendering on the GPU via `append_scaled_texture` with the `Trilinear` filter (mipmaps → smooth
  even at strong downscaling).
- **Loading** (`src/nacitani.rs`) runs in a fixed pool of 2 threads (`PRACANTU`) with a job queue.
  Jobs that are no longer needed (different list, image further than ± 1 from the current one) are
  skipped without decoding, so fast browsing doesn't start dozens of decodes at once. A decoder
  panic is caught and the file is treated as unreadable (the thread survives).
  - Raster: the `image` crate, downscaled by area averaging (`imageops::thumbnail`) to the window
    size × the largest monitor scale (physical px; smaller images and savings < ~20 % → full
    resolution). After zooming beyond that resolution, the full resolution is loaded ~150 ms after
    the last zoom step and swapped in without changing the view.
  - SVG: `resvg` (at least 2× resolution, re-rendered sharper after zooming, max. 8192 px).
  - The result is sent over a channel to the GTK main loop.
- **Cache** (768 MB cap, size tracked continuously) + preloading the images before and after the
  current one → `n`/`b` is instant. When full, the image **furthest** from the current one is
  evicted (least recently used on ties), so the preloaded neighbours stay.
- **Slideshow:** a single `glib` timer, scheduled only after the image is shown → slow decoding
  never skips anything, manual `n`/`b` resets the countdown, wraps around at the end. With a single
  image the timer doesn't run. Unreadable files are skipped like `n`.
- **Window center:** niri keeps the top-left corner when resizing, so the window is moved by half
  the difference with `niri msg action move-floating-window`.
- **Cursor position** for zoom: from the scroll event → pointer query → last `motion`/`enter`. It
  isn't cleared on `leave` (some tools briefly grab the pointer).
- **GPU:** on hybrid graphics with an AMD GPU (e.g. AMD iGPU + NVIDIA dGPU), rendering is forced
  onto the AMD GPU (`VK_DRIVER_FILES`, `__EGL_VENDOR_LIBRARY_FILENAMES`) so the dGPU can sleep. Only
  when an AMD GPU is present and the variables aren't set; `VIIM_AMD=0` disables it.

Constants (`src/okno.rs`): `OSTROST`, `KROK_ZOOM` (1.2), `KROK_POSUN` (10 %), `PODIL` (0.9),
`CACHE_BAJTY` (768 MB), `PRACANTU` (2); `src/nacitani.rs`: `RASTR_ZMENSIT_POD` (0.9),
`SVG_MAX_STRANA` (8192); `src/platno.rs`: `ZOOM_MIN`/`ZOOM_MAX`.

Release profile (`Cargo.toml`): `lto = "fat"`, `codegen-units = 1`, `strip = true` → ~4.5 MB binary
instead of ~6.8 MB, same or better speed. Deliberately **not** `panic = "abort"` (a decoder panic on
a broken file should only kill the thread, not the app) and **not** `opt-level = "s"` (slower
decoding). The build takes longer (~1.5 min).

## Dependencies
System: `gtk4` (≥ 4.14), Rust ≥ 1.85 (edition 2024). Optionally `niri` (window re-centering).
Crates: `gtk4`, `image`, `resvg`, `async-channel`, `serde_json`.

## Troubleshooting
- **Zoom happens at the center:** `VIIM_DEBUG=1 viim img.png` prints `událost=` / `dotaz=` /
  `motion=`; all `None` = the cursor position is unknown.
- **High memory usage:** the cache holds up to 768 MB of decoded images; photos are downscaled to
  the window size (~10 MB each), only zoomed-in ones are kept at full resolution – lower
  `CACHE_BAJTY`.
- **A photo is briefly blurry after zooming:** the full resolution loads ~150 ms after the last zoom
  step (`VIIM_DEBUG=1` shows `ostřejší`).
- **The window "jumps" when switching images:** `NIRI_SOCKET` or `niri` in PATH is missing → without
  re-centering the top-left corner stays in place.
- **The window is black or doesn't render:** try `VIIM_AMD=0 viim img.png`.
- **SVG with text looks different than in rsvg:** `resvg` uses system fonts via `fontdb`; missing
  fonts are substituted.

## Support
If viim is useful to you, you can buy me a coffee:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/Q7Z72B5QVS)

## License
MIT, see [`LICENSE`](LICENSE).
