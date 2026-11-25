# Local Image roadmap

The honest state of the app: what it does, what it deliberately does not do, and the limits we know. Release notes
list what changed in each version; this file carries no dates and no promises.

## What it does

- Opens a folder and shows one file at a time, walking the list with the arrow keys. Images and videos share the
  list, in name order.
- Zoom anchored at the pointer, drag to pan, fit to the window, center without changing the zoom, a full screen the
  image fills, a window sized to the image, and 90° rotation.
- Reads a fixed set of EXIF fields for the panel (camera, lens, date taken, exposure, aperture, ISO, focal length).
- Exports a re-encoded copy without any metadata, next to the original, with the rotation applied.
- Hands videos to the system player instead of decoding them.
- Command palette, language and theme settings, and an update check.
- The same commands over MCP, a live control channel and the command line.

## Deliberately out of scope

- **Editing and annotation.** The app is a viewer, and nothing in it changes the pixels of your files.
- **A photo library or database.** It reads the folder you point it at; inside that folder it writes only the clean
  export.
- **File management** (rename, move, delete, new folders). The system file manager does that better and the viewer
  stays small.
- **Video playback.** Videos are listed so they do not disappear from the sequence; playback belongs to a player.
- **AVIF and HEIC.** AVIF would need a C decoder (dav1d), and the app keeps its image path in Rust.
- **JPEG XL encoding.** The pure-Rust encoder is AGPL, and the dependency policy allows only permissive licenses;
  JPEG XL is read-only here.
- **macOS.** Windows and Linux are what we build and test.

## Known limits

- Animated GIF and WebP files show their first frame only.
- The EXIF orientation tag is not applied on open; a photo from a rotated camera may appear sideways until you use
  `[` / `]`. The export follows the rotation you chose in the viewer.
- Images are decoded whole into memory and loaded as a single texture. Very large photos use a lot of RAM, and going
  back to a photo decodes it again; there is no thumbnail grid or cache.
- The EXIF panel shows a fixed set of camera fields and does not show GPS or arbitrary tags.
- A JPEG export has no alpha channel; PNG keeps it.
- The app remembers the language and the theme, not the last folder, the window size or its position.
