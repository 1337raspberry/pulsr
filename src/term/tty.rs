//! The terminal itself: its size, unbuffered keys, and writing to it.

use std::io::Write as _;

/// A terminal's size in character cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// `None` when standard output is not a terminal.
pub fn size() -> Option<Size> {
    // SAFETY: `ws` is a valid out-pointer for TIOCGWINSZ.
    let ws = unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) != 0 {
            return None;
        }
        ws
    };
    if ws.ws_col == 0 || ws.ws_row == 0 {
        return None;
    }
    Some(Size { cols: ws.ws_col, rows: ws.ws_row })
}

pub fn write_all(bytes: &[u8]) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(bytes);
    let _ = stdout.flush();
}

/// While this lives, keys arrive unbuffered and unechoed and reads do not block.
/// Dropping it restores the terminal.
pub struct RawMode {
    saved: Option<libc::termios>,
}

impl RawMode {
    pub fn enter() -> Self {
        // SAFETY: `saved` is a valid out-pointer, and the copy passed back
        // differs from it only in its flag fields.
        unsafe {
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut saved) != 0 {
                return Self { saved: None };
            }
            let mut raw = saved;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            raw.c_cc[libc::VMIN] = 0;
            raw.c_cc[libc::VTIME] = 0;
            // Being suspended would leave the shell with the terminal as it is here.
            raw.c_cc[libc::VSUSP] = libc::_POSIX_VDISABLE;
            #[cfg(target_os = "macos")]
            {
                raw.c_cc[libc::VDSUSP] = libc::_POSIX_VDISABLE;
            }
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
            Self { saved: Some(saved) }
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            // SAFETY: `saved` is the state `enter` read.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, saved) };
        }
    }
}

/// Whatever has been typed since the last call. Never blocks.
fn keys() -> Vec<u8> {
    let mut buffer = [0u8; 256];
    let mut ready = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
    // SAFETY: `ready` is one valid pollfd, and the buffer is valid for its length.
    let n = unsafe {
        // Without the check a read blocks when standard input is not a terminal.
        if libc::poll(&mut ready, 1, 0) <= 0 {
            return Vec::new();
        }
        libc::read(libc::STDIN_FILENO, buffer.as_mut_ptr().cast(), buffer.len())
    };
    buffer[..n.max(0) as usize].to_vec()
}

/// The plain key presses in `bytes`, and where an unfinished escape sequence starts:
/// `bytes.len()` if there is none. Escape sequences (arrow keys, the mouse wheel, a
/// reply from the terminal) are not key presses.
fn scan(bytes: &[u8]) -> (Vec<u8>, usize) {
    let mut presses = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != 0x1b {
            presses.push(bytes[at]);
            at += 1;
            continue;
        }
        let body = at + 2;
        let end = match bytes.get(at + 1) {
            None => None,
            // CSI: parameters, then one final byte.
            Some(b'[') => bytes[body..].iter().position(|byte| (0x40..=0x7e).contains(byte)).map(|i| body + i + 1),
            // OSC, DCS and the like: a string ended by BEL or ESC \.
            Some(b']' | b'P' | b'X' | b'^' | b'_') => {
                let rest = &bytes[body..];
                (0..rest.len())
                    .find(|&i| rest[i] == 0x07 || (i > 0 && rest[i - 1] == 0x1b && rest[i] == b'\\'))
                    .map(|i| body + i + 1)
            }
            // SS3: one more byte.
            Some(b'O') => (body < bytes.len()).then_some(body + 1),
            // The two-byte sequences.
            Some(_) => Some(body),
        };
        match end {
            Some(end) => at = end,
            None => return (presses, at),
        }
    }
    (presses, bytes.len())
}

/// The bytes after ESC that start a sequence of more than two bytes.
const INTRODUCERS: &[u8] = b"[]PX^_O";
/// The longest unfinished escape sequence held back; a longer one is let go.
const MAX_HELD: usize = 1024;
/// Seconds an unfinished escape sequence is held back for its end.
const HELD_SECONDS: f64 = 0.1;

/// What is typed, in whole escape sequences: one cut short by a read is held back
/// for the next, so that the rest of it is not taken for key presses.
#[derive(Default)]
pub struct Input {
    held: Vec<u8>,
    held_since: f64,
}

impl Input {
    /// What has been typed since the last call, less a sequence not yet whole, and the
    /// key presses in it. Never blocks.
    pub fn read(&mut self, now: f64) -> (Vec<u8>, Vec<u8>) {
        self.take(keys(), now)
    }

