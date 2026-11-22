//! Colour themes: a background, a text colour and a few gradients each. A gradient is
//! three colours that colour a line by its height above its row's baseline.
//! The built-in themes use the colours of community colour schemes listed in the README.

pub type Rgb = [u8; 3];

/// A built-in theme as it is written down: colours as `0xRRGGBB`.
struct Scheme {
    name: &'static str,
    background: u32,
    text: u32,
    gradients: &'static [(&'static str, [u32; 3])],
}

const SCHEMES: &[Scheme] = &[
    Scheme {
        name: "greyscale",
        background: 0x000000,
        text: 0xbbbbbb,
        gradients: &[
            ("grey-white", [0x808080, 0xbbbbbb, 0xffffff]),
            ("white", [0xffffff, 0xffffff, 0xffffff]),
            ("white-grey", [0xffffff, 0xbbbbbb, 0x777777]),
            ("silver", [0x999999, 0xaaaaaa, 0xcccccc]),
        ],
    },
    Scheme {
        name: "catppuccin-macchiato",
        background: 0x24273a,
        text: 0xcad3f5,
        gradients: &[
            ("blue-pink", [0x8aadf4, 0xc6a0f6, 0xf5bde6]),
            ("green-sapphire", [0xa6da95, 0x8bd5ca, 0x7dc4e4]),
            ("yellow-red", [0xeed49f, 0xf5a97f, 0xed8796]),
            ("mauve-peach", [0xc6a0f6, 0xf5bde6, 0xf5a97f]),
        ],
    },
    Scheme {
        name: "catppuccin-mocha",
        background: 0x1e1e2e,
        text: 0xcdd6f4,
        gradients: &[
            ("blue-pink", [0x89b4fa, 0xcba6f7, 0xf5c2e7]),
            ("green-sapphire", [0xa6e3a1, 0x94e2d5, 0x74c7ec]),
            ("yellow-red", [0xf9e2af, 0xfab387, 0xf38ba8]),
            ("mauve-peach", [0xcba6f7, 0xf5c2e7, 0xfab387]),
        ],
    },
    Scheme {
        name: "catppuccin-frappe",
        background: 0x303446,
        text: 0xc6d0f5,
        gradients: &[
            ("blue-pink", [0x8caaee, 0xca9ee6, 0xf4b8e4]),
            ("green-sapphire", [0xa6d189, 0x81c8be, 0x85c1dc]),
            ("yellow-red", [0xe5c890, 0xef9f76, 0xe78284]),
            ("mauve-peach", [0xca9ee6, 0xf4b8e4, 0xef9f76]),
        ],
    },
    Scheme {
        name: "catppuccin-latte",
        background: 0xeff1f5,
        text: 0x4c4f69,
        gradients: &[
            ("blue-pink", [0x1e66f5, 0x8839ef, 0xea76cb]),
            ("green-sapphire", [0x40a02b, 0x179299, 0x209fb5]),
            ("yellow-red", [0xdf8e1d, 0xfe640b, 0xd20f39]),
            ("mauve-peach", [0x8839ef, 0xea76cb, 0xfe640b]),
        ],
    },
    Scheme {
        name: "dracula",
        background: 0x282a36,
        text: 0xf8f8f2,
        gradients: &[
            ("purple-green", [0xbd93f9, 0x8be9fd, 0x50fa7b]),
            ("purple-orange", [0xbd93f9, 0xff79c6, 0xffb86c]),
            ("cyan-pink", [0x8be9fd, 0xbd93f9, 0xff79c6]),
            ("yellow-red", [0xf1fa8c, 0xffb86c, 0xff5555]),
        ],
    },
    Scheme {
        name: "nord",
        background: 0x2e3440,
        text: 0xd8dee9,
        gradients: &[
            ("frost", [0x5e81ac, 0x81a1c1, 0x88c0d0]),
            ("aurora", [0xa3be8c, 0xebcb8b, 0xbf616a]),
            ("frost-aurora", [0x81a1c1, 0xb48ead, 0xbf616a]),
            ("ice-sun", [0x88c0d0, 0xa3be8c, 0xebcb8b]),
        ],
    },
    Scheme {
        name: "nordic",
        background: 0x242933,
        text: 0xbbc3d4,
        gradients: &[
            ("frost", [0x5e81ac, 0x81a1c1, 0x88c0d0]),
            ("aurora", [0xb1c89d, 0xefd49f, 0xc5727a]),
            ("frost-aurora", [0x81a1c1, 0xbe9db8, 0xc5727a]),
            ("ice-sun", [0x9fc6c5, 0xb1c89d, 0xefd49f]),
        ],
    },
    Scheme {
        name: "gruvbox",
        background: 0x282828,
        text: 0xebdbb2,
        gradients: &[
            ("green-red", [0xb8bb26, 0xfabd2f, 0xfb4934]),
            ("blue-yellow", [0x83a598, 0x8ec07c, 0xfabd2f]),
            ("blue-red", [0x83a598, 0xd3869b, 0xfb4934]),
            ("yellow-red", [0xfabd2f, 0xfe8019, 0xfb4934]),
        ],
    },
    Scheme {
        name: "gruvbox-hard",
        background: 0x1d2021,
        text: 0xebdbb2,
        gradients: &[
            ("green-red", [0xb8bb26, 0xfabd2f, 0xfb4934]),
            ("blue-yellow", [0x83a598, 0x8ec07c, 0xfabd2f]),
            ("blue-red", [0x83a598, 0xd3869b, 0xfb4934]),
            ("yellow-red", [0xfabd2f, 0xfe8019, 0xfb4934]),
        ],
    },
    Scheme {
        name: "solarized-dark",
        background: 0x002b36,
        text: 0x839496,
        gradients: &[
            ("cyan-yellow", [0x2aa198, 0x859900, 0xb58900]),
            ("blue-magenta", [0x268bd2, 0x6c71c4, 0xd33682]),
            ("yellow-red", [0xb58900, 0xcb4b16, 0xdc322f]),
            ("blue-green", [0x268bd2, 0x2aa198, 0x859900]),
        ],
    },
    Scheme {
        name: "solarized-light",
        background: 0xfdf6e3,
        text: 0x657b83,
        gradients: &[
            ("cyan-yellow", [0x2aa198, 0x859900, 0xb58900]),
            ("blue-magenta", [0x268bd2, 0x6c71c4, 0xd33682]),
            ("yellow-red", [0xb58900, 0xcb4b16, 0xdc322f]),
            ("blue-green", [0x268bd2, 0x2aa198, 0x859900]),
        ],
    },
    Scheme {
        name: "tokyo-night",
        background: 0x1a1b26,
        text: 0xa9b1d6,
        gradients: &[
            ("blue-red", [0x7aa2f7, 0xbb9af7, 0xf7768e]),
            ("teal-blue", [0x73daca, 0x7dcfff, 0x7aa2f7]),
            ("green-orange", [0x9ece6a, 0xe0af68, 0xff9e64]),
            ("magenta-orange", [0xbb9af7, 0xf7768e, 0xff9e64]),
        ],
    },
    Scheme {
        name: "tokyo-storm",
        background: 0x24283b,
        text: 0xa9b1d6,
        gradients: &[
            ("blue-red", [0x7aa2f7, 0xbb9af7, 0xf7768e]),
            ("teal-blue", [0x73daca, 0x7dcfff, 0x7aa2f7]),
            ("green-orange", [0x9ece6a, 0xe0af68, 0xff9e64]),
            ("magenta-orange", [0xbb9af7, 0xf7768e, 0xff9e64]),
        ],
    },
    Scheme {
        name: "gotham",
        background: 0x0c1014,
        text: 0x99d1ce,
        gradients: &[
            ("cyan-white", [0x33859e, 0x599cab, 0xd3ebe9]),
            ("green-orange", [0x2aa889, 0xedb443, 0xd26937]),
            ("cyan-red", [0x33859e, 0xedb443, 0xc23127]),
            ("blue-green", [0x599cab, 0x2aa889, 0xedb443]),
        ],
    },
    Scheme {
        name: "rose-pine",
        background: 0x191724,
        text: 0xe0def4,
        gradients: &[
            ("foam-iris", [0x9ccfd8, 0xc4a7e7, 0xebbcba]),
            ("pine-gold", [0x31748f, 0x9ccfd8, 0xf6c177]),
            ("iris-love", [0xc4a7e7, 0xebbcba, 0xeb6f92]),
            ("gold-love", [0xf6c177, 0xebbcba, 0xeb6f92]),
        ],
    },
    Scheme {
        name: "rose-pine-moon",
        background: 0x232136,
        text: 0xe0def4,
        gradients: &[
            ("foam-iris", [0x9ccfd8, 0xc4a7e7, 0xea9a97]),
            ("pine-gold", [0x3e8fb0, 0x9ccfd8, 0xf6c177]),
            ("iris-love", [0xc4a7e7, 0xea9a97, 0xeb6f92]),
            ("gold-love", [0xf6c177, 0xea9a97, 0xeb6f92]),
        ],
    },
    Scheme {
        name: "rose-pine-dawn",
        background: 0xfaf4ed,
        text: 0x464261,
        gradients: &[
            ("foam-iris", [0x56949f, 0x907aa9, 0xd7827e]),
            ("pine-gold", [0x286983, 0x56949f, 0xea9d34]),
            ("iris-love", [0x907aa9, 0xd7827e, 0xb4637a]),
            ("gold-love", [0xea9d34, 0xd7827e, 0xb4637a]),
        ],
    },
    Scheme {
        name: "everforest",
        background: 0x2d353b,
        text: 0xd3c6aa,
        gradients: &[
            ("green-red", [0xa7c080, 0xdbbc7f, 0xe67e80]),
            ("aqua-yellow", [0x83c092, 0xa7c080, 0xdbbc7f]),
            ("blue-purple", [0x7fbbb3, 0xd699b6, 0xe67e80]),
            ("yellow-red", [0xdbbc7f, 0xe69875, 0xe67e80]),
        ],
    },
    Scheme {
        name: "kanagawa",
        background: 0x1f1f28,
        text: 0xdcd7ba,
        gradients: &[
            ("blue-yellow", [0x7e9cd8, 0x98bb6c, 0xe6c384]),
            ("violet-pink", [0x957fb8, 0xd27e99, 0xffa066]),
            ("aqua-red", [0x7aa89f, 0xe6c384, 0xe46876]),
            ("yellow-red", [0xe6c384, 0xffa066, 0xff5d62]),
        ],
    },
    Scheme {
        name: "kanagawa-dragon",
        background: 0x181616,
        text: 0xc5c9c5,
        gradients: &[
            ("blue-yellow", [0x8ba4b0, 0x87a987, 0xc4b28a]),
            ("violet-pink", [0x8992a7, 0xa292a3, 0xb6927b]),
            ("aqua-red", [0x8ea4a2, 0xc4b28a, 0xc4746e]),
            ("yellow-red", [0xc4b28a, 0xb6927b, 0xc4746e]),
        ],
    },
    Scheme {
        name: "one-dark",
        background: 0x282c34,
        text: 0xabb2bf,
        gradients: &[
            ("blue-purple", [0x61afef, 0xc678dd, 0xe06c75]),
            ("cyan-green", [0x56b6c2, 0x98c379, 0xe5c07b]),
            ("green-red", [0x98c379, 0xe5c07b, 0xe06c75]),
            ("blue-yellow", [0x61afef, 0x56b6c2, 0xe5c07b]),
        ],
    },
    Scheme {
        name: "ayu",
        background: 0x10141c,
        text: 0xbfbdb6,
        gradients: &[
            ("blue-purple", [0x39bae6, 0x59c2ff, 0xd2a6ff]),
            ("green-orange", [0xaad94c, 0xffb454, 0xff8f40]),
            ("teal-yellow", [0x95e6cb, 0xaad94c, 0xffb454]),
            ("yellow-red", [0xffb454, 0xff8f40, 0xf07178]),
        ],
    },
    Scheme {
        name: "ayu-mirage",
        background: 0x242936,
        text: 0xcccac2,
        gradients: &[
            ("blue-purple", [0x5ccfe6, 0x73d0ff, 0xdfbfff]),
            ("green-orange", [0xd5ff80, 0xffcd66, 0xffa659]),
            ("teal-yellow", [0x95e6cb, 0xd5ff80, 0xffcd66]),
            ("yellow-red", [0xffcd66, 0xffa659, 0xf28779]),
        ],
    },
];

