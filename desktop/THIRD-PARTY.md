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
| Inter 4.1 (Regular, SemiBold), as Rune bundles it | SIL Open Font License 1.1 | `sshee-ui/fonts/OFL-Inter.txt` |

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

`crates/sshee-term` (outside this folder) ports xterm.js's key encoders the same way.
