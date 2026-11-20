//! The terminal front end: the ridge drawn as characters, in the lines below the prompt
//! or over the whole screen. Each frame is rasterised into dots (`raster`) and written as
//! characters (`cells`, `theme`), or drawn straight into ASCII cells (`ascii`). A timer
//! paces the loop; the ridge moves by elapsed time.

mod ascii;
mod cells;
mod config;
mod octants;
mod raster;
mod theme;
mod tty;

use std::{
    io::Write as _,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    audio::{self, PcmHandler},
    bands, clock,
    ridge::{DrawList, GeometryBuilder, Ridge, RidgeParams, RidgeTarget, sync_lead},
    spectrum::{Analyser, CaptureStats, LiveSpectrum},
};
use config::Config;
use raster::Canvas;
use theme::{Rgb, Theme, Themes};

pub const USAGE: &str = "\
pulsr [options]

Draws what the computer is playing as a ridge, in the terminal.

  -full         take the whole screen, and give it back on quitting (by
                default the ridge is drawn in the lines below the prompt)
  -rising       the front row, which follows the sound as it plays, rises
                from below the bottom edge with the rest instead of standing
                on it
  -settle       in silence the rows settle and stop, instead of
                scrolling on
  -theme NAME   the colours: terminal (the terminal's own text colour
                on its own background, the default) or one of
                greyscale, catppuccin-macchiato, catppuccin-mocha,
                catppuccin-frappe, catppuccin-latte, dracula, nord,
                nordic, gruvbox, gruvbox-hard, solarized-dark,
                solarized-light, tokyo-night, tokyo-storm, gotham,
                rose-pine, rose-pine-moon, rose-pine-dawn, everforest,
                kanagawa, kanagawa-dragon, one-dark, ayu, ayu-mirage
  -gradient NAME
                which of the theme's gradients colours the lines, from the
                flat rows to the peaks: the theme's first by default; each
                theme has its own, and all have text (the theme's text
                colour throughout). The G key shows their names
  -mode MODE    the characters the ridge is drawn with:
                  octant      solid dots (the default in terminals known
                              to have them)
                  braille     separated dots (the default elsewhere; in
                              nearly every font)
                  static      plain ASCII: dashes with gaps that drift
                              along the rows, heavier on the peaks
                  line        plain ASCII: an unbroken line
  -testtone     analyse a synthetic signal instead of the system's audio
  -fps N        frames a second (60 by default)
  -lines N      terminal lines to draw in when not full screen (all but
                one by default)
  -pitch N      dots between rows in octant and braille (5 by default);
                a character cell is 4 dots tall
  -ascii-pitch N
                the same for static and line (10 by default)
  -height N     how far up the lines the stack reaches, as a percentage
                (90 by default)
  -fade N       how far the rows fade toward the back, as a percentage:
                100 fades the back row out altogether, 0 draws every row
                as bright as the front one (80 by default)
  -curve N      the shape of that fade: 1 is even, below 1 keeps the rows
                bright further back, above 1 dims them sooner (0.75 by
                default)
  -seconds N    quit after N seconds
  -stats        on quitting, print what the capture heard and how long the
                frames took

The settings to start with can be kept in ~/.config/pulsr/config (or
under $XDG_CONFIG_HOME), as lines like theme = nord. They are theme, gradient,
mode, full, rising and settle (true or false), fps, lines, pitch,
ascii-pitch, height, fade and curve.
What the keys last chose is saved on quitting, in the file state beside
it, and the next run starts from there; of the two files the one written
later is gone by. An option given here wins over both. Themes of your own go in
the same place, one file each:
themes/NAME.theme adds a theme called NAME. A line of each kind:

  background = #0b1410
  text = #c8d6c0
  gradient atomic = #1f6f4a #39d98a #b8fff0

Keys: Q quits, F switches full screen on and off, T moves to the next
theme, G to the next gradient, M to the next characters, R lets the
front row rise from below the edge or stand on it, S lets the rows
settle and stop in silence or scroll on. [ and ] move
the rows closer together and further apart, - and + lower and raise the
frames a second, , and . the height of the stack, ; and ' the fade, 9
and 0 its curve.";

/// The characters the ridge is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Octant,
    Braille,
    Static,
    Line,
}

impl Mode {
    const ALL: [Self; 4] = [Self::Octant, Self::Braille, Self::Static, Self::Line];

    fn name(self) -> &'static str {
        match self {
            Self::Octant => "octant",
            Self::Braille => "braille",
            Self::Static => "static",
            Self::Line => "line",
        }
    }

    /// The look of an ASCII mode; `None` for the modes drawn in dots.
    fn look(self) -> Option<ascii::Look> {
        match self {
            Self::Octant | Self::Braille => None,
            Self::Static => Some(ascii::Look::Static),
            Self::Line => Some(ascii::Look::Line),
        }
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// Octants where the terminal is known to draw them itself, Braille elsewhere:
    /// a terminal cannot be asked whether it has a character.
    fn for_terminal(term_program: Option<&str>, term: Option<&str>) -> Self {
        let program = term_program.unwrap_or_default().to_ascii_lowercase();
        let term = term.unwrap_or_default().to_ascii_lowercase();
        if program == "ghostty" || term.contains("ghostty") || term.contains("kitty") {
            Self::Octant
        } else {
            Self::Braille
        }
    }
}

const DEFAULT_FPS: f64 = 60.0;
/// Below this a frame takes longer than the ridge allows before it counts as a stall.
const MIN_FPS: f64 = 5.0;
/// Dots between rows: in the modes drawn in dots, and in the ASCII modes, whose rows
/// need more than a cell (4 dots) between them.
const DEFAULT_PITCH: f64 = 5.0;
const DEFAULT_ASCII_PITCH: f64 = 10.0;
/// The fewest and most dots between rows the keys go to.
const KEY_PITCH: (f64, f64) = (2.0, 24.0);
/// The frame rates the keys step through.
const KEY_FPS: [f64; 9] = [10.0, 15.0, 24.0, 30.0, 45.0, 60.0, 90.0, 120.0, 144.0];

/// The next of `KEY_FPS` above or below `fps`.
fn stepped_fps(fps: f64, up: bool) -> f64 {
    let next = if up {
        KEY_FPS.iter().find(|step| **step > fps + 0.5)
    } else {
        KEY_FPS.iter().rev().find(|step| **step < fps - 0.5)
    };
    next.copied().unwrap_or(fps)
}
/// Milliseconds of history the stack holds, however many rows it has.
const STACK_MS: f64 = 2500.0;
/// Height of the stack as a percentage of the lines drawn in.
const DEFAULT_HEIGHT: f64 = 90.0;
/// How far the rows fade toward the back, as a percentage.
const DEFAULT_FADE: f64 = 80.0;
/// The fade's exponent.
const DEFAULT_CURVE: f64 = 0.75;
/// (least, most, key step) for the stack's height, the fade and its curve.
const HEIGHT_RANGE: (f64, f64, f64) = (20.0, 100.0, 5.0);
const FADE_RANGE: (f64, f64, f64) = (0.0, 100.0, 10.0);
const CURVE_RANGE: (f64, f64, f64) = (0.25, 3.0, 0.25);

