//! The tray icon's usage badge: a percentage drawn into an RGBA bitmap.
//!
//! Windows tray icons cannot carry text, so the number has to be pixels. The
//! font is a hand-drawn bitmap rather than a TTF: at 16px a rasterised outline
//! font blurs into grey, while integer-scaled pixel glyphs stay sharp at every
//! icon size Windows asks for.

/// What the tray icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    /// The static app icon: the badge is off, or there is nothing to report.
    Logo,
    /// The active account's five-hour usage, already rounded and clamped.
    Percent { pct: u32, stale: bool },
    /// The active account has no five-hour number.
    Unknown,
    /// `cswap` reported a usage status other than `ok`.
    Alert,
}

/// Glyphs are 7 rows tall; widths vary so `!` does not sit in a 4px cell.
const GLYPH_HEIGHT: u32 = 7;
/// Blank columns between glyphs, before scaling.
const GLYPH_GAP: u32 = 1;
/// Blank pixels kept clear on every edge of the icon.
const MARGIN: u32 = 1;
/// The width the glyph scale is chosen against: three digit-wide glyphs, as in
/// `42%`. Scaling each label to fill the icon instead would draw `5%` twice the
/// size of `42%` and blow `!` up into a solid block.
const SCALE_WIDTH: u32 = 3 * 4 + 2 * GLYPH_GAP;

const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];
const GREEN: [u8; 3] = [0x2E, 0x7D, 0x32];
const AMBER: [u8; 3] = [0xB2, 0x6A, 0x00];
const RED: [u8; 3] = [0xC6, 0x28, 0x28];
const GREY: [u8; 3] = [0x61, 0x61, 0x61];

/// Percentages from here up are drawn amber, and from `RED_FROM` up red.
const AMBER_FROM: u32 = 70;
const RED_FROM: u32 = 90;

fn glyph(c: char) -> &'static [&'static str] {
    match c {
        '0' => &[".##.", "#..#", "#..#", "#..#", "#..#", "#..#", ".##."],
        '1' => &["..#.", ".##.", "..#.", "..#.", "..#.", "..#.", ".###"],
        '2' => &[".##.", "#..#", "...#", "..#.", ".#..", "#...", "####"],
        '3' => &[".##.", "#..#", "...#", ".##.", "...#", "#..#", ".##."],
        '4' => &["..#.", ".##.", "#.#.", "#.#.", "####", "..#.", "..#."],
        '5' => &["####", "#...", "###.", "...#", "...#", "#..#", ".##."],
        '6' => &[".##.", "#...", "#...", "###.", "#..#", "#..#", ".##."],
        '7' => &["####", "...#", "..#.", "..#.", ".#..", ".#..", ".#.."],
        '8' => &[".##.", "#..#", "#..#", ".##.", "#..#", "#..#", ".##."],
        '9' => &[".##.", "#..#", "#..#", ".###", "...#", "...#", ".##."],
        '%' => &["##.#", "##.#", "..#.", "..#.", ".#..", "#.##", "#.##"],
        '-' => &["...", "...", "...", "###", "...", "...", "..."],
        '!' => &["#", "#", "#", "#", "#", ".", "#"],
        _ => &[],
    }
}

fn glyph_width(c: char) -> u32 {
    glyph(c).first().map_or(0, |row| row.len() as u32)
}

/// Unscaled width of `text`, gaps included.
fn text_width(text: &str) -> u32 {
    let glyphs: u32 = text.chars().map(glyph_width).sum();
    let gaps = text.chars().count().saturating_sub(1) as u32 * GLYPH_GAP;
    glyphs + gaps
}

fn background(badge: &Badge) -> [u8; 3] {
    match *badge {
        Badge::Percent { stale: true, .. } | Badge::Unknown | Badge::Logo => GREY,
        Badge::Percent { pct, .. } if pct >= RED_FROM => RED,
        Badge::Percent { pct, .. } if pct >= AMBER_FROM => AMBER,
        Badge::Percent { .. } => GREEN,
        Badge::Alert => RED,
    }
}

