//! What a Files pane shows at its width: which toolbar buttons have room (the ⋯ menu always
//! has them all), which columns the list keeps, and where the open file goes. 0.10 decided
//! this with container queries on the pane (`@xs` 320 px up to `@3xl` 768 px); these are the
//! same steps, in points.

/// The toolbar at a pane's width. The bookmarks button, the path, "+" and "⋯" always show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Toolbar {
    /// Back, Forward (once there's somewhere to go) and Up one folder.
    pub moves: bool,
    /// The filter field, and how wide it is.
    pub filter: Option<f32>,
    pub upload: bool,
    pub refresh: bool,
}

impl Toolbar {
    pub fn at(width: f32) -> Toolbar {
        Toolbar {
            moves: width >= 320.0,
            filter: (width >= 512.0).then_some(if width >= 768.0 { 160.0 } else { 120.0 }),
            upload: width >= 672.0,
            refresh: width >= 768.0,
        }
    }
}

/// The list's columns after the name and size, at the list's own width: the date steps out
/// first, then the permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub modified: bool,
    /// `drwxr-xr-x`.
    pub mode: bool,
}

impl Columns {
    pub fn at(width: f32) -> Columns {
        Columns { modified: width >= 448.0, mode: width >= 672.0 }
    }
}

/// The open file's pane: 420 points unless dragged, between these.
pub const PREVIEW_DEFAULT: f32 = 420.0;
pub const PREVIEW_MIN: f32 = 220.0;
pub const PREVIEW_MAX: f32 = 900.0;
/// Below this a pane has no room for the list beside an open file: the file takes it all.
pub const BESIDE_FROM: f32 = 672.0;

/// Where the open file goes in a pane `width` wide, with `wanted` the width it was dragged to
/// (Settings' `previewWidth`): `None` when it takes the whole pane, else its width, which
/// leaves the list at least 30% and never less than its own least.
pub fn preview_width(width: f32, wanted: f64) -> Option<f32> {
    if width < BESIDE_FROM {
        return None;
    }
    let wanted = if wanted.is_finite() && wanted > 0.0 { wanted as f32 } else { PREVIEW_DEFAULT };
    Some(wanted.clamp(PREVIEW_MIN, PREVIEW_MAX).min(width * 0.7))
}

/// The width the preview is dragged to: its edge moved `dx` points to the right from where it
/// was `from` wide. Whole points, between the least and the most.
pub fn dragged_width(from: f64, dx: f32) -> f64 {
    (from - f64::from(dx)).clamp(f64::from(PREVIEW_MIN), f64::from(PREVIEW_MAX)).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_step_out_as_the_pane_narrows() {
        assert_eq!(Toolbar::at(1000.0), Toolbar { moves: true, filter: Some(160.0), upload: true, refresh: true });
        assert_eq!(Toolbar::at(700.0), Toolbar { moves: true, filter: Some(120.0), upload: true, refresh: false });
        assert_eq!(Toolbar::at(600.0), Toolbar { moves: true, filter: Some(120.0), upload: false, refresh: false });
        assert_eq!(Toolbar::at(400.0), Toolbar { moves: true, filter: None, upload: false, refresh: false });
        assert_eq!(Toolbar::at(300.0), Toolbar { moves: false, filter: None, upload: false, refresh: false });
    }

    #[test]
    fn the_date_goes_before_the_permissions() {
        assert_eq!(Columns::at(900.0), Columns { modified: true, mode: true });
        assert_eq!(Columns::at(500.0), Columns { modified: true, mode: false });
        assert_eq!(Columns::at(400.0), Columns { modified: false, mode: false });
    }

    #[test]
    fn the_open_file_takes_a_narrow_pane_and_keeps_to_its_width_in_a_wide_one() {
        assert_eq!(preview_width(600.0, 420.0), None);
        assert_eq!(preview_width(1200.0, 420.0), Some(420.0));
        assert_eq!(preview_width(1200.0, 2000.0), Some(840.0f32.min(900.0)));
        assert_eq!(preview_width(1600.0, 2000.0), Some(900.0));
        assert_eq!(preview_width(1200.0, 100.0), Some(220.0));
        assert_eq!(preview_width(700.0, 600.0), Some(490.0));
        assert_eq!(preview_width(1200.0, f64::NAN), Some(420.0));
        // Dragged left, it widens.
        assert_eq!(dragged_width(420.0, -30.4), 450.0);
        assert_eq!(dragged_width(420.0, 400.0), 220.0);
    }
}