/// `value` moved to the next multiple of the range's step, up or down, within the range.
fn stepped(value: f64, (least, most, step): (f64, f64, f64), up: bool) -> f64 {
    let at = value / step;
    // A value a hair off a multiple counts as on it.
    let next = if up { (at + 1e-6).floor() + 1.0 } else { (at - 1e-6).ceil() - 1.0 };
    (next * step).clamp(least, most)
}

/// Rise of a full-scale point on the front row as a fraction of the lines drawn in,
/// and the back row's rise relative to it.
const STACK_PEAK: f64 = 0.5;
const STACK_DEPTH_SCALE: f64 = 0.34;
/// The newest row is not drawn until it is a whole row above the front one: rows a
/// dot or two apart read as one thick line.
const EMERGE_DOTS: f64 = f64::INFINITY;
/// Half the thickness of a line, in dots: one dot.
const DOT_HALF_WIDTH: f32 = 0.5;
/// The level at which a line reaches the end of the gradient. Music rarely reaches
/// full scale, so at 1 the hot end would hardly show.
const GRADIENT_TOP: f32 = 0.6;
/// Seconds the status line stays up after a key changes something.
const STATUS_SECONDS: f64 = 1.5;
/// Seconds to wait for the terminal to report its colours.
const REPLY_SECONDS: f64 = 0.3;

/// Seconds of nothing but zero samples before the silence notice goes up.
const SILENCE_SECONDS: f64 = 2.0;
/// Seconds between looks at what the capture has heard.
const SILENCE_POLL: f64 = 0.25;

/// Watches for a capture that runs but has only ever delivered zeros.
#[derive(Default)]
struct SilenceWatch {
    /// When blocks were first seen arriving.
    since: Option<f64>,
    silent: bool,
}

impl SilenceWatch {
    /// Returns whether the notice is due.
    fn update(&mut self, stats: &CaptureStats, now: f64) -> bool {
        if stats.heard_signal || stats.blocks == 0 {
            self.since = None;
            self.silent = false;
        } else {
            let since = *self.since.get_or_insert(now);
            self.silent = now - since >= SILENCE_SECONDS;
        }
        self.silent
    }
}

/// Appends `notice` to `out` on the cursor's line, dim, cut to `cols` columns, leaving
/// the cursor at the start of the line.
fn write_notice(notice: &str, cols: u16, theme: &Theme, out: &mut Vec<u8>) {
    let room = usize::from(cols).saturating_sub(1);
    let text: String = notice.chars().take(room).collect();
    let [r, g, b] = theme.text;
    let _ = write!(out, "\x1b[0;2;38;2;{r};{g};{b}m");
    if let Some([r, g, b]) = theme.background {
        let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
    }
    let _ = write!(out, " {text}\x1b[0m\r");
}

