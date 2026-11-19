# Third-party material in `desktop/`

Besides the crates Cargo builds (each under its own licence, listed in `Cargo.lock`), this folder
carries the following. Code ported from other projects keeps its notice at the top of the file it
went into.

## Fonts (`sshee-ui/fonts/`)

Sources, versions and SHA-256s: `sshee-ui/fonts/SOURCES.md`.

| Font | Licence | Licence text |
|---|---|---|
| JetBrains Mono 2.304 (Regular, Bold, Italic, Bold Italic) | SIL Open Font License 1.1 | `sshee-ui/fonts/OFL-JetBrainsMono.txt` |
| Symbols Nerd Font Mono (Nerd Fonts 3.5.1) | MIT | `sshee-ui/fonts/LICENSE-NerdFonts.txt` |
| Inter 4.1 (Regular, Medium, SemiBold), as Rune bundles it | SIL Open Font License 1.1 | `sshee-ui/fonts/OFL-Inter.txt` |

## Rune

The window toolkit, https://github.com/anomalyco/rune (MIT), at the rev `Cargo.toml` pins, with
the four crates Rune patches (`winit`, `parley_engine`, `wgpu-hal`, `accesskit_windows`) taken
from the same rev; each patch is described in Rune's root `Cargo.toml` and keeps its upstream
licence. Rune's own crate is built from `vendor/rune`: a copy of `crates/rune` at that rev
(`scripts/vendor-rune.sh`) with our fixes in `vendor/rune.patch`, under Rune's MIT licence
(`vendor/rune/LICENSE`).

## Ported code

| File | From | Licence |
|---|---|---|
| `sshee-ui/src/terminal/glyphs.rs` | xterm.js, `addon-webgl/src/CustomGlyphs.ts` (box drawing, block elements, Powerline separators), reworked onto whole device pixels | MIT, notice at the top of the file |
| `sshee-ui/src/kit/scroll_bar.rs` and `scroll_bar/visibility.rs` | Rune's `rune-kit` crate, `src/scroll_bar.rs`, `scroll_bar/press.rs`, `scroll_bar/visibility.rs` and `grip.rs` at the rev `Cargo.toml` pins, cut down to one look with a horizontal bar added | MIT (Rune's), said at the top of the file |

`crates/sshee-term` (outside this folder) ports xterm.js's key encoders the same way.

## Agents' marks (`sshee-ui/src/screens/prompts.rs`)

The buttons that start an agent carry its mark, as 0.10's `AgentIcons.tsx` drew them:
Claude's from Simple Icons (CC0 1.0, no notice required); OpenCode's and oh-my-pi's from
their own repositories (MIT); Codex's after LobeHub Icons (MIT), redrawn in one colour. The
marks belong to their owners and show only which program a button starts.

## Icons (`sshee-ui/src/icons.rs`)

The icons are line drawings written for SSHeesh (the 1.0 design's symbols, 0.10's onion and
logo), with one exception:

| Icon | From | Licence |
|---|---|---|
| The gear (Settings) | Lucide, "settings" (https://lucide.dev/icons/settings), path data unchanged | ISC (with Feather's MIT for the parts that come from it), below |

The licence as lucide-react 1.64.0 ships it (`node_modules/lucide-react/LICENSE`, the copy 0.10 is
built with):

```
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The following Lucide icons are derived from the Feather project:

airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out

The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Pixel server icons (`sshee-ui/src/pixel_icons.rs`)

The server icons of Settings → Layout → Server icons → Pixels are pixelarticons by Gerrit
Halfmann (https://github.com/halfmage/pixelarticons), path data unchanged, as 0.10 shipped them
in `src/lib/pixelIcons.ts`. `local` (this computer) was drawn for SSHeesh in the same manner.

```
MIT License

Copyright (c) 2019 Gerrit Halfmann

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Video in Files (`media/`)

The preview's player decodes with crates built into the program, so nothing has to be
installed to watch a video. Their licences let them be combined with ours: the MPL-2.0 ones are
used unchanged, as crates.io publishes them (their source is there).

| Crate | What for | Licence |
|---|---|---|
| `openh264` 0.9, `openh264-sys2` 0.9 | H.264. The crate compiles Cisco's OpenH264 (C++, the upstream rev in its `upstream/VERSION`) into the program; its notice is below | BSD-2-Clause |
| `rusty_vp9` 0.3 | VP9 | Apache-2.0 |
| `mp4` 0.14 | MP4 and MOV indexes | MIT |
| `matroska-demuxer` 0.8 | Matroska and WebM | Zlib OR MIT OR Apache-2.0 |
| `symphonia-core`, `symphonia-codec-aac`, `symphonia-codec-vorbis` 0.5.5 | AAC and Vorbis sound | MPL-2.0 |
| `opuscule` 0.2 | Opus sound | MPL-2.0 |
| `cpal` 0.15 (macOS and Windows) | The sound: Core Audio and WASAPI | Apache-2.0 |
| `libloading` 0.8 (Linux) | The sound: ALSA's `libasound.so.2`, opened when a video plays | ISC |

H.264 is covered by patents in some countries. Cisco pays the licence for the OpenH264 binaries it
builds and hands out itself; a copy compiled from its source, as here, isn't covered by that.

The demo's clip (`app/src/demo/media/promo.mp4`) and the tests' clips (`media/fixtures/`) were
made for SSHeesh with ffmpeg from its test sources (moving colour, sine tones); the commands are
beside them in the code.

OpenH264's notice, as `openh264-sys2` ships it (`upstream/LICENSE`):

```
Copyright (c) 2013, Cisco Systems
All rights reserved.

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice, this
  list of conditions and the following disclaimer in the documentation and/or
  other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```