    fn take(&mut self, new: Vec<u8>, now: f64) -> (Vec<u8>, Vec<u8>) {
        let held = std::mem::take(&mut self.held);
        // Escape pressed before another key is not the start of a sequence.
        let escape = held == [0x1b] && new.first().is_some_and(|byte| !INTRODUCERS.contains(byte));
        let given_up = escape || now - self.held_since >= HELD_SECONDS;
        let goes_on = !given_up && !held.is_empty();
        let (mut bytes, mut rest) = if given_up { (held, new) } else { (Vec::new(), [held, new].concat()) };
        let mut presses = scan(&bytes).0;
        let (more, whole) = scan(&rest);
        presses.extend(more);
        if rest.len() - whole <= MAX_HELD {
            self.held = rest.split_off(whole);
            // Held since it was first cut short, however many reads add to it.
            if !(goes_on && whole == 0) {
                self.held_since = now;
            }
        }
        bytes.extend(rest);
        (bytes, presses)
    }
}

/// Reads the terminal's reply until `done` accepts it or `seconds` pass. Needs `RawMode`.
pub fn read_reply(done: impl Fn(&[u8]) -> bool, seconds: f64) -> Vec<u8> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
    let mut reply = Vec::new();
    while std::time::Instant::now() < deadline {
        reply.extend(keys());
        if done(&reply) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presses(bytes: &[u8]) -> Vec<u8> {
        scan(bytes).0
    }

    #[test]
    fn escape_sequences_are_not_key_presses() {
        assert_eq!(presses(b"q"), b"q");
        assert_eq!(presses(b"a\x1b[Ab"), b"ab");
        assert_eq!(presses(b"\x1bOAq"), b"q");
        assert_eq!(presses(b"\x1b[<64;10;5Mt"), b"t");
        assert_eq!(presses(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]11;rgb:1e1e/1e1e/2e2e\x07\x1b[?62;c["), b"[");
        assert_eq!(presses(b"g\x1b"), b"g");
        assert_eq!(presses(b"\x1b[1;5"), b"");
    }

    #[test]
    fn a_sequence_cut_short_waits_for_its_end() {
        let mut input = Input::default();
        let mut take = |new: &[u8], now: f64| input.take(new.to_vec(), now);
        let nothing = (Vec::new(), Vec::new());
        // A colour reply in two reads: its letters are not R, G and B pressed.
        assert_eq!(take(b"t\x1b]11;rg", 1.0), (b"t".to_vec(), b"t".to_vec()));
        assert_eq!(take(b"", 1.01), nothing);
        let whole = (b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\q".to_vec(), b"q".to_vec());
        assert_eq!(take(b"b:1e1e/1e1e/2e2e\x1b\\q", 1.02), whole);
        // The string's own ESC may be the last byte read.
        assert_eq!(take(b"\x1b]11;rgb:0/0/0\x1b", 2.0), nothing);
        assert_eq!(take(b"\\g", 2.01).1, b"g");
        assert_eq!(take(b"\x1b[1;", 3.0), nothing);
        assert_eq!(take(b"5A\x1bO", 3.01).1, b"");
        assert_eq!(take(b"Af", 3.02).1, b"f");
        // A reply may start on a read's last byte.
        assert_eq!(take(b"\x1b", 4.0), nothing);
        assert_eq!(take(b"]11;rgb:0/0/0\x07]", 4.01).1, b"]");
    }

    #[test]
    fn what_is_held_is_let_go_of() {
        let mut input = Input::default();
        let mut take = |new: &[u8], now: f64| input.take(new.to_vec(), now);
        let nothing = (Vec::new(), Vec::new());
        // Escape, then another key.
        assert_eq!(take(b"\x1b", 1.0), nothing);
        assert_eq!(take(b"q", 1.01), (b"\x1bq".to_vec(), b"q".to_vec()));
        // Escape alone, in time.
        assert_eq!(take(b"\x1b", 2.0), nothing);
        assert_eq!(take(b"", 2.0 + HELD_SECONDS / 2.0), nothing);
        assert_eq!(take(b"", 2.0 + HELD_SECONDS), (b"\x1b".to_vec(), Vec::new()));
        // A sequence whose end never comes, however often more of it does.
        assert_eq!(take(b"\x1b]11;rgb:1e1e/1e1e/2", 3.0), nothing);
        assert_eq!(take(b"e", 3.0 + HELD_SECONDS / 2.0), nothing);
        assert_eq!(take(b"q", 3.0 + HELD_SECONDS), (b"\x1b]11;rgb:1e1e/1e1e/2eq".to_vec(), b"q".to_vec()));
        // One too long to hold.
        let mut long = b"\x1b]".to_vec();
        long.resize(MAX_HELD + 10, b'x');
        assert_eq!(take(&long, 4.0), (long.clone(), Vec::new()));
        assert_eq!(take(b"q", 4.01).1, b"q");
    }
}