const fn rgb(colour: u32) -> Rgb {
    [(colour >> 16) as u8, (colour >> 8) as u8, colour as u8]
}

/// A run of colour from `stops[0]` through `stops[1]` to `stops[2]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub name: String,
    stops: [Rgb; 3],
}

impl Gradient {
    #[cfg(test)]
    pub fn through(name: &str, stops: [Rgb; 3]) -> Self {
        Self { name: name.to_string(), stops }
    }

    fn flat(name: &str, colour: Rgb) -> Self {
        Self { name: name.to_string(), stops: [colour; 3] }
    }

    /// The colour `t` of the way along, 0...1.
    pub fn at(&self, t: f32) -> Rgb {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) } * 2.0;
        if t <= 1.0 { mix(self.stops[0], self.stops[1], t) } else { mix(self.stops[1], self.stops[2], t - 1.0) }
    }
}

/// `t` of the way from `a` to `b`, 0...1.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t + 0.5) as u8)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    /// `None` leaves the terminal's own background showing.
    pub background: Option<Rgb>,
    pub text: Rgb,
    /// Never empty. One is always "text": the text colour throughout.
    pub gradients: Vec<Gradient>,
}

/// The terminal theme's gradient of the terminal's palette.
const PALETTE: &str = "palette";

impl Theme {
    /// White lines on the terminal's own background, used until its colours are known.
    /// Palette is there to be chosen, flat until the terminal reports one.
    pub fn terminal() -> Self {
        let mut theme = Self::terminal_in([255; 3]);
        theme.gradients.push(Gradient::flat(PALETTE, theme.text));
        theme
    }

