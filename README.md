# Mr. Reader

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Built with GTK4](https://img.shields.io/badge/Built%20with-GTK4-4a86cf?logo=gnome)](https://www.gtk.org/)
[![libadwaita](https://img.shields.io/badge/libadwaita-0.9-7b5ea7)](https://gnome.pages.gitlab.gnome.org/libadwaita/)

**Mr. Reader** is a Rust + GTK4 desktop app that turns a folder of Markdown notes (or a
single file) into a splash/screensaver. It shows one line at a time, moving through your
files while the most frequent words grow in size and glow.

## Table of contents

- [Features](#features)
- [System dependencies](#system-dependencies)
- [Build and run](#build-and-run)
- [Configuration](#configuration)
- [Interface](#interface)
- [Performance](#performance)
- [Web preview](#web-preview)
- [Implementation notes](#implementation-notes)
- [License](#license)

## Features

- Reads a whole folder or one specific `.md`/`.markdown` file.
- Horizontal marquee animation, one line at a time.
- Word-frequency ranking with progressive sizing and glow levels.
- Live reindexing when the folder or file changes.
- SQLite cache for fast subsequent startups.
- Folder/file picker and full preferences dialog.
- Dark, light and automatic themes with a configurable accent color.
- Keyboard shortcuts for closing and fullscreen.

## System dependencies

### Debian/Ubuntu

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config
```

### Arch Linux

```bash
pacman -S gtk4 libadwaita
```

## Build and run

```bash
cargo build --release
cargo run --release -- ~/notes
```

If no folder is given, the current directory is used. You can also pass the path to a
specific `.md`/`.markdown` file.

## Configuration

The configuration lives at `~/.config/mr-reader/config.toml` (XDG).

| Key | Default | Description |
| --- | --- | --- |
| `folder` | — | Folder or Markdown file to read. |
| `top_n` | `40` | Number of words in the frequency ranking. |
| `min_word_len` | `3` | Minimum word length to consider. |
| `base_font_size` | `22` | Normal font size in points for all words. |
| `min_font_size` | `18` | Minimum font size. |
| `max_font_size` | `72` | Maximum size for highlighted words. |
| `font_family` | `FantasqueSansM Nerd Font` | Any font family installed on the system. |
| `accent_color` | `#e5304a` | Hex accent color for the background, glow and controls. |
| `speed` | `1.0` | Playback speed, from `0.05×` to `4×`. |
| `line_gap_ms` | `700` | Blank pause between lines, in milliseconds. |
| `order` | `alphabetical` | Line order: `alphabetical` or `random`. |
| `include_code_blocks` | `false` | Whether to include code blocks. |
| `max_file_size` | `5242880` | Maximum file size, in bytes (5 MB). |
| `worker_threads` | — | Number of indexer worker threads. |
| `stopwords` | — | Words excluded from the ranking. |
| `colors` | — | Highlight color palette. |
| `theme` | `dark` | `dark`, `light` or `auto`. |

> **Migration:** on the first run after the rename, if the new state does not exist yet,
> `config.toml` and the SQLite cache are migrated automatically from the directories used
> by the previous names (`marquesina-markdown` or `text-motion`). The new state is never
> overwritten.

### Choosing another folder or file

The folder button in the controls (or **Preferences → Folder or file**) lets you pick a
complete folder or a single Markdown file. When you confirm, the app clears its cache,
stops playback and reindexes without restarting the process.

The text field accepts `~` and `~/path`. When a single file is chosen, the file watcher
follows only that file.

## Interface

The main window keeps a minimal title bar with native minimize, maximize and close
buttons and can be resized freely. The app closes with `Escape` or `Ctrl+Q`, and `F11`
toggles fullscreen.

The bottom bar is a glass capsule with a *material aero* look: symbolic Adwaita icons,
circular buttons in the accent color, a draggable speed selector (`0.05×`–`4×`) and a
sun/moon button to switch between dark and light themes. **Preferences** lets you change
the folder or file, speed, line gap, order, theme, installed font family, normal font
size, maximum highlighted word size and accent color.

Marquee text has an outline (dark on a dark background, light on a light background) and
the red glow sits outside it, so letters stay readable even with colored words.

Anki-style metadata lines (`created-at:: …`, `card-s:: …`, `due-date:: …`) are discarded
while cleaning the Markdown: they are neither shown nor counted in the word ranking.

## Performance

Measured with **10,000 files** using:

```bash
python3 scripts/gen_notes.py /tmp/opencode/notes10k 10000
```

- Peak resident memory: ~189 MB (target < 200 MB).
- The UI stays responsive during indexing; CPU usage is limited by a pool of
  `cores - 1` threads running at low priority.
- The first startup shows content as soon as the first file is indexed. Later startups
  reuse the SQLite cache and do not reprocess unchanged files.

## Web preview

`preview/marquee.html` reproduces the same marquee in the browser (FantasqueSansM Nerd
Font, glow via `text-shadow`, `[gap][text][gap]` geometry, constant speed and pauses).
It does not use the Rust backend, so you can tune the style and test the animation
without launching GTK.

```bash
# Option 1: open the file directly
xdg-open preview/marquee.html

# Option 2: local server
python3 -m http.server 8765 --bind 127.0.0.1
# http://127.0.0.1:8765/preview/marquee.html
```

Preview controls: `Space` pauses, `←`/`→` change lines, `F11` toggles fullscreen, the
slider adjusts the speed and the sun/moon button switches themes.

## Implementation notes

- The text stays as a horizontal marquee (scrolling), one line at a time, rather than
  the original static centered layout. The animation uses a `GtkScrolledWindow`
  adjustment, without moving widgets or breaking the layout.
- The typeface is FantasqueSansM Nerd Font (with MesloLGS/monospace fallbacks), and the
  glow uses CSS `text-shadow` while keeping contrast on dark backgrounds.
- The marquee uses a `GtkScrolledWindow` with the horizontal policy set to `External`
  and `propagate-natural-width = false`, so content width does not propagate to the
  window.
- The UI event queue is capped at 4,096 entries to keep memory stable.
- The travel distance is `visible width + text width` (with padding on both sides), so
  text enters from the right and leaves completely on the left.
- Animation uses accumulated time rather than an absolute `Instant`, so pausing and
  resuming does not cause jumps.
- Word highlighting does not restart the marquee while it is moving; it is applied on
  the next line. Lines are trimmed to ~600 characters for safety.
- `notify` 9.0.0-rc.8 (the available stable release) is used for the watcher.
- Low thread priority is achieved by limiting the pool; OS priorities are not modified,
  keeping the app portable.
- The top words are kept in a `top_words` table updated incrementally.
- Lines are not stored in SQLite; they are read from the file when displayed.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