/// Asks the terminal for its text and background colours (OSC 10 and 11). The device
/// attributes query sent last is always answered, which ends the wait. Needs `RawMode`.
fn terminal_colours() -> (Option<Rgb>, Option<Rgb>) {
    tty::write_all(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[c");
    let answered = |reply: &[u8]| {
        reply.windows(3).position(|window| window == b"\x1b[?").is_some_and(|at| reply[at..].contains(&b'c'))
    };
    let reply = tty::read_reply(answered, REPLY_SECONDS);
    theme::reported_colours(&String::from_utf8_lossy(&reply))
}

#[derive(Debug)]
pub struct Options {
    /// `None` picks a mode from the terminal.
    mode: Option<Mode>,
    test_tone: bool,
    fps: f64,
    lines: Option<u16>,
    /// Dots between rows, in the modes drawn in dots and in the ASCII modes.
    pitch: f64,
    ascii_pitch: f64,
    /// Percentages, and the fade's exponent.
    height: f64,
    fade: f64,
    curve: f64,
    seconds: Option<f64>,
    stats: bool,
    full: bool,
    /// The front row rises from below the bottom edge instead of standing on it.
    rising: bool,
    /// In silence the rows come to rest instead of scrolling on.
    settle: bool,
    theme: Theme,
    /// `None` is the theme's first.
    gradient: Option<String>,
    /// The built-in themes and the ones from the configuration directory.
    themes: Themes,
    /// What was left out of the configuration, and why.
    warnings: Vec<String>,
    /// What the keys chose in earlier runs, and where to save it.
    state: config::State,
    config_dir: Option<std::path::PathBuf>,
}

const SETTINGS: [&str; 13] =
    [
    "theme",
    "gradient",
    "mode",
    "full",
    "rising",
    "settle",
    "fps",
    "lines",
    "pitch",
    "ascii-pitch",
    "height",
    "fade",
    "curve",
];

impl Options {
    /// Parses the arguments after the program's name over the configuration directory's
    /// settings. An empty error asks for the usage text.
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        Self::over(config::dir().map(|dir| config::read(&dir)).unwrap_or_default(), args)
    }

    /// Order: defaults, then the settings file and the saved state (the later written
    /// last), then the arguments. A bad setting is a warning; a bad argument is an error.
    fn over(config: Config, args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut themes = Themes::built_in();
        for theme in config.themes {
            themes.add(theme);
        }
        let mut options = Self {
            mode: None,
            test_tone: false,
            fps: DEFAULT_FPS,
            lines: None,
            pitch: DEFAULT_PITCH,
            ascii_pitch: DEFAULT_ASCII_PITCH,
            height: DEFAULT_HEIGHT,
            fade: DEFAULT_FADE,
            curve: DEFAULT_CURVE,
            seconds: None,
            stats: false,
            full: false,
            rising: false,
            settle: false,
            theme: Theme::terminal(),
            gradient: None,
            themes,
            warnings: config.warnings,
            state: config.state,
            config_dir: config.dir,
        };
        if config.state_is_newer {
            options.apply_settings(&config.settings);
            options.apply_state();
        } else {
            options.apply_state();
            options.apply_settings(&config.settings);
        }

        // The file's gradient belongs to the file's theme: it is dropped
        // when the arguments choose another theme and no gradient.
        let from_config = options.gradient.take();
        let mut theme_chosen = false;
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-full" => options.full = true,
                "-rising" => options.rising = true,
                "-settle" => options.settle = true,
                "-testtone" => options.test_tone = true,
                "-stats" => options.stats = true,
                "-theme" | "-gradient" | "-mode" | "-fps" | "-lines" | "-pitch" | "-ascii-pitch" | "-height"
                | "-fade" | "-curve" | "-seconds" => {
                    let value = args.next().ok_or(format!("{arg} needs a value"))?;
                    options.set(&arg[1..], &value).map_err(|problem| format!("{arg} {problem}"))?;
                    theme_chosen |= arg == "-theme";
                }
                "-h" | "-help" | "--help" => return Err(String::new()),
                other => return Err(format!("unknown option {other}")),
            }
        }
        if options.gradient.is_none() && !theme_chosen {
            options.gradient = from_config;
        }
        options.check_gradient()?;
        Ok(options)
    }

    /// A gradient goes with its theme: one set earlier does not outlast a change of theme.
    fn apply_settings(&mut self, settings: &[config::Setting]) {
        let names = |key: &str| settings.iter().any(|setting| setting.key == key);
        if names("theme") && !names("gradient") {
            self.gradient = None;
        }
        for setting in settings {
            let outcome = if SETTINGS.contains(&setting.key.as_str()) {
                self.set(&setting.key, &setting.value)
            } else {
                Err(format!("is not a setting; the settings are: {}", SETTINGS.join(", ")))
            };
            if let Err(problem) = outcome {
                self.warnings.push(format!("config line {}: {} {problem}", setting.line, setting.key));
            }
        }
        if let Err(problem) = self.check_gradient() {
            self.warnings.push(format!("config: {problem}"));
            self.gradient = None;
        }
    }

    /// Applies what the keys chose in earlier runs; anything no longer usable is skipped.
    fn apply_state(&mut self) {
        let state = self.state.clone();
        let saved = |key: &str| state.get(key).map(String::as_str);
        if let Some(theme) = saved("theme").and_then(|name| self.themes.load(name).ok()) {
            self.gradient = saved("gradient").filter(|name| theme.gradient_index(name).is_some()).map(str::to_string);
            self.theme = theme;
        }
        for key in ["mode", "full", "rising", "settle", "fps", "pitch", "ascii-pitch", "height", "fade", "curve"] {
            if let Some(value) = saved(key) {
                let _ = self.set(key, value);
            }
        }
    }

    /// Sets `key` (a settings-file key, or `-key` as an argument). The error reads on from it.
    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let number = || {
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or("needs a positive number".to_string())
        };
        let switch = || match value {
            "true" | "yes" | "on" => Ok(true),
            "false" | "no" | "off" => Ok(false),
            _ => Err("needs true or false".to_string()),
        };
        match key {
            "theme" => self.theme = self.themes.load(value).map_err(|problem| format!("finds {problem}"))?,
            "gradient" => self.gradient = Some(value.to_string()),
            "mode" => self.mode = Some(Mode::named(value).ok_or("needs octant, braille, static or line")?),
            "full" => self.full = switch()?,
            "rising" => self.rising = switch()?,
            "settle" => self.settle = switch()?,
            "fps" => self.fps = number()?.clamp(MIN_FPS, 1000.0),
            "lines" => self.lines = Some(number()?.round().clamp(2.0, 10_000.0) as u16),
            "pitch" => self.pitch = number()?.max(1.0),
            "ascii-pitch" => self.ascii_pitch = number()?.max(1.0),
            "height" => self.height = number()?.clamp(HEIGHT_RANGE.0, HEIGHT_RANGE.1),
            "fade" => {
                self.fade = value
                    .parse::<f64>()
                    .ok()
                    .filter(|fade| (FADE_RANGE.0..=FADE_RANGE.1).contains(fade))
                    .ok_or("needs a number from 0 to 100")?;
            }
            "curve" => self.curve = number()?.clamp(CURVE_RANGE.0, CURVE_RANGE.1),
            "seconds" => self.seconds = Some(number()?),
            _ => return Err("is not a setting".to_string()),
        }
        Ok(())
    }

    /// An error if the theme has no gradient of the name asked for.
    fn check_gradient(&self) -> Result<(), String> {
        match &self.gradient {
            Some(name) if self.theme.gradient_index(name).is_none() => Err(format!(
                "theme {} has no gradient {name}; it has: {}",
                self.theme.name,
                self.theme.gradient_names()
            )),
            _ => Ok(()),
        }
    }
}

impl Options {
    /// The setting that holds the dots between rows for `mode`, and its value.
    fn pitch(&self, mode: Mode) -> (&'static str, f64) {
        if mode.look().is_some() { ("ascii-pitch", self.ascii_pitch) } else { ("pitch", self.pitch) }
    }

    /// Gives `params` the fade toward the back that was asked for.
    fn set_fade(&self, params: &mut RidgeParams) {
        params.ridge_back_alpha = params.ridge_alpha * (1.0 - self.fade / 100.0);
        params.ridge_fade_curve = self.curve;
    }
}

/// Rows in a stack `height` dots tall with about `pitch` dots between rows.
fn dot_rows(height: u32, pitch: f64, params: &RidgeParams) -> usize {
    let stack = f64::from(height) * params.ridge_height;
    ((stack / pitch).round() as usize + 1).clamp(4, params.ridge_rows)
}

/// A ridge laid out for a canvas of the terminal's dots.
struct Stage {
    size: tty::Size,
    /// Terminal lines drawn in.
    lines: u16,
    target: RidgeTarget,
    ridge: Ridge,
    canvas: Canvas,
    /// The same lines as whole cells, for the ASCII modes.
    grid: ascii::Grid,
}

impl Stage {
    /// Over the whole screen when `full`, else over the lines asked for, with the rows
    /// spaced for `mode`.
    fn new(options: &Options, mode: Mode, size: tty::Size, full: bool, seed: u64) -> Self {
        let lines = if full { size.rows } else { options.lines.unwrap_or(size.rows.saturating_sub(1)) };
        let lines = lines.clamp(2, size.rows.max(2));
        let (across, down) = cells::CELL_DOTS;
        let width = (u32::from(size.cols) * across as u32).max(2);
        let height = (u32::from(lines) * down as u32).max(2);
        let mut params = RidgeParams {
            ridge_height: options.height / 100.0,
            ridge_peak: STACK_PEAK,
            ridge_depth_scale: STACK_DEPTH_SCALE,
            // The front row stands on the last line of dots.
            ridge_bottom: 0.0,
            ridge_emerge: EMERGE_DOTS,
            ridge_rising: options.rising,
            ..RidgeParams::default()
        };
        options.set_fade(&mut params);
        let rows = dot_rows(height, options.pitch(mode).1, &params);
        params.ridge_rows = rows;
        params.ridge_row_ms = STACK_MS / (rows - 1) as f64;
        let mut ridge = Ridge::new(params, seed);
        ridge.keep_moving = !options.settle;
        // One dot to the point, so the layout's pixels are dots.
        let target = RidgeTarget { pixel_width: width, pixel_height: height, scale: 1.0 };
        let canvas = Canvas::new(width as usize, height as usize);
        let grid = ascii::Grid::new(usize::from(size.cols), usize::from(lines));
        Self { size, lines, target, ridge, canvas, grid }
    }
}