/// The text to draw at `size`, or `None` for the logo.
///
/// The `%` sign is dropped only when the number would not otherwise fit: at
/// 16px `100%` needs 19 columns against 14 available, and a shrunken 100 is
/// worse than a bare one. Every other value keeps its sign at every size.
fn label(badge: &Badge, size: u32) -> Option<String> {
    match *badge {
        Badge::Logo => None,
        Badge::Unknown => Some("-".to_string()),
        Badge::Alert => Some("!".to_string()),
        Badge::Percent { pct, .. } => {
            let with_sign = format!("{pct}%");
            if text_width(&with_sign) <= size.saturating_sub(2 * MARGIN) {
                Some(with_sign)
            } else {
                Some(pct.to_string())
            }
        }
    }
}

/// Render `badge` as a `size`×`size` RGBA buffer, or `None` for the logo.
pub fn render(badge: &Badge, size: u32) -> Option<Vec<u8>> {
    let text = label(badge, size)?;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let bg = background(badge);
    let radius = (size / 8).max(1);
    for y in 0..size {
        for x in 0..size {
            if !outside_corner(x, y, size, radius) {
                put(&mut rgba, size, x, y, bg);
            }
        }
    }

    // Integer scales only, so every glyph pixel lands on whole icon pixels.
    // Labels wider than `SCALE_WIDTH` (`100%`) scale down to fit.
    // The vertical scale may run up to twice the horizontal one: three glyphs
    // side by side leave 16px of height for 7 rows, and condensed digits that
    // fill it read far better than square ones a third of the icon tall.
    let avail = size.saturating_sub(2 * MARGIN);
    let width = text_width(&text);
    let sx = (avail / width.max(SCALE_WIDTH)).max(1);
    let sy = (avail / GLYPH_HEIGHT).clamp(1, 2 * sx);

    let left = (size.saturating_sub(width * sx)) / 2;
    let top = (size.saturating_sub(GLYPH_HEIGHT * sy)) / 2;

    let mut pen = left;
    for c in text.chars() {
        for (row, line) in glyph(c).iter().enumerate() {
            for (col, cell) in line.chars().enumerate() {
                if cell != '#' {
                    continue;
                }
                let x0 = pen + col as u32 * sx;
                let y0 = top + row as u32 * sy;
                for y in y0..y0 + sy {
                    for x in x0..x0 + sx {
                        put(&mut rgba, size, x, y, WHITE);
                    }
                }
            }
        }
        pen += (glyph_width(c) + GLYPH_GAP) * sx;
    }

    Some(rgba)
}

/// True for pixels cut away to round the badge's corners.
fn outside_corner(x: u32, y: u32, size: u32, radius: u32) -> bool {
    let near = |v: u32| {
        if v < radius {
            Some(radius - v)
        } else if v >= size - radius {
            Some(v + radius + 1 - size)
        } else {
            None
        }
    };
    match (near(x), near(y)) {
        (Some(dx), Some(dy)) => {
            // Distance from the pixel's centre to the corner circle's centre.
            let dx = dx as f64 - 0.5;
            let dy = dy as f64 - 0.5;
            dx * dx + dy * dy > (radius as f64) * (radius as f64)
        }
        _ => false,
    }
}

