//! The fonts SSHeesh draws with, bundled, so every machine draws the same and Latin, Cyrillic
//! and Greek never wait on a system fallback (slow on macOS: Rune #916). Sources, versions,
//! licences and SHA-256s: `fonts/SOURCES.md`.

use rune::text::FaceWeights;

/// The terminal's Regular face: its character map says what stays in a row's line, its
/// metrics size the cells.
pub static TERMINAL_REGULAR: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");

/// Every face, for `AppSpec::fonts` (which replaces Rune's built-in Inter).
pub static FONTS: &[&[u8]] = &[
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
    TERMINAL_REGULAR,
    include_bytes!("../fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../fonts/JetBrainsMono-BoldItalic.ttf"),
    include_bytes!("../fonts/SymbolsNerdFontMono-Regular.ttf"),
];

/// The sans and mono slots (`AppSpec::families`).
pub const FAMILIES: (&str, &str) = ("Inter", "JetBrains Mono");

/// The terminal's family list: JetBrains Mono, then the Nerd Font's icons (a comma makes
/// `Family::Named` a CSS list).
pub const TERMINAL_FAMILY: &str = "JetBrains Mono, Symbols Nerd Font Mono";

/// JetBrains Mono comes in two weights, so its Regular answers 100-599 and its Bold 600-900, as
/// a stylesheet's `@font-face` rules would declare them.
static FACE_WEIGHTS: &[FaceWeights] = &[
    FaceWeights { family: "JetBrains Mono", weight: 400.0, range: (100.0, 599.0) },
    FaceWeights { family: "JetBrains Mono", weight: 700.0, range: (600.0, 900.0) },
];

/// Declares the faces' weights. Call before the window opens: Rune registers the faces when
/// it makes its first text system.
pub fn declare() {
    rune::text::set_face_weights(FACE_WEIGHTS);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each file is a TrueType font (Rune reads no WOFF2), not an HTML error page or a stub.
    #[test]
    fn every_bundled_face_is_a_truetype_file() {
        for (i, face) in FONTS.iter().enumerate() {
            assert!(face.len() > 100_000, "face {i}: {} bytes", face.len());
            assert_eq!(&face[..4], &[0, 1, 0, 0], "face {i}");
        }
    }
}