/// Sums over the frames of one mode at one size.
#[derive(Default)]
struct Timings {
    frames: u64,
    /// Frames that were not finished by the time the next was due.
    late: u64,
    ridge: f64,
    raster: f64,
    encode: f64,
    write: f64,
    bytes: u64,
}

impl Timings {
    fn line(&self, mode: Mode, stage: &Stage, elapsed: f64) -> Option<String> {
        if self.frames == 0 || !(elapsed > 0.0) {
            return None;
        }
        let n = self.frames as f64;
        let ms = |total: f64| total / n * 1000.0;
        Some(format!(
            "{:<10} {}x{}, {} rows: {:.1} fps over {:.1} s, {} of {} frames late; per frame: \
             ridge {:.2} ms, raster {:.2} ms, encode {:.2} ms, write {:.2} ms, {:.0} KB",
            mode.name(),
            stage.canvas.width,
            stage.canvas.height,
            stage.ridge.row_count(),
            n / elapsed,
            elapsed,
            self.late,
            self.frames,
            ms(self.ridge),
            ms(self.raster),
            ms(self.encode),
            ms(self.write),
            self.bytes as f64 / n / 1024.0,
        ))
    }
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// Hides the cursor and takes the keys while it lives, and tracks where the drawing
/// is. Dropping it clears the drawing and hands the terminal back.
struct Screen {
    _raw: tty::RawMode,
    /// On the alternate screen.
    full: bool,
    /// Lines below the prompt have been made room for.
    room: bool,
    /// The terminal was resized while full screen, so the lines made room for have moved.
    moved: bool,
}

impl Screen {
    fn enter() -> Self {
        let raw = tty::RawMode::enter();
        tty::write_all(b"\x1b[?25l");
        Self { _raw: raw, full: false, room: false, moved: false }
    }

    /// Appends to `out` what clears the way for `stage`, leaving the cursor on its first
    /// line. `resized` says the terminal has changed size since the last stage.
    fn show(&mut self, stage: &Stage, full: bool, resized: bool, out: &mut Vec<u8>) {
        if full {
            if !self.full {
                out.extend_from_slice(b"\x1b[?1049h");
            }
            self.moved |= resized;
            out.extend_from_slice(b"\x1b[2J\x1b[H");
        } else {
            if self.full {
                out.extend_from_slice(b"\x1b[?1049l");
            }
            if !self.room {
                // Make room below the prompt and come back to its first line.
                let below = stage.lines.saturating_sub(1);
                out.extend(std::iter::repeat_n(b'\n', usize::from(below)));
                if below > 0 {
                    let _ = write!(out, "\x1b[{below}A");
                }
                out.push(b'\r');
                self.room = true;
            } else if resized || self.moved {
                // The lines made room for have moved: clear the screen and
                // draw in its last lines.
                let first = stage.size.rows.saturating_sub(stage.lines) + 1;
                let _ = write!(out, "\x1b[2J\x1b[{first};1H");
            }
            self.moved = false;
            out.extend_from_slice(b"\x1b[J");
        }
        self.full = full;
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let mut out = b"\x1b[0m".to_vec();
        if self.full {
            out.extend_from_slice(b"\x1b[?1049l");
        }
        out.extend_from_slice(b"\x1b[J\x1b[?25h");
        tty::write_all(&out);
    }
}

pub fn run(mut options: Options) -> i32 {
    let Some(mut size) = tty::size() else {
        eprintln!("pulsr: standard output is not a terminal");
        return 2;
    };

    let live = LiveSpectrum::new();
    let reader = live.reader();
    let mut analyser = Analyser::new(live.clone());
    let handler: PcmHandler = Box::new(move |left, right, sample_rate, start| {
        analyser.process(left, right, sample_rate, start);
    });
    let _capture = if options.test_tone {
        audio::start_test_tone(handler)
    } else {
        match audio::start_system(handler) {
            Ok(capture) => {
                live.set_output_delay(audio::output_info().map_or(0.0, |output| output.latency));
                capture
            }
            Err(error) => {
                eprintln!("pulsr: no capture: {error}");
                return 1;
            }
        }
    };

    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
        libc::signal(libc::SIGQUIT, handler);
    }
    let mut screen = Screen::enter();
    // Dim lines fade toward what is behind them.
    let (terminal_text, terminal_background) = terminal_colours();
    let terminal_backdrop = terminal_background.unwrap_or([0; 3]);
    // The terminal theme draws in the terminal's own text colour.
    let own = Theme::terminal_reporting(terminal_text, terminal_background);
    if options.theme.name == own.name {
        options.theme = own.clone();
    }
    options.themes.add(own);

    // What the terminal is taken to draw, going by its name.
    let terminal =
        Mode::for_terminal(std::env::var("TERM_PROGRAM").ok().as_deref(), std::env::var("TERM").ok().as_deref());
    let mut mode = options.mode.unwrap_or(terminal);
    let mut full = options.full;
    let mut theme = options.theme.clone();
    let mut gradient = options.gradient.as_deref().and_then(|name| theme.gradient_index(name)).unwrap_or(0);
    let mut state = options.state.clone();
    let mut state_changed = false;
    let mut status_until = f64::NEG_INFINITY;
    // What the status line says in place of the look: a figure a key changed.
    let mut figure: Option<String> = None;
    // Every octant only where the terminal is known to draw them all.
    let octant = match terminal {
        Mode::Octant => cells::Style::octants(),
        _ => cells::Style::cautious_octants(),
    };
    let braille = cells::Style::braille();
    let mut stage = Stage::new(&options, mode, size, full, clock::now().to_bits());
    let mut builder = GeometryBuilder::default();
    let mut list = DrawList::default();
    let mut timings = Timings::default();
    let mut report = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    screen.show(&stage, full, false, &mut out);
    tty::write_all(&out);

    let mut silence = SilenceWatch::default();
    // Nothing to say about the test tone, or where silence is only silence.
    let silence_notice = audio::SILENCE_NOTICE.filter(|_| !options.test_tone);
    let mut silence_checked = f64::NEG_INFINITY;