    /// Lines in `text` on the terminal's own background.
    fn terminal_in(text: Rgb) -> Self {
        Self { name: "terminal".to_string(), background: None, text, gradients: vec![Gradient::flat("text", text)] }
    }

    /// Lines in the reported text colour, or failing that white, or black on a light
    /// background; and a gradient of the terminal's palette where it reports one.
    pub fn terminal_reporting(reported: &Reported) -> Self {
        let light = reported.background.is_some_and(is_light);
        let mut theme = Self::terminal_in(reported.text.unwrap_or(if light { [0; 3] } else { [255; 3] }));
        if let Some(stops) = palette_stops(&reported.accents, reported.background) {
            theme.gradients.push(Gradient { name: PALETTE.to_string(), stops });
        }
        theme
    }

    fn of(scheme: &Scheme) -> Self {
        let text = rgb(scheme.text);
        let mut gradients: Vec<Gradient> = scheme
            .gradients
            .iter()
            .map(|(name, stops)| Gradient { name: name.to_string(), stops: stops.map(rgb) })
            .collect();
        gradients.push(Gradient::flat("text", text));
        Self { name: scheme.name.to_string(), background: Some(rgb(scheme.background)), text, gradients }
    }

    /// Every theme's name, in the order they are cycled through.
    pub fn names() -> impl Iterator<Item = &'static str> {
        std::iter::once("terminal").chain(SCHEMES.iter().map(|scheme| scheme.name))
    }

    pub fn named(name: &str) -> Option<Self> {
        if name == "terminal" {
            return Some(Self::terminal());
        }
        SCHEMES.iter().find(|scheme| scheme.name == name).map(Self::of)
    }

    /// Parses a theme file: optional `background = #rrggbb` and `text = #rrggbb`, and
    /// `gradient NAME = ...` lines of one to three colours, cold end first.
    pub fn parse(name: &str, text: &str) -> Result<Self, String> {
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err("its name has to be a single word".to_string());
        }
        if name == "terminal" {
            return Err("terminal is the name of the theme that leaves the terminal's colours alone".to_string());
        }
        let mut background = None;
        let mut plain = None;
        let mut gradients: Vec<Gradient> = Vec::new();
        for (line, entry) in super::config::entries(text) {
            let fail = |problem: String| format!("line {line}: {problem}");
            let (key, value) = entry.ok_or_else(|| fail("expected key = value".to_string()))?;
            let colours = value.split_whitespace().map(hex).collect::<Option<Vec<Rgb>>>();
            let colour = || match colours.as_deref() {
                Some([colour]) => Ok(*colour),
                _ => Err(fail(format!("{key} needs one colour, like #24273a"))),
            };
            match key.split_whitespace().collect::<Vec<_>>()[..] {
                ["background"] => background = Some(colour()?),
                ["text"] => plain = Some(colour()?),
                ["gradient", "text"] => {
                    return Err(fail("text is the name of the gradient every theme has".to_string()));
                }
                ["gradient", gradient] => {
                    if gradients.iter().any(|other| other.name == gradient) {
                        return Err(fail(format!("gradient {gradient} is there twice")));
                    }
                    let stops = match colours.as_deref() {
                        Some([one]) => [*one; 3],
                        Some([from, to]) => [*from, mix(*from, *to, 0.5), *to],
                        Some([from, through, to]) => [*from, *through, *to],
                        _ => return Err(fail(format!("gradient {gradient} needs one to three colours, like #24273a"))),
                    };
                    gradients.push(Gradient { name: gradient.to_string(), stops });
                }
                _ => return Err(fail(format!("{key} is not something a theme has"))),
            }
        }
        let light = background.is_some_and(is_light);
        let text = plain.unwrap_or(if light { [0; 3] } else { [255; 3] });
        gradients.push(Gradient::flat("text", text));
        Ok(Self { name: name.to_string(), background, text, gradients })
    }

    pub fn gradient_index(&self, name: &str) -> Option<usize> {
        self.gradients.iter().position(|gradient| gradient.name == name)
    }

    pub fn gradient_names(&self) -> String {
        self.gradients.iter().map(|gradient| gradient.name.as_str()).collect::<Vec<_>>().join(", ")
    }
}

