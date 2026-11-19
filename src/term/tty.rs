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
pub fn keys() -> Vec<u8> {
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

/// The plain key presses in `bytes`: escape sequences (arrow keys, the mouse
/// wheel, a late reply from the terminal) are left out.
pub fn presses(bytes: &[u8]) -> Vec<u8> {
    let mut presses = Vec::new();
    let mut rest = bytes.iter().copied();
    while let Some(byte) = rest.next() {
        if byte != 0x1b {
            presses.push(byte);
            continue;
        }
        match rest.next() {
            // CSI: parameters, then one final byte.
            Some(b'[') => {
                rest.find(|byte| (0x40..=0x7e).contains(byte));
            }
            // OSC, DCS and the like: a string ended by BEL or ESC \.
            Some(b']' | b'P' | b'X' | b'^' | b'_') => {
                let mut escaped = false;
                for byte in rest.by_ref() {
                    if byte == 0x07 || (escaped && byte == b'\\') {
                        break;
                    }
                    escaped = byte == 0x1b;
                }
            }
            // SS3 and the two-byte sequences: one more byte.
            Some(b'O') => {
                rest.next();
            }
            _ => {}
        }
    }
    presses
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
}
