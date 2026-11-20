//! A canvas as text: every character cell shows a 2 by 4 block of the canvas's dots in
//! one colour, taken from its brightest dot. Drawn with octants (Unicode 16, solid) or
//! Braille patterns (separated dots, in nearly every font).

use std::io::Write as _;

use super::{
    octants::OCTANTS,
    raster::Canvas,
    theme::{Gradient, Rgb, mix},
};

/// Dots across and down one character cell.
pub const CELL_DOTS: (usize, usize) = (2, 4);

/// A dot brighter than this is lit.
const LIT: f32 = 0.04;
/// A dot dimmer than this share of its cell's brightest is left out: a cell has one
/// colour, so a dim line sharing it would show as a bright smudge.
const KEEP: f32 = 0.7;

/// The character for each pattern of lit dots: bit `2 * row + column` (row 0 at the top).
pub type Glyphs = [u32; 256];

/// A set of characters and how strongly it is drawn.
pub struct Style {
    pub glyphs: Glyphs,
    /// Exponent from a cell's brightness to how far its colour stands out from the
    /// backdrop; below 1 keeps the dim back rows readable.
    pub curve: f32,
}

impl Style {
    pub fn octants() -> Self {
        Self { glyphs: OCTANTS, curve: 0.6 }
    }

    /// Six patterns lie outside the octant block and are often missing (Konsole shows empty
    /// boxes), so each is drawn as the nearest pattern with one more eighth lit.
    pub fn cautious_octants() -> Self {
        let mut glyphs = OCTANTS;
        for (pattern, stand_in) in STAND_INS {
            glyphs[pattern] = OCTANTS[stand_in];
        }
        Self { glyphs, curve: 0.6 }
    }

    /// Braille dots cover far less of the cell than octants; the curve makes up for it.
    pub fn braille() -> Self {
        Self { glyphs: braille(), curve: 0.35 }
    }
}

/// (pattern, stand-in) for the characters outside the octant block: a corner eighth
/// grows into its quadrant, the middle two eighths of a side gain the one above.
const STAND_INS: [(usize, usize); 6] = [
    (0b0000_0001, 0b0000_0101),
    (0b0000_0010, 0b0000_1010),
    (0b0100_0000, 0b0101_0000),
    (0b1000_0000, 0b1010_0000),
    (0b0001_0100, 0b0001_0101),
    (0b0010_1000, 0b0010_1010),
];

pub struct Paint<'a> {
    /// Colours the lines by level.
    pub gradient: &'a Gradient,
    /// The level at which the gradient reaches its end.
    pub top_level: f32,
    /// The brightness drawn at the gradient's full colour.
    pub full: f32,
    /// What dim lines fade toward: the background, set or the terminal's.
    pub backdrop: Rgb,
    /// The background to fill the cells with; `None` leaves the terminal's.
    pub background: Option<Rgb>,
}

impl Paint<'_> {
    /// The colour of a cell whose brightest dot is `brightness` at `level`.
    pub(super) fn colour(&self, brightness: f32, level: f32, curve: f32) -> Rgb {
        let strength = if self.full > 0.0 { (brightness / self.full).min(1.0).powf(curve) } else { 1.0 };
        let along = if self.top_level > 0.0 { level / self.top_level } else { 0.0 };
        mix(self.backdrop, self.gradient.at(along), strength)
    }
}

fn braille() -> Glyphs {
    // Braille numbers its dots down the left column, then the right, bottom pair last.
    const DOT: [u32; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];
    std::array::from_fn(|pattern| {
        0x2800 + (0..8).filter(|bit| pattern & (1 << bit) != 0).map(|bit| DOT[bit]).sum::<u32>()
    })
}

/// Appends the canvas to `out` as lines of cells, leaving the cursor where it started.
pub fn encode(canvas: &Canvas, style: &Style, paint: &Paint<'_>, out: &mut Vec<u8>) {
    let (across, down) = CELL_DOTS;
    let (cols, lines) = (canvas.width / across, canvas.height / down);
    let mut utf8 = [0u8; 4];
    for line in 0..lines {
        if let Some([r, g, b]) = paint.background {
            let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
        }
        let mut colour = None;
        for col in 0..cols {
            let at = |row: usize, side: usize| (line * down + row) * canvas.width + col * across + side;
            let mut brightest = 0.0f32;
            for row in 0..down {
                for side in 0..across {
                    brightest = brightest.max(canvas.pixels[at(row, side)]);
                }
            }
            let mut pattern = 0usize;
            let mut level = 0.0f32;
            for row in 0..down {
                for side in 0..across {
                    let value = canvas.pixels[at(row, side)];
                    if value > LIT && value >= brightest * KEEP {
                        pattern |= 1 << (row * across + side);
                        level = level.max(canvas.levels[at(row, side)]);
                    }
                }
            }
            if pattern == 0 {
                out.push(b' ');
                continue;
            }
            let next = paint.colour(brightest, level, style.curve);
            if colour != Some(next) {
                let [r, g, b] = next;
                let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
                colour = Some(next);
            }
            let glyph = char::from_u32(style.glyphs[pattern]).unwrap_or(' ');
            out.extend_from_slice(glyph.encode_utf8(&mut utf8).as_bytes());
        }
        out.extend_from_slice(b"\x1b[0m");
        if line + 1 < lines {
            out.extend_from_slice(b"\r\n");
        }
    }
    if lines > 1 {
        let _ = write!(out, "\x1b[{}A", lines - 1);
    }
    out.push(b'\r');
}

#[cfg(test)]
mod tests {
    use super::super::theme::Theme;
    use super::*;