    let mut period = 1.0 / options.fps;
    let started = clock::now();
    let mut mode_started = started;
    let mut next = started;
    while !STOP.load(Ordering::Relaxed) {
        let now = clock::now();
        if options.seconds.is_some_and(|seconds| now - started >= seconds) {
            break;
        }
        let mut restage = false;
        for key in tty::presses(&tty::keys()) {
            match key.to_ascii_lowercase() {
                b'q' => STOP.store(true, Ordering::Relaxed),
                b'm' => {
                    let at = Mode::ALL.iter().position(|other| *other == mode).unwrap_or(0);
                    report.extend(timings.line(mode, &stage, now - mode_started));
                    let next = Mode::ALL[(at + 1) % Mode::ALL.len()];
                    // The ASCII modes have their own dots between rows.
                    restage |= next.look().is_some() != mode.look().is_some();
                    mode = next;
                    state.insert("mode".to_string(), mode.name().to_string());
                    state_changed = true;
                    timings = Timings::default();
                    mode_started = now;
                }
                b'f' => {
                    full = !full;
                    state.insert("full".to_string(), full.to_string());
                    state_changed = true;
                    restage = true;
                    continue;
                }
                b'r' => {
                    options.rising = !options.rising;
                    state.insert("rising".to_string(), options.rising.to_string());
                    state_changed = true;
                    stage.ridge.params.ridge_rising = options.rising;
                    figure = Some(format!("front row {}", if options.rising { "rising" } else { "standing" }));
                    status_until = now + STATUS_SECONDS;
                    continue;
                }
                b's' => {
                    options.settle = !options.settle;
                    state.insert("settle".to_string(), options.settle.to_string());
                    state_changed = true;
                    stage.ridge.keep_moving = !options.settle;
                    let rows = if options.settle { "settle" } else { "scroll on" };
                    figure = Some(format!("in silence the rows {rows}"));
                    status_until = now + STATUS_SECONDS;
                    continue;
                }
                b't' => {
                    theme = options.themes.next(&theme);
                    gradient = 0;
                }
                b'g' => gradient = (gradient + 1) % theme.gradients.len(),
                b'[' | b']' => {
                    let step = if key == b']' { 1.0 } else { -1.0 };
                    let (setting, pitch) = options.pitch(mode);
                    let pitch = (pitch.round() + step).clamp(KEY_PITCH.0, KEY_PITCH.1);
                    if mode.look().is_some() {
                        options.ascii_pitch = pitch;
                    } else {
                        options.pitch = pitch;
                    }
                    state.insert(setting.to_string(), pitch.to_string());
                    state_changed = true;
                    restage = true;
                    figure = Some(format!("{pitch} dots between rows"));
                    status_until = now + STATUS_SECONDS;
                    continue;
                }
                b',' | b'<' | b'.' | b'>' => {
                    options.height = stepped(options.height, HEIGHT_RANGE, matches!(key, b'.' | b'>'));
                    state.insert("height".to_string(), options.height.to_string());
                    state_changed = true;
                    restage = true;
                    figure = Some(format!("stack {}% of the height", options.height));
                    status_until = now + STATUS_SECONDS;
                    continue;
                }
                b';' | b':' | b'\'' | b'"' | b'9' | b'(' | b'0' | b')' => {
                    let up = matches!(key, b'\'' | b'"' | b'0' | b')');
                    if matches!(key, b'9' | b'(' | b'0' | b')') {
                        options.curve = stepped(options.curve, CURVE_RANGE, up);
                        state.insert("curve".to_string(), options.curve.to_string());
                        figure = Some(format!("fade curve {}", options.curve));
                    } else {
                        options.fade = stepped(options.fade, FADE_RANGE, up);
                        state.insert("fade".to_string(), options.fade.to_string());
                        figure = Some(format!("fade {}%", options.fade));
                    }
                    options.set_fade(&mut stage.ridge.params);
                    state_changed = true;
                    status_until = now + STATUS_SECONDS;
                    continue;
                }
                b'-' | b'_' | b'+' | b'=' => {
                    options.fps = stepped_fps(options.fps, matches!(key, b'+' | b'='));
                    period = 1.0 / options.fps;
                    report.extend(timings.line(mode, &stage, now - mode_started));
                    timings = Timings::default();
                    mode_started = now;
                    state.insert("fps".to_string(), options.fps.to_string());
                    state_changed = true;
                    figure = Some(format!("{} frames a second", options.fps));
                    status_until = now + STATUS_SECONDS;
                    continue;
                }
                _ => continue,
            }
            figure = None;
            if matches!(key.to_ascii_lowercase(), b't' | b'g') {
                // The theme and its gradient are saved together: one means nothing
                // without the other.
                state.insert("theme".to_string(), theme.name.clone());
                state.insert("gradient".to_string(), theme.gradients[gradient].name.clone());
                state_changed = true;
            }
            status_until = now + STATUS_SECONDS;
        }
        let resized = tty::size().filter(|new| *new != size);
        if restage || resized.is_some() {
            report.extend(timings.line(mode, &stage, now - mode_started));
            if let Some(new) = resized {
                size = new;
            }
            stage = Stage::new(&options, mode, size, full, now.to_bits());
            out.clear();
            screen.show(&stage, full, resized.is_some(), &mut out);
            tty::write_all(&out);
            timings = Timings::default();
            mode_started = now;
        }

        if silence_notice.is_some() && now - silence_checked >= SILENCE_POLL {
            silence_checked = now;
            silence.update(&live.stats(), now);
        }

        // A frame is written as its time comes and stays up for a period.
        stage.ridge.lead_sec = sync_lead(&stage.ridge.params, period, period, 1.0 / f64::from(bands::FRAME_RATE));
        stage.ridge.advance(now, Some(&reader), stage.target.height());
        let frame = stage.ridge.frame(now);
        builder.build(frame.as_ref(), stage.target, &mut list);
        let built = clock::now();

        match mode.look() {
            Some(look) => stage.grid.draw(&list, look, now),
            None => stage.canvas.draw(&list, DOT_HALF_WIDTH),
        }
        let drawn = clock::now();

        // Synchronised output: the terminal holds its repaint between the two marks.
        out.clear();
        out.extend_from_slice(b"\x1b[?2026h");
        let paint = cells::Paint {
            gradient: &theme.gradients[gradient],
            top_level: GRADIENT_TOP,
            full: stage.ridge.params.ridge_alpha as f32,
            backdrop: theme.background.unwrap_or(terminal_backdrop),
            background: theme.background,
        };
        match mode {
            Mode::Octant => cells::encode(&stage.canvas, &octant, &paint, &mut out),
            Mode::Braille => cells::encode(&stage.canvas, &braille, &paint, &mut out),
            Mode::Static | Mode::Line => stage.grid.encode(&paint, &mut out),
        }
        // A notice lies over the top line, which the next frame redraws.
        if now < status_until {
            let status = figure
                .clone()
                .unwrap_or_else(|| format!("{} · {} · {}", theme.name, theme.gradients[gradient].name, mode.name()));
            write_notice(&status, stage.size.cols, &theme, &mut out);
        } else if let Some(notice) = silence_notice.filter(|_| silence.silent) {
            write_notice(notice, stage.size.cols, &theme, &mut out);
        }
        out.extend_from_slice(b"\x1b[?2026l");
        let encoded = clock::now();
        tty::write_all(&out);
        let written = clock::now();

        timings.frames += 1;
        timings.ridge += built - now;
        timings.raster += drawn - built;
        timings.encode += encoded - drawn;
        timings.write += written - encoded;
        timings.bytes += out.len() as u64;

        next += period;
        if next > written {
            std::thread::sleep(std::time::Duration::from_secs_f64(next - written));
        } else {
            timings.late += 1;
            next = written;
        }
    }
    report.extend(timings.line(mode, &stage, clock::now() - mode_started));
    drop(screen);

