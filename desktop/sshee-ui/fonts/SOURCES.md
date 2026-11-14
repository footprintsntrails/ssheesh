# Bundled fonts

TrueType files, because Rune reads raw OpenType and nothing in it decodes WOFF2 (0.10 ships WOFF2
copies of the same families). Fetched once on 2026-11-14 from the upstream releases below and
committed with their licence texts; nothing downloads them at build time. Every face covers Latin,
Cyrillic and Greek, so Russian text never takes a system fallback (Rune #916 on macOS).

| File | From | Version | Licence |
|---|---|---|---|
| JetBrainsMono-Regular.ttf, -Bold, -Italic, -BoldItalic | `fonts/ttf/` in https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip (SHA-256 `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`) | 2.304 | SIL OFL 1.1, `OFL-JetBrainsMono.txt` (the zip's `OFL.txt`) |
| SymbolsNerdFontMono-Regular.ttf | https://github.com/ryanoasis/nerd-fonts/releases/download/v3.5.1/NerdFontsSymbolsOnly.zip (SHA-256 `fdca3682534f6f65e1ccb2345b0362ccf67d9b8eca7c8025330946e93e2473bc`) | Nerd Fonts 3.5.1 | MIT, `LICENSE-NerdFonts.txt` (the zip's `LICENSE`, the same text as 0.10's `src/fonts/SymbolsNerdFontMono-LICENSE.txt`) |
| Inter-Regular.ttf, Inter-Medium.ttf, Inter-SemiBold.ttf | Rune's own copies, `crates/rune/assets/` at rev `c8d77a3f8559dcb52deb71c62f56cd92698b1095` (the rev `desktop/Cargo.toml` pins) | 4.1 ("Version 4.001") | SIL OFL 1.1, `OFL-Inter.txt` (Rune's `Inter-LICENSE.txt`) |

The OFL allows bundling the fonts with any software as long as the licence travels with them; the
fonts themselves may not be sold on their own. None of the files is modified here.

`scripts/check-desktop-workspace.sh` checks these hashes (`sha256sum -c` format):

```sha256
40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82  Inter-Regular.ttf
97ad806f526e41546d46365bb3a393145f75b7b1568913db74549ad8b8dba872  Inter-Medium.ttf
78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3  Inter-SemiBold.ttf
a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f  JetBrainsMono-Regular.ttf
5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb  JetBrainsMono-Bold.ttf
9d0a1f7a708e6af183f1193b7e81d40da294f5c67682c085d8401c60aac8ded4  JetBrainsMono-Italic.ttf
4039d5ce0ed225bf9c8b2c8c6436290ae2f356b7e90d70fa666227238324aa3b  JetBrainsMono-BoldItalic.ttf
fe471e538392f51910faab985fa8e192a39dd3426125edd15b71b3680df0e749  SymbolsNerdFontMono-Regular.ttf
30f0c136e3c88e422d0791acd97238870f9054a9729bc34cf2ff0d4ed8cac4ad  OFL-JetBrainsMono.txt
84a7a98c82140fb12c37fe42b93805baa16024cb3e5acc599b7ffe612c55d847  LICENSE-NerdFonts.txt
262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a  OFL-Inter.txt
```