/// The colour written as `#rrggbb`, with or without the `#`.
fn hex(colour: &str) -> Option<Rgb> {
    let digits = colour.strip_prefix('#').unwrap_or(colour);
    if digits.len() != 6 || !digits.bytes().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(rgb)
}

/// The themes to choose from, in the order they are cycled through.
#[derive(Clone, Debug, PartialEq)]
pub struct Themes(Vec<Theme>);

impl Themes {
    pub fn built_in() -> Self {
        Self(Theme::names().filter_map(Theme::named).collect())
    }

    /// Adds a theme at the end, or in place of the one of the same name.
    pub fn add(&mut self, theme: Theme) {
        match self.0.iter_mut().find(|other| other.name == theme.name) {
            Some(other) => *other = theme,
            None => self.0.push(theme),
        }
    }

    /// The theme called `name`, or an error naming the ones there are.
    pub fn load(&self, name: &str) -> Result<Theme, String> {
        self.0.iter().find(|theme| theme.name == name).cloned().ok_or_else(|| {
            let names: Vec<&str> = self.0.iter().map(|theme| theme.name.as_str()).collect();
            format!("no theme {name}; there are: {}", names.join(", "))
        })
    }

    pub fn next(&self, theme: &Theme) -> Theme {
        let at = self.0.iter().position(|other| other.name == theme.name).map_or(0, |at| (at + 1) % self.0.len());
        self.0.get(at).cloned().unwrap_or_else(Theme::terminal)
    }
}

