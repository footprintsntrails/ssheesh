//! The terminal's cell size, from the font and the display's scale.
//!
//! 0.10.14 sized cells with xterm.js's WebGL formulas: the advance and the height rounded to
//! whole device pixels (`char.width = floor(advance × dpr)`, `char.height = ceil(height × dpr)`,
//! `cell.height = floor(char.height × lineHeight)`). The same here gives the same columns and
//! rows for the same window, and puts every cell, row and the grid's origin on whole pixels, so
//! box drawing meets its neighbours without seams. Shaped glyphs are pulled onto the cells with
//! `tracking = cell_w − advance` (under a pixel, either way).

/// How cells are snapped to the device's pixels (`--cell`, for the alignment experiment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CellMode {
    /// xterm.js's: the advance floored to whole pixels (0.10.14).
    #[default]
    Floor,
    /// The advance rounded to the nearest pixel.
    Round,
    /// No snapping: the font's own advance, no tracking.
    Exact,
}

impl CellMode {
    pub fn parse(s: &str) -> Option<CellMode> {
        match s {
            "floor" => Some(CellMode::Floor),
            "round" => Some(CellMode::Round),
            "exact" => Some(CellMode::Exact),
            _ => None,
        }
    }
}

/// One cell's size, in points and device pixels, at one scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    /// The text size in points.
    pub size: f32,
    pub scale: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    /// Device pixels (whole, except in `CellMode::Exact`).
    pub cell_w_px: f32,
    pub cell_h_px: f32,
    /// Letter spacing that puts each shaped glyph on its cell: `cell_w − advance`.
    pub tracking: f32,
}

impl CellMetrics {
    /// `advance`: the font's advance at `size` in points; `natural`: its natural line height in
    /// points (ascent + descent + leading); `line_height`: the setting (1.22 in 0.10).
    pub fn new(size: f32, advance: f32, natural: f32, line_height: f32, scale: f32, mode: CellMode) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let (cw_px, ch_px) = match mode {
            CellMode::Floor => ((advance * scale).floor().max(1.0), ((natural * scale).ceil() * line_height).floor().max(1.0)),
            CellMode::Round => ((advance * scale).round().max(1.0), ((natural * scale).ceil() * line_height).floor().max(1.0)),
            CellMode::Exact => (advance * scale, natural * line_height * scale),
        };
        let cell_w = cw_px / scale;
        let tracking = if mode == CellMode::Exact { 0.0 } else { cell_w - advance };
        CellMetrics { size, scale, cell_w, cell_h: ch_px / scale, cell_w_px: cw_px, cell_h_px: ch_px, tracking }
    }

    /// The columns and rows that fit `w` × `h` points (the terminal's area without padding).
    pub fn fit(&self, w: f32, h: f32) -> (u16, u16) {
        // A hair of slack so a size that is exactly n cells (in floating point) gives n.
        let cols = ((w + 1e-3) / self.cell_w).floor().clamp(0.0, u16::MAX as f32) as u16;
        let rows = ((h + 1e-3) / self.cell_h).floor().clamp(0.0, u16::MAX as f32) as u16;
        (cols, rows)
    }
}

/// JetBrains Mono's vertical metrics, read from the font: `(ascent + descent + leading) / em`,
/// the browser's "normal" line height for it.
pub fn natural_height_em(font: &[u8]) -> f32 {
    let Some(f) = swash::FontRef::from_index(font, 0) else { return 1.32 };
    let m = f.metrics(&[]);
    if m.units_per_em == 0 {
        return 1.32;
    }
    (m.ascent + m.descent + m.leading) / f32::from(m.units_per_em)
}

/// The advance of `'0'` in em, read from the font (Rune's own measure is preferred, this is
/// for when no text system is at hand).
pub fn advance_em(font: &[u8]) -> f32 {
    let Some(f) = swash::FontRef::from_index(font, 0) else { return 0.6 };
    let m = f.metrics(&[]);
    let glyph = f.charmap().map('0');
    let adv = f.glyph_metrics(&[]).advance_width(glyph);
    if m.units_per_em == 0 || adv <= 0.0 {
        return 0.6;
    }
    adv / f32::from(m.units_per_em)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGULAR: &[u8] = include_bytes!("../../fonts/JetBrainsMono-Regular.ttf");

    #[test]
    fn jetbrains_mono_is_600_units_wide_and_132_tall() {
        assert!((advance_em(REGULAR) - 0.6).abs() < 1e-6);
        assert!((natural_height_em(REGULAR) - 1.32).abs() < 1e-3, "{}", natural_height_em(REGULAR));
    }

    #[test]
    fn cells_are_whole_pixels_at_every_scale() {
        // 13 pt JetBrains Mono, line height 1.22 (0.10's defaults).
        let (adv, nat) = (7.8, 13.0 * 1.32);
        for (scale, w, h) in [(1.0, 7.0, 21.0), (1.25, 9.0, 26.0), (1.5, 11.0, 31.0), (1.75, 13.0, 37.0), (2.0, 15.0, 42.0)] {
            let m = CellMetrics::new(13.0, adv, nat, 1.22, scale, CellMode::Floor);
            assert_eq!((m.cell_w_px, m.cell_h_px), (w, h), "at {scale}x");
            assert!((m.cell_w * scale - w).abs() < 1e-4);
            assert!(m.tracking < 0.0 && m.tracking > -1.0, "{}", m.tracking);
        }
        let round = CellMetrics::new(13.0, adv, nat, 1.22, 1.0, CellMode::Round);
        assert_eq!(round.cell_w_px, 8.0);
        let exact = CellMetrics::new(13.0, adv, nat, 1.22, 2.0, CellMode::Exact);
        assert_eq!((exact.cell_w, exact.tracking), (7.8, 0.0));
    }

    #[test]
    fn what_fits_in_an_area() {
        let m = CellMetrics::new(13.0, 7.8, 17.16, 1.22, 1.0, CellMode::Floor);
        assert_eq!(m.fit(936.0, 648.0), (133, 30));
        assert_eq!(m.fit(7.0 * 80.0, 21.0 * 24.0), (80, 24));
        assert_eq!(m.fit(0.0, -5.0), (0, 0));
    }
}