    /// White on black, full colour at brightness 1, the gradient's end at level 1.
    fn white_on_black(gradient: &Gradient, background: Option<Rgb>) -> Paint<'_> {
        Paint { gradient, top_level: 1.0, full: 1.0, backdrop: [0; 3], background }
    }

    #[test]
    fn a_dim_dot_beside_a_bright_one_is_left_out() {
        let mut canvas = Canvas::new(2, 4);
        // A bright dot top left; below it one nearly as bright and one far
        // dimmer.
        canvas.pixels[0] = 1.0;
        canvas.pixels[2] = 0.9;
        canvas.pixels[4] = 0.3;
        let theme = Theme::terminal();
        let mut out = Vec::new();
        encode(&canvas, &Style::braille(), &white_on_black(&theme.gradients[0], None), &mut out);
        // Dots 1 and 2, without dot 3.
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b[38;2;255;255;255m\u{2803}\x1b[0m\r");
    }

    #[test]
    fn a_cell_takes_its_colour_from_its_level_and_its_strength_from_its_brightness() {
        let gradient = Gradient::through("g", [[0, 0, 255], [128, 0, 128], [255, 0, 0]]);
        let paint = Paint {
            gradient: &gradient,
            top_level: 0.5,
            full: 0.8,
            backdrop: [0, 0, 0x20],
            background: Some([0, 0, 0x20]),
        };
        // A flat row is the start colour; a quarter of full scale is half
        // way along, since the gradient ends at a half; above that the end.
        assert_eq!(paint.colour(0.8, 0.0, 1.0), [0, 0, 255]);
        assert_eq!(paint.colour(0.8, 0.25, 1.0), [128, 0, 128]);
        assert_eq!(paint.colour(0.8, 0.9, 1.0), [255, 0, 0]);
        // Half as bright stands half as far from the backdrop.
        assert_eq!(paint.colour(0.4, 0.9, 1.0), [128, 0, 16]);

        // The background fills every line, lit cells or not.
        let mut canvas = Canvas::new(4, 8);
        canvas.pixels[0] = 0.8;
        let mut out = Vec::new();
        encode(&canvas, &Style::braille(), &paint, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\x1b[48;2;0;0;32m\x1b[38;2;0;0;255m\u{2801} \x1b[0m\r\n\x1b[48;2;0;0;32m  \x1b[0m\x1b[1A\r"
        );
    }

    #[test]
    fn braille_patterns_follow_the_dot_numbering() {
        let braille = braille();
        assert_eq!(braille[0], 0x2800);
        assert_eq!(braille[0xFF], 0x28FF);
        // Top left is dot 1, top right dot 4, bottom left dot 7, bottom
        // right dot 8.
        assert_eq!(braille[0b0000_0001], 0x2801);
        assert_eq!(braille[0b0000_0010], 0x2808);
        assert_eq!(braille[0b0100_0000], 0x2840);
        assert_eq!(braille[0b1000_0000], 0x2880);
        // The whole left column.
        assert_eq!(braille[0b0101_0101], 0x2847);
    }

    #[test]
    fn cautious_octants_leave_out_the_six_outside_the_block() {
        let missing = [0x1CEA8, 0x1CEAB, 0x1CEA3, 0x1CEA0, 0x1FBE6, 0x1FBE7];
        let glyphs = Style::cautious_octants().glyphs;
        assert!(missing.iter().all(|code| OCTANTS.contains(code)));
        assert!(!glyphs.iter().any(|code| missing.contains(code)));
        // Every other pattern is as it was, and a stand-in keeps all of
        // the pattern it stands for.
        let changed: Vec<usize> = (0..256).filter(|&pattern| glyphs[pattern] != OCTANTS[pattern]).collect();
        assert_eq!(changed.len(), missing.len());
        for (pattern, stand_in) in STAND_INS {
            assert!(changed.contains(&pattern));
            assert_eq!(pattern & stand_in, pattern);
            assert_eq!((pattern ^ stand_in).count_ones(), 1);
        }
    }

    #[test]
    fn every_octant_pattern_has_its_own_character() {
        let octants = &OCTANTS;
        let mut seen = std::collections::HashSet::new();
        for code in octants {
            assert!(char::from_u32(*code).is_some());
            assert!(seen.insert(*code), "{code:#X} twice");
        }
        assert_eq!(octants[0], u32::from(' '));
        assert_eq!(octants[0xFF], 0x2588);
        // The halves are the long-standing block characters.
        assert_eq!(octants[0b0000_1111], 0x2580);
        assert_eq!(octants[0b1111_0000], 0x2584);
        assert_eq!(octants[0b0101_0101], 0x258C);
        assert_eq!(octants[0b1010_1010], 0x2590);
        // The first of the octant block proper: the left eighth of the
        // second row.
        assert_eq!(octants[0b0000_0100], 0x1CD00);
    }

    #[test]
    fn cells_carry_their_dots_and_fade_with_their_brightness() {
        let mut canvas = Canvas::new(4, 8);
        // Top-left dot of the first cell, full brightness; bottom-right
        // dot of the cell below and to the right, dim.
        canvas.pixels[0] = 1.0;
        canvas.pixels[7 * 4 + 3] = 0.25;
        let theme = Theme::terminal();
        let style = Style::braille();
        let mut out = Vec::new();
        encode(&canvas, &style, &white_on_black(&theme.gradients[0], None), &mut out);
        let text = String::from_utf8(out).unwrap();
        let dim = (0.25f32.powf(style.curve) * 255.0 + 0.5) as u8;
        assert_eq!(
            text,
            format!("\x1b[38;2;255;255;255m\u{2801} \x1b[0m\r\n \x1b[38;2;{dim};{dim};{dim}m\u{2880}\x1b[0m\x1b[1A\r")
        );
    }
}