/// Whether a background is light enough to want dark text on it.
fn is_light([r, g, b]: Rgb) -> bool {
    u32::from(r) + u32::from(g) + u32::from(b) > 382
}

/// How many palette colours are neither black nor white: 1 to 6.
const ACCENTS: usize = 6;

/// What a terminal reports of its colours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reported {
    pub text: Option<Rgb>,
    pub background: Option<Rgb>,
    /// Palette colours 1 to 6.
    pub accents: [Option<Rgb>; ACCENTS],
}

impl Reported {
    /// The colours in a terminal's whole replies to OSC 10, 11 and 4 queries.
    pub fn parse(reply: &str) -> Self {
        let answer = |code: &str| {
            let (_, rest) = reply.split_once(&format!("]{code};"))?;
            reported_colour(rest.split_once(['\x1b', '\x07'])?.0)
        };
        Self {
            text: answer("10"),
            background: answer("11"),
            accents: std::array::from_fn(|i| answer(&format!("4;{}", i + 1))),
        }
    }

    pub fn any(&self) -> bool {
        *self != Self::default()
    }

    /// These colours, with `newer`'s where it has them.
    pub fn with(self, newer: Self) -> Self {
        Self {
            text: newer.text.or(self.text),
            background: newer.background.or(self.background),
            accents: std::array::from_fn(|i| newer.accents[i].or(self.accents[i])),
        }
    }
}