    if state_changed
        && let Some(dir) = &options.config_dir
        && let Err(error) = config::write_state(dir, &state)
    {
        eprintln!("pulsr: what the keys chose was not saved: {error}");
    }
    for warning in &options.warnings {
        eprintln!("pulsr: {warning}");
    }

    // Said again where it stays on the screen.
    if let Some(notice) = silence_notice.filter(|_| silence.update(&live.stats(), clock::now())) {
        eprintln!("pulsr: {notice}");
    }

    if options.stats {
        let heard = live.stats();
        eprintln!("terminal {}x{} cells", size.cols, size.rows);
        eprintln!(
            "capture: {} blocks at {} Hz, peak {:.4}, {}",
            heard.blocks,
            heard.sample_rate,
            heard.peak,
            if heard.heard_signal { "signal heard" } else { "silence" },
        );
        for line in report {
            eprintln!("{line}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ridge::FrameSource;

    #[test]
    fn the_silence_notice_waits_for_blocks_of_nothing_but_zeros() {
        let mut watch = SilenceWatch::default();
        let mut stats = CaptureStats::default();
        // Nothing playing: the tap delivers no blocks, however long.
        assert!(!watch.update(&stats, 0.0));
        assert!(!watch.update(&stats, 60.0));
        // Blocks of zeros: due once they have gone on long enough.
        stats.blocks = 10;
        assert!(!watch.update(&stats, 61.0));
        assert!(!watch.update(&stats, 61.0 + SILENCE_SECONDS - 0.1));
        assert!(watch.update(&stats, 61.0 + SILENCE_SECONDS));
        // The first sample that is not zero takes it down for good.
        stats.heard_signal = true;
        assert!(!watch.update(&stats, 70.0));
        assert!(!watch.update(&stats, 80.0));
    }

    #[test]
    fn the_notice_is_cut_to_the_terminal_and_leaves_the_cursor_at_the_margin() {
        let mut out = Vec::new();
        write_notice("only silence so far", 12, &Theme::terminal(), &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b[0;2;38;2;255;255;255m only silenc\x1b[0m\r");
        // On a theme's background the line carries it.
        let mut out = Vec::new();
        write_notice("hi", 12, &Theme::named("nord").unwrap(), &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b[0;2;38;2;216;222;233m\x1b[48;2;46;52;64m hi\x1b[0m\r");
    }

    fn parse(args: &[&str]) -> Result<Options, String> {
        over(Config::default(), args)
    }

    fn over(config: Config, args: &[&str]) -> Result<Options, String> {
        Options::over(config, args.iter().map(|arg| arg.to_string()))
    }

    fn settings(lines: &[(&str, &str)]) -> Config {
        let settings = lines
            .iter()
            .enumerate()
            .map(|(index, (key, value))| config::Setting {
                line: index + 1,
                key: key.to_string(),
                value: value.to_string(),
            })
            .collect();
        Config { settings, ..Config::default() }
    }

    #[test]
    fn the_settings_file_is_where_the_options_start_from() {
        let file = || {
            settings(&[
                ("theme", "nord"),
                ("gradient", "aurora"),
                ("mode", "braille"),
                ("full", "yes"),
                ("fps", "30"),
                ("pitch", "7"),
                ("lines", "12"),
            ])
        };
        let options = over(file(), &[]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient.as_deref()), ("nord", Some("aurora")));
        assert_eq!(
            (options.mode, options.full, options.fps, options.pitch, options.lines),
            (Some(Mode::Braille), true, 30.0, 7.0, Some(12))
        );
        assert!(options.warnings.is_empty());

        // An argument wins over the file.
        let options = over(file(), &["-fps", "120", "-gradient", "frost"]).unwrap();
        assert_eq!(
            (options.fps, options.theme.name.as_str(), options.gradient.as_deref()),
            (120.0, "nord", Some("frost"))
        );
        // The file's gradient goes with the file's theme, not another.
        let options = over(file(), &["-theme", "dracula"]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient), ("dracula", None));
        let options = over(file(), &["-gradient", "cyan-pink", "-theme", "dracula"]).unwrap();
        assert_eq!(options.gradient.as_deref(), Some("cyan-pink"));
    }

    #[test]
    fn what_the_keys_last_chose_is_where_the_next_run_starts() {
        let saved = |pairs: &[(&str, &str)]| -> config::State {
            pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
        };
        let state = saved(&[("theme", "dracula"), ("gradient", "cyan-pink"), ("mode", "braille"), ("full", "true")]);
        let figures = saved(&[("fps", "30"), ("pitch", "8")]);
        let options = over(Config { state: figures, state_is_newer: true, ..Config::default() }, &[]).unwrap();
        assert_eq!((options.fps, options.pitch), (30.0, 8.0));
        let options = over(Config { state: state.clone(), state_is_newer: true, ..Config::default() }, &[]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient.as_deref()), ("dracula", Some("cyan-pink")));
        assert_eq!((options.mode, options.full), (Some(Mode::Braille), true));

        // Written after the settings file, it wins over it; the file still
        // gives what the keys do not change.
        let file = || settings(&[("theme", "nord"), ("gradient", "aurora"), ("fps", "30")]);
        let options = over(Config { state: state.clone(), state_is_newer: true, ..file() }, &[]).unwrap();
        assert_eq!(
            (options.theme.name.as_str(), options.gradient.as_deref(), options.fps),
            ("dracula", Some("cyan-pink"), 30.0)
        );
        // A settings file edited since wins, where it says anything.
        let options = over(Config { state: state.clone(), state_is_newer: false, ..file() }, &[]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient.as_deref()), ("nord", Some("aurora")));
        assert_eq!((options.mode, options.full), (Some(Mode::Braille), true));
        let options =
            over(Config { state: state.clone(), state_is_newer: false, ..settings(&[("theme", "nord")]) }, &[])
                .unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient), ("nord", None));
        // An argument wins over both.
        let options = over(Config { state, state_is_newer: true, ..file() }, &["-theme", "gotham"]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient), ("gotham", None));

        // A saved theme that is gone, or a gradient it no longer has, is
        // passed over without a word.
        let stale = saved(&[("theme", "deleted"), ("gradient", "cyan-pink"), ("mode", "gone"), ("full", "true")]);
        let options = over(Config { state: stale, state_is_newer: true, ..Config::default() }, &[]).unwrap();
        assert_eq!(
            (options.theme.name.as_str(), options.gradient, options.mode, options.full),
            ("terminal", None, None, true)
        );
        let stale = saved(&[("theme", "nord"), ("gradient", "gone")]);
        let options = over(Config { state: stale, state_is_newer: true, ..Config::default() }, &[]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient), ("nord", None));
        assert!(options.warnings.is_empty());
    }

    #[test]
    fn a_key_moves_a_figure_to_the_next_step_within_its_range() {
        // Between steps: up to 85, down to 80, then by fives.
        assert_eq!((stepped(82.0, HEIGHT_RANGE, true), stepped(82.0, HEIGHT_RANGE, false)), (85.0, 80.0));
        assert_eq!((stepped(85.0, HEIGHT_RANGE, true), stepped(85.0, HEIGHT_RANGE, false)), (90.0, 80.0));
        assert_eq!((stepped(100.0, HEIGHT_RANGE, true), stepped(20.0, HEIGHT_RANGE, false)), (100.0, 20.0));
        assert_eq!((stepped(100.0, FADE_RANGE, false), stepped(0.0, FADE_RANGE, false)), (90.0, 0.0));
        assert_eq!((stepped(0.75, CURVE_RANGE, true), stepped(0.75, CURVE_RANGE, false)), (1.0, 0.5));
    }

    #[test]
    fn the_height_and_the_fade_reach_the_ridge() {
        let options = parse(&["-height", "100", "-fade", "40", "-curve", "2"]).unwrap();
        let stage = Stage::new(&options, Mode::Octant, tty::Size { cols: 80, rows: 30 }, true, 1);
        let params = &stage.ridge.params;
        assert_eq!((params.ridge_height, params.ridge_fade_curve), (1.0, 2.0));
        assert!((params.ridge_back_alpha - params.ridge_alpha * 0.6).abs() < 1e-9);

        let options = parse(&[]).unwrap();
        assert_eq!((options.height, options.fade, options.curve), (DEFAULT_HEIGHT, DEFAULT_FADE, DEFAULT_CURVE));
        let stage = Stage::new(&options, Mode::Octant, tty::Size { cols: 80, rows: 30 }, true, 1);
        let params = &stage.ridge.params;
        assert_eq!(params.ridge_height, 0.9);
        assert!((params.ridge_back_alpha - params.ridge_alpha * 0.2).abs() < 1e-9);
        assert!(parse(&["-fade", "0"]).is_ok());
        assert!(parse(&["-fade", "101"]).is_err());
        assert_eq!(parse(&["-height", "500"]).unwrap().height, 100.0);
    }

    #[test]
    fn the_frame_rate_keys_step_through_the_usual_rates() {
        assert_eq!((stepped_fps(60.0, true), stepped_fps(60.0, false)), (90.0, 45.0));
        // From a rate between two steps, to the nearest on that side.
        assert_eq!((stepped_fps(50.0, true), stepped_fps(50.0, false)), (60.0, 45.0));
        // The ends stay put.
        assert_eq!(
            (stepped_fps(144.0, true), stepped_fps(10.0, false), stepped_fps(500.0, true)),
            (144.0, 10.0, 500.0)
        );
        assert_eq!(stepped_fps(500.0, false), 144.0);
    }

    #[test]
    fn a_setting_that_cannot_be_used_is_a_warning_and_the_rest_stand() {
        let config = settings(&[
            ("fps", "fast"),
            ("theme", "nope"),
            ("seconds", "3"),
            ("colour", "red"),
            ("full", "maybe"),
            ("gradient", "aurora"),
            ("pitch", "8"),
        ]);
        let options = over(config, &[]).unwrap();
        assert_eq!((options.fps, options.pitch, options.full), (DEFAULT_FPS, 8.0, false));
        assert_eq!((options.theme.name.as_str(), options.gradient, options.seconds), ("terminal", None, None));
        assert_eq!(options.warnings.len(), 6, "{:#?}", options.warnings);
        assert_eq!(options.warnings[0], "config line 1: fps needs a positive number");
        assert!(options.warnings[1].starts_with("config line 2: theme finds no theme nope; there are: terminal,"));
        assert!(options.warnings[2].starts_with("config line 3: seconds is not a setting; the settings are: theme,"));
        assert_eq!(options.warnings[4], "config line 5: full needs true or false");
        assert!(options.warnings[5].starts_with("config: theme terminal has no gradient aurora"));
    }

    #[test]
    fn themes_from_the_configuration_can_be_chosen_and_are_warned_about() {
        let config = || Config {
            themes: vec![Theme::parse("godzilla", "gradient atomic = #1f6f4a #39d98a").unwrap()],
            warnings: vec!["theme broken.theme left out: line 1: nonsense".to_string()],
            ..settings(&[("theme", "godzilla"), ("gradient", "atomic")])
        };
        let options = over(config(), &[]).unwrap();
        assert_eq!((options.theme.name.as_str(), options.gradient.as_deref()), ("godzilla", Some("atomic")));
        assert_eq!(options.warnings, ["theme broken.theme left out: line 1: nonsense"]);
        assert_eq!(options.themes.next(&options.theme).name, "terminal");
        assert!(over(Config::default(), &["-theme", "godzilla"]).is_err());
        assert_eq!(
            over(config(), &["-theme", "nope"]).unwrap_err().split(';').next(),
            Some("-theme finds no theme nope")
        );
    }

    #[test]
    fn options_default_to_sixty_frames_and_the_terminals_own_mode() {
        let options = parse(&[]).unwrap();
        assert_eq!((options.mode, options.fps, options.pitch), (None, DEFAULT_FPS, DEFAULT_PITCH));
        assert!(!options.test_tone && !options.stats && options.lines.is_none());

        let options = parse(&["-mode", "braille", "-testtone", "-fps", "120", "-lines", "20"]).unwrap();
        assert_eq!((options.mode, options.fps, options.lines), (Some(Mode::Braille), 120.0, Some(20)));
        assert!(options.test_tone);

        assert!(parse(&["-mode", "no-such-mode"]).is_err());
        assert!(parse(&["-fps", "0"]).is_err());
        assert!(parse(&["-no-such-option"]).is_err());

        let options = parse(&[]).unwrap();
        assert!(!options.full && options.gradient.is_none());
        assert_eq!(options.theme, Theme::terminal());
        let options = parse(&["-full", "-gradient", "aurora", "-theme", "nord"]).unwrap();
        assert!(options.full);
        assert_eq!((options.theme.name.as_str(), options.gradient.as_deref()), ("nord", Some("aurora")));
        assert!(parse(&["-theme", "nord", "-gradient", "blue-pink"]).unwrap_err().contains("it has: frost, aurora"));
        assert!(parse(&["-gradient", "text"]).is_ok());
        assert!(parse(&["-theme", "no-such-theme"]).is_err());
        assert!(parse(&["-theme"]).is_err());
        assert_eq!(parse(&["-help"]).unwrap_err(), "");
    }

    #[test]
    fn the_usage_text_names_every_theme() {
        for name in Theme::names() {
            assert!(USAGE.contains(name), "{name}");
        }
    }

    #[test]
    fn the_rows_scroll_through_silence_unless_asked_to_settle() {
        let size = tty::Size { cols: 80, rows: 24 };
        let stage = |options: &Options| Stage::new(options, Mode::Octant, size, false, 1);
        assert!(stage(&parse(&[]).unwrap()).ridge.keep_moving);
        assert!(!stage(&parse(&["-settle"]).unwrap()).ridge.keep_moving);
        assert!(!stage(&over(settings(&[("settle", "true")]), &[]).unwrap()).ridge.keep_moving);
    }

    #[test]
    fn the_ascii_modes_keep_their_own_spacing_of_the_rows() {
        let size = tty::Size { cols: 80, rows: 41 };
        let rows = |options: &Options, mode: Mode| Stage::new(options, mode, size, true, 1).ridge.row_count();
        // 164 dots, 90% of them the stack: a row every 5 dots, or every 10.
        let options = parse(&[]).unwrap();
        assert_eq!((options.pitch, options.ascii_pitch), (DEFAULT_PITCH, DEFAULT_ASCII_PITCH));
        assert_eq!((rows(&options, Mode::Octant), rows(&options, Mode::Braille)), (31, 31));
        assert_eq!((rows(&options, Mode::Static), rows(&options, Mode::Line)), (16, 16));
        // Each is set and saved under its own name.
        let saved = || {
            let state = [("pitch", "4"), ("ascii-pitch", "12")];
            let state = state.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
            Config { state, state_is_newer: true, ..Config::default() }
        };
        assert_eq!(over(saved(), &[]).unwrap().pitch(Mode::Static), ("ascii-pitch", 12.0));
        let options = over(saved(), &["-ascii-pitch", "8"]).unwrap();
        assert_eq!(options.pitch(Mode::Braille), ("pitch", 4.0));
        assert_eq!(options.pitch(Mode::Line), ("ascii-pitch", 8.0));
    }

    #[test]
    fn every_mode_is_reached_by_its_name() {
        for mode in Mode::ALL {
            assert_eq!(Mode::named(mode.name()), Some(mode));
        }
    }

    #[test]
    fn octants_are_the_default_only_where_the_terminal_draws_them() {
        assert_eq!(Mode::for_terminal(Some("ghostty"), Some("xterm-ghostty")), Mode::Octant);
        assert_eq!(Mode::for_terminal(None, Some("xterm-kitty")), Mode::Octant);
        assert_eq!(Mode::for_terminal(Some("Apple_Terminal"), Some("xterm-256color")), Mode::Braille);
        assert_eq!(Mode::for_terminal(None, None), Mode::Braille);
    }

    #[test]
    fn a_dot_stack_has_as_many_rows_as_fit_at_the_pitch() {
        let params = RidgeParams::default();
        // 57 lines of 4 dots: a stack of 0.621 * 228 = 142 dots.
        assert_eq!(dot_rows(228, 5.0, &params), 29);
        assert_eq!(dot_rows(228, 1.0, &params), params.ridge_rows);
        assert_eq!(dot_rows(8, 5.0, &params), 4);
    }

    struct Tone;

    impl FrameSource for Tone {
        fn width(&self) -> usize {
            128
        }

        fn read(&self, _: f64, _: f64, out: &mut [f32]) -> bool {
            for (i, level) in out.iter_mut().enumerate() {
                *level = if i % 64 == 20 { 1.0 } else { 0.0 };
            }
            true
        }
    }

    /// The canvas after two seconds of a steady tone in a 120 by 40 cell terminal.
    fn canvas_after_a_tone() -> (Stage, Canvas) {
        let size = tty::Size { cols: 120, rows: 40 };
        let options = parse(&[]).unwrap();
        let mut stage = Stage::new(&options, Mode::Octant, size, false, 1);
        let mut now = 5.0;
        for _ in 0..120 {
            now += 1.0 / 60.0;
            stage.ridge.advance(now, Some(&Tone), stage.target.height());
        }
        let mut list = DrawList::default();
        GeometryBuilder::default().build(stage.ridge.frame(now).as_ref(), stage.target, &mut list);
        let mut canvas = Canvas::new(stage.target.pixel_width as usize, stage.target.pixel_height as usize);
        canvas.draw(&list, DOT_HALF_WIDTH);
        (stage, canvas)
    }

    #[test]
    fn the_stage_is_two_by_four_dots_a_cell() {
        let (stage, canvas) = canvas_after_a_tone();
        assert_eq!((stage.lines, canvas.width, canvas.height), (39, 240, 156));
        assert_eq!(stage.ridge.row_count(), dot_rows(156, DEFAULT_PITCH, &stage.ridge.params));
        // The back row stands in the top fifth, and all but the loudest
        // tenth or so of what it can show fits under the top line.
        let params = &stage.ridge.params;
        let back = 1.0 - params.ridge_bottom - params.ridge_height;
        let rise = params.ridge_peak * params.ridge_depth_scale;
        assert!(back < 0.2 && rise <= back + 0.08, "{back} {rise}");
        // The tone stands above the flat rows: the lit dots reach higher
        // in its column than at the edge.
        let top = |x: usize| (0..canvas.height).find(|y| canvas.pixels[y * canvas.width + x] > 0.0);
        let peak = (0..canvas.width).filter_map(top).min().unwrap();
        assert!(peak + 5 < top(2).unwrap(), "{peak} against {:?}", top(2));
        // Every dot is off or carries a row's alpha.
        assert!(canvas.pixels.iter().all(|value| (0.0..=1.0).contains(value)));

        let mut out = Vec::new();
        let theme = Theme::terminal();
        let paint = cells::Paint {
            gradient: &theme.gradients[0],
            top_level: GRADIENT_TOP,
            full: 0.85,
            backdrop: [0; 3],
            background: None,
        };
        cells::encode(&canvas, &cells::Style::octants(), &paint, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("\r\n").count(), 38);
        assert!(text.ends_with("\x1b[38A\r"));
    }

    #[test]
    fn a_full_screen_stage_takes_every_line() {
        let size = tty::Size { cols: 120, rows: 40 };
        let options = parse(&["-lines", "10"]).unwrap();
        assert_eq!(Stage::new(&options, Mode::Octant, size, false, 1).lines, 10);
        assert_eq!(Stage::new(&options, Mode::Octant, size, true, 1).lines, 40);
    }
}
