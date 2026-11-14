//! Which characters JetBrains Mono draws itself. Those stay in a row's shaped line; any other
//! is drawn on its own, pinned to its column, so a fallback font's other advance can't move
//! the columns after it.

/// Code points there are (U+0000 to U+10FFFF).
const CODE_POINTS: usize = 0x11_0000;

/// The Regular face's character map, asked once per character. What it answered is kept in a
/// fixed table of two bits per code point (asked, and the font has it): about 272 KiB once any
/// character beyond ASCII is met, however many different ones a server prints.
pub struct Coverage {
    font: Option<swash::FontRef<'static>>,
    /// Empty until the first character beyond ASCII.
    bits: Vec<u64>,
    /// Characters met so far that the font lacks (CJK, emoji, icons): each first meeting sends
    /// shaping to a fallback font, which is slow on macOS (Rune #916). For the meter.
    pub fallbacks: u64,
}

impl Coverage {
    pub fn new(font: &'static [u8]) -> Self {
        Coverage { font: swash::FontRef::from_index(font, 0), bits: Vec::new(), fallbacks: 0 }
    }

    pub fn has(&mut self, c: char) -> bool {
        if c.is_ascii() {
            return !c.is_ascii_control();
        }
        if self.bits.is_empty() {
            self.bits = vec![0; CODE_POINTS * 2 / 64];
        }
        let at = c as usize * 2;
        let (word, bit) = (at / 64, at % 64);
        let known = self.bits[word] >> bit;
        if known & 1 == 1 {
            return known & 2 == 2;
        }
        let has = self.font.is_some_and(|f| f.charmap().map(c) != 0);
        self.bits[word] |= (1 | u64::from(has) << 1) << bit;
        self.fallbacks += u64::from(!has);
        has
    }

    /// Bytes the table holds (for tests).
    pub fn table_bytes(&self) -> usize {
        self.bits.len() * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_cyrillic_and_greek_are_in_the_font_cjk_and_emoji_are_not() {
        let mut c = Coverage::new(crate::fonts::TERMINAL_REGULAR);
        for ch in ['a', 'Z', 'й', 'Ж', 'λ', 'ß', '•', '─'] {
            assert!(c.has(ch), "{ch}");
        }
        for ch in ['漢', '😀', '\u{f418}'] {
            assert!(!c.has(ch), "{ch}");
        }
        assert!(!c.has('漢'));
        assert!(c.has('й'));
        assert_eq!(c.fallbacks, 3, "each missing character counts once");
    }

    /// A server printing every code point there is (a fuzzer, random bytes read as UTF-8)
    /// leaves the table as big as one character did.
    #[test]
    fn every_code_point_fits_in_the_same_table() {
        let mut c = Coverage::new(crate::fonts::TERMINAL_REGULAR);
        c.has('й');
        let size = c.table_bytes();
        assert!(size <= 280 * 1024, "{size}");
        let mut missing = 0u64;
        for ch in (0x80..=0x10_FFFFu32).filter_map(char::from_u32) {
            missing += u64::from(!c.has(ch));
        }
        assert_eq!(c.table_bytes(), size);
        assert_eq!(c.fallbacks, missing);
        // Asked again, nothing is counted twice.
        assert!(!c.has('漢'));
        assert_eq!(c.fallbacks, missing);
    }
}