/// How far a colour is from grey: the spread of its channels.
fn vividness([r, g, b]: Rgb) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

/// A colour drawn on a background differs from it by more than this, summed over its channels.
const MIN_APART: i32 = 120;

/// A gradient's colours from a palette's: the least vivid, the middling and the most
/// vivid of those that show on the background. `None` with fewer than three.
fn palette_stops(accents: &[Option<Rgb>], background: Option<Rgb>) -> Option<[Rgb; 3]> {
    let apart = |a: &Rgb, b: &Rgb| (0..3).map(|i| (i32::from(a[i]) - i32::from(b[i])).abs()).sum::<i32>();
    let shows = |colour: &Rgb| background.is_none_or(|behind| apart(colour, &behind) > MIN_APART);
    let mut colours: Vec<Rgb> = accents.iter().flatten().copied().filter(shows).collect();
    colours.sort_by_key(|colour| vividness(*colour));
    match colours[..] {
        [mildest, _, .., vividest] => Some([mildest, colours[colours.len() / 2], vividest]),
        _ => None,
    }
}

/// The colour in a terminal's answer to an OSC 4, 10 or 11 query:
/// `rgb:RRRR/GGGG/BBBB`, each part one to four hex digits.
fn reported_colour(reply: &str) -> Option<Rgb> {
    let (_, rest) = reply.split_once("rgb:")?;
    let mut parts = rest.split('/');
    let mut channel = || {
        let digits: String = parts.next()?.chars().take_while(char::is_ascii_hexdigit).collect();
        if !(1..=4).contains(&digits.len()) {
            return None;
        }
        let value = u32::from_str_radix(&digits, 16).ok()?;
        let full = (1u32 << (4 * digits.len() as u32)) - 1;
        Some((value * 255 / full) as u8)
    };
    Some([channel()?, channel()?, channel()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gradient_runs_through_its_middle() {
        let gradient = Gradient { name: "g".into(), stops: [[0, 0, 0], [100, 0, 0], [100, 200, 0]] };
        assert_eq!((gradient.at(0.0), gradient.at(0.5), gradient.at(1.0)), ([0, 0, 0], [100, 0, 0], [100, 200, 0]));
        assert_eq!((gradient.at(0.25), gradient.at(0.75)), ([50, 0, 0], [100, 100, 0]));
        assert_eq!((gradient.at(-1.0), gradient.at(9.0), gradient.at(f32::NAN)), ([0, 0, 0], [100, 200, 0], [0, 0, 0]));
    }

    #[test]
    fn every_theme_is_well_formed_and_they_cycle_in_order() {
        let themes = Themes::built_in();
        let mut theme = Theme::terminal();
        let mut seen = Vec::new();
        for _ in 0..Theme::names().count() {
            assert!(theme.gradient_index("text").is_some(), "{}", theme.name);
            if theme.name != "terminal" {
                let background = theme.background.unwrap();
                assert!(theme.gradients.len() >= 4, "{}", theme.name);
                // Every flat row is drawn in a gradient's first colour, so
                // it has to stand out from the background.
                for gradient in &theme.gradients {
                    let start = gradient.at(0.0);
                    let apart: i32 = (0..3).map(|i| (i32::from(start[i]) - i32::from(background[i])).abs()).sum();
                    assert!(apart > 120, "{} {}: {apart}", theme.name, gradient.name);
                }
                let mut names: Vec<&str> = theme.gradients.iter().map(|gradient| gradient.name.as_str()).collect();
                names.sort_unstable();
                names.dedup();
                assert_eq!(names.len(), theme.gradients.len(), "{}", theme.name);
            }
            assert!(!seen.contains(&theme.name), "{} twice", theme.name);
            seen.push(theme.name.clone());
            theme = themes.next(&theme);
        }
        assert_eq!(theme.name, "terminal");
        assert_eq!(seen, Theme::names().collect::<Vec<_>>());
    }

    #[test]
    fn themes_are_found_by_name() {
        let themes = Themes::built_in();
        let theme = themes.load("catppuccin-macchiato").unwrap();
        assert_eq!((theme.background, theme.text), (Some([0x24, 0x27, 0x3a]), [0xca, 0xd3, 0xf5]));
        assert_eq!(theme.gradients[0].at(0.0), [0x8a, 0xad, 0xf4]);
        assert_eq!((theme.gradient_index("yellow-red"), theme.gradient_index("nope")), (Some(2), None));
        assert!(theme.gradient_names().starts_with("blue-pink, green-sapphire"));
        assert!(
            themes.load("no-such-theme").unwrap_err().contains("there are: terminal, greyscale, catppuccin-macchiato")
        );
    }

    #[test]
    fn a_theme_file_gives_a_background_a_text_colour_and_gradients() {
        let theme = Theme::parse(
            "godzilla",
            "# a comment\nbackground = #0b1410\ntext = c8d6c0\ngradient atomic = #1f6f4a #39d98a #b8fff0\n\
             gradient ember = #000000 #646464\ngradient flat = #010203\n",
        )
        .unwrap();
        assert_eq!(
            (theme.name.as_str(), theme.background, theme.text),
            ("godzilla", Some([0x0b, 0x14, 0x10]), [0xc8, 0xd6, 0xc0])
        );
        assert_eq!(theme.gradient_names(), "atomic, ember, flat, text");
        assert_eq!(theme.gradients[0].at(0.5), [0x39, 0xd9, 0x8a]);
        // Two colours run straight from one to the other; one is flat.
        assert_eq!(theme.gradients[1].at(0.5), [50, 50, 50]);
        assert_eq!((theme.gradients[2].at(0.0), theme.gradients[2].at(1.0)), ([1, 2, 3], [1, 2, 3]));

        // Nothing but a gradient: the terminal's background, white text.
        let bare = Theme::parse("bare", "gradient g = #ff0000 #00ff00").unwrap();
        assert_eq!((bare.background, bare.text), (None, [255; 3]));
        // Text left out on a light background is black.
        assert_eq!(Theme::parse("light", "background = #fafafa").unwrap().text, [0; 3]);
    }

    #[test]
    fn a_theme_file_that_cannot_be_used_says_which_line_and_why() {
        let problem = |text: &str| Theme::parse("t", text).unwrap_err();
        assert_eq!(problem("background = green"), "line 1: background needs one colour, like #24273a");
        assert_eq!(problem("\n\nbackgroud = #000000"), "line 3: backgroud is not something a theme has");
        assert_eq!(
            problem("gradient g = #000000 #111111 #222222 #333333"),
            "line 1: gradient g needs one to three colours, like #24273a"
        );
        assert_eq!(problem("gradient g = #000000\ngradient g = #111111"), "line 2: gradient g is there twice");
        assert!(problem("gradient text = #000000").starts_with("line 1: text is the name"));
        assert_eq!(problem("just words"), "line 1: expected key = value");
        assert!(Theme::parse("two words", "").is_err());
        assert!(Theme::parse("terminal", "").is_err());
    }

    #[test]
    fn an_added_theme_joins_the_end_or_takes_the_place_of_its_namesake() {
        let mut themes = Themes::built_in();
        themes.add(Theme::parse("godzilla", "background = #0b1410").unwrap());
        themes.add(Theme::parse("nord", "background = #000001").unwrap());
        assert_eq!(themes.load("nord").unwrap().background, Some([0, 0, 1]));
        // Still where the built-in nord was, and godzilla is last.
        assert_eq!(themes.next(&themes.load("dracula").unwrap()).background, Some([0, 0, 1]));
        assert_eq!(themes.next(&themes.load("ayu-mirage").unwrap()).name, "godzilla");
        assert_eq!(themes.next(&themes.load("godzilla").unwrap()).name, "terminal");
        assert!(themes.load("nope").unwrap_err().ends_with("ayu-mirage, godzilla"));
    }

    #[test]
    fn the_terminal_theme_takes_the_terminals_own_text_colour() {
        let both = "\x1b]10;rgb:2222/2222/2222\x1b\\\x1b]11;rgb:f5f5/f5f5/f0f0\x1b\\\x1b[?62;c";
        let read = Reported::parse(both);
        assert_eq!((read.text, read.background), (Some([0x22; 3]), Some([0xf5, 0xf5, 0xf0])));
        assert!(read.any() && read.accents == [None; ACCENTS]);
        let read = Reported::parse("\x1b]11;rgb:00/00/00\x07\x1b[?62;c");
        assert_eq!((read.text, read.background), (None, Some([0; 3])));
        assert!(!Reported::parse("\x1b[?62;c").any());

        let reporting =
            |text, background| Theme::terminal_reporting(&Reported { text, background, ..Reported::default() });
        let theme = reporting(Some([0x22; 3]), Some([0xf5; 3]));
        assert_eq!((theme.name.as_str(), theme.text, theme.background), ("terminal", [0x22; 3], None));
        assert_eq!((theme.gradient_names().as_str(), theme.gradients[0].at(1.0)), ("text", [0x22; 3]));
        // With no text colour reported: white, or black on a light
        // background.
        assert_eq!(reporting(None, Some([0xf5; 3])).text, [0; 3]);
        assert_eq!(reporting(None, Some([0x10; 3])).text, [255; 3]);
        // Before any report, palette is there to be asked for.
        assert_eq!(Theme::terminal().gradient_names(), "text, palette");
    }

    #[test]
    fn the_terminals_palette_makes_a_gradient_from_mild_to_vivid() {
        let reply = "\x1b]11;rgb:0202/0505/0909\x07\x1b]4;1;rgb:8181/3e3e/5959\x07\x1b]4;2;rgb:8888/8e8e/5e5e\x07\
                     \x1b]4;3;rgb:4646/b5b5/dbdb\x07\x1b]4;4;rgb:dada/8383/5656\x07\x1b]4;5;rgb:e5e5/2f2f/4242\x07\
                     \x1b]4;6;rgb:d4d4/c7c7/3838\x1b\\";
        let read = Reported::parse(reply);
        assert_eq!((read.accents[0], read.accents[5]), (Some([0x81, 0x3e, 0x59]), Some([0xd4, 0xc7, 0x38])));
        let theme = Theme::terminal_reporting(&read);
        assert_eq!(theme.gradient_names(), "text, palette");
        let palette = &theme.gradients[1];
        // Olive is the nearest to grey, red the furthest from it.
        assert_eq!(
            (palette.at(0.0), palette.at(0.5), palette.at(1.0)),
            ([0x88, 0x8e, 0x5e], [0x46, 0xb5, 0xdb], [0xe5, 0x2f, 0x42])
        );

        // A colour that would not show on the background is passed over.
        let accents = [Some([10, 5, 20]), Some([200, 0, 0]), Some([0, 150, 150]), Some([90, 90, 60]), None, None];
        let hidden = Reported { accents, ..read };
        assert_eq!(Theme::terminal_reporting(&hidden).gradients[1].at(0.0), [90, 90, 60]);
        // Fewer than three colours are not a gradient.
        let few = Reported { accents: [Some([200, 0, 0]), Some([0, 150, 150]), None, None, None, None], ..read };
        assert_eq!(Theme::terminal_reporting(&few).gradient_names(), "text");

        // A reply cut short is not read.
        assert!(!Reported::parse("\x1b]11;rgb:1e1e/1e1e/2").any());

        // A later report changes only what it names.
        let later = read.with(Reported { text: Some([1, 2, 3]), ..Reported::default() });
        assert_eq!((later.text, later.background, later.accents), (Some([1, 2, 3]), read.background, read.accents));
    }

    #[test]
    fn a_terminals_colour_report_is_read_at_any_width() {
        assert_eq!(reported_colour("\x1b]11;rgb:2424/2727/3a3a\x1b\\"), Some([0x24, 0x27, 0x3a]));
        assert_eq!(reported_colour("\x1b]11;rgb:ff/00/80\x07"), Some([255, 0, 128]));
        assert_eq!(reported_colour("\x1b]11;rgb:f/0/8\x07"), Some([255, 0, 136]));
        assert_eq!(reported_colour("\x1b[?62;c"), None);
        assert_eq!(reported_colour("\x1b]11;rgb:ffffffff/00/00\x07"), None);
        assert_eq!(reported_colour("\x1b]11;rgb:/00/00\x07"), None);
    }
}