fn put(rgba: &mut [u8], size: u32, x: u32, y: u32, rgb: [u8; 3]) {
    if x >= size || y >= size {
        return;
    }
    let i = ((y * size + x) * 4) as usize;
    rgba[i..i + 3].copy_from_slice(&rgb);
    rgba[i + 3] = 0xFF;
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [u32; 5] = [16, 20, 24, 32, 48];

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    fn white_pixels(rgba: &[u8]) -> Vec<usize> {
        rgba.chunks(4)
            .enumerate()
            .filter(|(_, p)| p[..3] == WHITE && p[3] == 0xFF)
            .map(|(i, _)| i)
            .collect()
    }

    fn pct(pct: u32) -> Badge {
        Badge::Percent { pct, stale: false }
    }

    #[test]
    fn the_logo_is_not_rendered() {
        assert_eq!(render(&Badge::Logo, 16), None);
    }

    #[test]
    fn every_glyph_has_seven_rows_of_equal_width() {
        for c in "0123456789%-!".chars() {
            let rows = glyph(c);
            assert_eq!(rows.len(), GLYPH_HEIGHT as usize, "glyph {c:?}");
            assert!(rows.iter().all(|r| r.len() == rows[0].len()), "glyph {c:?}");
            assert!(rows.iter().any(|r| r.contains('#')), "glyph {c:?} is blank");
        }
    }

    #[test]
    fn the_buffer_is_square_rgba() {
        for size in SIZES {
            let rgba = render(&pct(42), size).unwrap();
            assert_eq!(rgba.len(), (size * size * 4) as usize);
        }
    }

    #[test]
    fn every_value_fits_inside_the_margin_at_every_size() {
        for size in SIZES {
            for value in 0..=100 {
                let rgba = render(&pct(value), size).unwrap();
                let white = white_pixels(&rgba);
                assert!(!white.is_empty(), "{value}% at {size}px drew nothing");
                for i in white {
                    let (x, y) = (i as u32 % size, i as u32 / size);
                    assert!(
                        (MARGIN..size - MARGIN).contains(&x)
                            && (MARGIN..size - MARGIN).contains(&y),
                        "{value}% at {size}px touches the edge at ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_percent_sign_is_dropped_only_where_it_cannot_fit() {
        assert_eq!(label(&pct(5), 16).as_deref(), Some("5%"));
        assert_eq!(label(&pct(99), 16).as_deref(), Some("99%"));
        assert_eq!(label(&pct(100), 16).as_deref(), Some("100"));
        assert_eq!(label(&pct(100), 24).as_deref(), Some("100%"));
        assert_eq!(label(&pct(100), 32).as_deref(), Some("100%"));
    }

    #[test]
    fn the_background_follows_the_usage_level() {
        let bg = |badge: Badge| {
            let rgba = render(&badge, 16).unwrap();
            // Top edge, middle column: inside the badge and clear of any text.
            let p = pixel(&rgba, 16, 8, 0);
            [p[0], p[1], p[2]]
        };
        assert_eq!(bg(pct(0)), GREEN);
        assert_eq!(bg(pct(69)), GREEN);
        assert_eq!(bg(pct(70)), AMBER);
        assert_eq!(bg(pct(89)), AMBER);
        assert_eq!(bg(pct(90)), RED);
        assert_eq!(bg(pct(100)), RED);
        assert_eq!(
            bg(Badge::Percent {
                pct: 95,
                stale: true
            }),
            GREY
        );
        assert_eq!(bg(Badge::Unknown), GREY);
        assert_eq!(bg(Badge::Alert), RED);
    }

    #[test]
    fn the_corners_are_transparent() {
        for size in SIZES {
            let rgba = render(&pct(42), size).unwrap();
            for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
                assert_eq!(pixel(&rgba, size, x, y)[3], 0, "({x}, {y}) at {size}px");
            }
        }
    }

    #[test]
    fn different_values_draw_different_pixels() {
        assert_ne!(white_pixels(&render(&pct(12), 16).unwrap()), {
            white_pixels(&render(&pct(13), 16).unwrap())
        });
    }

    /// Height of the drawn text, in pixels.
    fn ink_height(badge: Badge, size: u32) -> u32 {
        let rows: Vec<u32> = white_pixels(&render(&badge, size).unwrap())
            .into_iter()
            .map(|i| i as u32 / size)
            .collect();
        rows.iter().max().unwrap() - rows.iter().min().unwrap() + 1
    }

    #[test]
    fn short_labels_are_not_blown_up() {
        for size in SIZES {
            let reference = ink_height(pct(42), size);
            assert_eq!(ink_height(pct(5), size), reference, "5% at {size}px");
            assert_eq!(ink_height(Badge::Alert, size), reference, "! at {size}px");
        }
    }
}
