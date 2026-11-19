//! System audio on Linux: a record stream on the default sink's monitor, over the
//! PulseAudio native protocol (also served by PipeWire's `pipewire-pulse`).
//! Follows the default sink when it changes and reconnects once a second if lost.

use std::{
    ffi::{CStr, CString},
    io::{BufReader, Cursor, Read},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use pulseaudio::protocol::{
    self, AuthParams, AuthReply, ChannelMap, Command, CreateRecordStreamReply, Prop, Props, ProtocolError,
    RecordStreamParams, SampleFormat, SampleSpec, ServerInfo, SetClientNameReply, SubscriptionEventFacility,
    SubscriptionMask,
};

use super::PcmHandler;
use crate::clock;

/// Seconds of audio per block asked of the server.
const BLOCK_SECONDS: f64 = 0.005;

/// The rate asked for when the server's own is not usable.
const FALLBACK_RATE: u32 = 48_000;

/// Bytes in a frame: two channels of 32-bit float.
const FRAME_BYTES: usize = 8;

/// The protocol's own limit on a frame.
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// How long to wait before making a lost connection again.
const RETRY: Duration = Duration::from_secs(1);
/// How long the server gets to answer while a connection is set up.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// A running capture of the default sink's monitor. Dropping it stops the capture.
pub struct Monitor {
    stop: Arc<AtomicBool>,
    /// Shut down from another thread to wake the capture thread from its read.
    socket: Arc<Mutex<Option<UnixStream>>>,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    pub fn start(mut handler: PcmHandler) -> Result<Self, String> {
        let mut session = Session::open().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(session.socket()));

        let (stopped, shared) = (stop.clone(), socket.clone());
        let thread = thread::spawn(move || {
            loop {
                // A moved default sink is followed at once; any other failure waits `RETRY`.
                let mut wait = !matches!(session.run(&mut handler), Failure::Moved);
                session = loop {
                    if wait {
                        pause(&stopped);
                    }
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    match Session::open() {
                        Ok(session) => break session,
                        Err(_) => wait = true,
                    }
                };
                *shared.lock().unwrap() = session.socket();
                // A stop that came while connecting shut down the old
                // socket, not this one.
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
            }
        });
        Ok(Self { stop, socket, thread: Some(thread) })
    }
}

/// Waits out `RETRY`, or less if the capture is stopped meanwhile.
fn pause(stopped: &AtomicBool) {
    const STEP: Duration = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < RETRY && !stopped.load(Ordering::Relaxed) {
        thread::sleep(STEP);
        waited += STEP;
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug)]
enum Failure {
    NoServer,
    Protocol(ProtocolError),
    /// The default sink is no longer the one being captured.
    Moved,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoServer => write!(f, "no PulseAudio or PipeWire sound server was found"),
            Self::Protocol(error) => write!(f, "the sound server: {error}"),
            Self::Moved => write!(f, "the default output changed"),
        }
    }
}

impl<E: Into<ProtocolError>> From<E> for Failure {
    fn from(error: E) -> Self {
        Self::Protocol(error.into())
    }
}

/// An authenticated connection.
struct Connection {
    reader: BufReader<UnixStream>,
    version: u16,
    sequence: u32,
}

impl Connection {
    fn open() -> Result<Self, Failure> {
        let stream = socket_paths().iter().find_map(|path| UnixStream::connect(path).ok()).ok_or(Failure::NoServer)?;
        // A server that accepts and then says nothing must not hold the thread.
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
        let mut connection = Self { reader: BufReader::new(stream), version: protocol::MAX_VERSION, sequence: 0 };

        let cookie = pulseaudio::cookie_path_from_env().and_then(|path| std::fs::read(path).ok()).unwrap_or_default();
        let auth = AuthParams { version: protocol::MAX_VERSION, supports_shm: false, supports_memfd: false, cookie };
        connection.send(&Command::Auth(auth))?;
        let (_, reply) = protocol::read_reply_message::<AuthReply>(&mut connection.reader, connection.version)?;
        connection.version = reply.version.min(protocol::MAX_VERSION);

        let mut props = Props::new();
        props.set(Prop::ApplicationName, c"pulsr");
        connection.send(&Command::SetClientName(props))?;
        protocol::read_reply_message::<SetClientNameReply>(&mut connection.reader, connection.version)?;
        Ok(connection)
    }

    /// Returns the sequence number the reply will carry.
    fn send(&mut self, command: &Command) -> Result<u32, Failure> {
        let sequence = self.sequence;
        protocol::write_command_message(self.reader.get_mut(), sequence, command, self.version)?;
        self.sequence += 1;
        Ok(sequence)
    }

    fn server_info(&mut self) -> Result<ServerInfo, Failure> {
        self.send(&Command::GetServerInfo)?;
        Ok(protocol::read_reply_message::<ServerInfo>(&mut self.reader, self.version)?.1)
    }
}

/// A record stream on the default sink's monitor. PipeWire may not answer the stream
/// request until the sink is running, so the reply is handled as it arrives in `run`.
struct Session {
    connection: Connection,
    /// Sequence number of the stream request's reply.
    request: u32,
    /// Channel index and sample rate, once the server has made the stream.
    stream: Option<(u32, f64)>,
    /// The default sink when the stream was made.
    sink: Option<CString>,
}

impl Session {
    fn open() -> Result<Self, Failure> {
        let mut connection = Connection::open()?;
        let server = connection.server_info()?;
        let sink = server.default_sink_name.clone();

        let rate = server.sample_spec.sample_rate;
        let rate = if (8_000..=384_000).contains(&rate) { rate } else { FALLBACK_RATE };
        let mut props = Props::new();
        props.set(Prop::MediaName, c"what is playing");
        let mut params = RecordStreamParams {
            sample_spec: SampleSpec { format: SampleFormat::Float32Le, channels: 2, sample_rate: rate },
            channel_map: ChannelMap::stereo(),
            source_name: Some(monitor_of(sink.as_deref())),
            props,
            ..Default::default()
        };
        params.buffer_attr.max_length = u32::MAX;
        params.buffer_attr.fragment_size = (f64::from(rate) * BLOCK_SECONDS) as u32 * FRAME_BYTES as u32;
        // Makes the source itself run in blocks this short, not only the stream's buffer.
        params.flags.adjust_latency = true;
        // Do not keep an idle device awake.
        params.flags.no_inhibit_auto_suspend = true;
        let request = connection.send(&Command::CreateRecordStream(params))?;
        // The subscribe acknowledgement is skipped in `run`.
        connection.send(&Command::Subscribe(SubscriptionMask::SERVER))?;
        // From here the server is silent for as long as nothing plays.
        connection.reader.get_ref().set_read_timeout(None)?;
        Ok(Self { connection, request, stream: None, sink })
    }

    /// Another handle on the connection's socket.
    fn socket(&self) -> Option<UnixStream> {
        self.connection.reader.get_ref().try_clone().ok()
    }

    /// Runs until the connection is lost, the stream ends or the default sink changes.
    fn run(&mut self, handler: &mut PcmHandler) -> Failure {
        let mut message = Vec::new();
        let (mut left, mut right) = (Vec::new(), Vec::new());
        let mut times = BlockTimes::default();
        loop {
            let descriptor = match protocol::read_descriptor(&mut self.connection.reader) {
                Ok(descriptor) => descriptor,
                Err(error) => return error.into(),
            };
            let length = descriptor.length as usize;
            if length > MAX_MESSAGE_BYTES {
                return ProtocolError::Invalid(format!("a message of {length} bytes")).into();
            }
            message.resize(length, 0);
            if let Err(error) = self.connection.reader.read_exact(&mut message) {
                return error.into();
            }

            // Channel -1 carries commands; any other channel is stream audio.
            if descriptor.channel == u32::MAX {
                let version = self.connection.version;
                match Command::read_tag_prefixed(&mut Cursor::new(&message[..]), version) {
                    Ok((sequence, Command::Reply)) if sequence == self.request => {
                        // `read_reply_message` expects a descriptor, so put it back in front.
                        let mut framed = Vec::with_capacity(protocol::DESCRIPTOR_SIZE + message.len());
                        if let Err(error) = protocol::write_descriptor(&mut framed, &descriptor) {
                            return error.into();
                        }
                        framed.extend_from_slice(&message);
                        let reply = protocol::read_reply_message::<CreateRecordStreamReply>;
                        match reply(&mut Cursor::new(framed), version) {
                            Ok((_, stream)) => {
                                let rate = stream.sample_spec.sample_rate;
                                if !(8_000..=384_000).contains(&rate) {
                                    return ProtocolError::Invalid(format!("a stream at {rate} Hz")).into();
                                }
                                self.stream = Some((stream.channel, f64::from(rate)));
                            }
                            Err(error) => return error.into(),
                        }
                    }
                    Ok((sequence, Command::Error(error))) if sequence == self.request => {
                        return ProtocolError::ServerError(error).into();
                    }
                    Ok((_, Command::RecordStreamKilled(_))) => return Failure::Moved,
                    Ok((_, Command::SubscribeEvent(event)))
                        if event.event_facility == SubscriptionEventFacility::Server =>
                    {
                        // Queried on a new connection, so the reply need not be picked out
                        // of this one's audio.
                        let now = Connection::open().and_then(|mut connection| connection.server_info());
                        match now {
                            Ok(server) if server.default_sink_name == self.sink => {}
                            Ok(_) => return Failure::Moved,
                            Err(error) => return error,
                        }
                    }
                    // Other replies and events, including unreadable ones, are ignored.
                    _ => {}
                }
            } else if let Some((_, sample_rate)) = self.stream.filter(|(channel, _)| *channel == descriptor.channel) {
                let (frames, _) = message.as_chunks::<FRAME_BYTES>();
                let count = frames.len();
                if count == 0 {
                    continue;
                }
                left.clear();
                right.clear();
                for frame in frames {
                    left.push(f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]));
                    right.push(f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]));
                }
                let time = times.next(clock::now(), count as f64 / sample_rate);
                handler(&left, &right, sample_rate, time);
            }
        }
    }
}

/// A sink's monitor source name; the default monitor when the server named no sink.
fn monitor_of(sink: Option<&CStr>) -> CString {
    let Some(sink) = sink else { return c"@DEFAULT_MONITOR@".into() };
    let mut name = sink.to_bytes().to_vec();
    name.extend_from_slice(b".monitor");
    CString::new(name).unwrap_or_else(|_| c"@DEFAULT_MONITOR@".into())
}

/// Candidate socket paths, likeliest first, read from the environment as libpulse does.
fn socket_paths() -> Vec<PathBuf> {
    let variable = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    // SAFETY: no arguments, no preconditions.
    let user = unsafe { libc::getuid() };
    candidates(
        std::env::var("PULSE_SERVER").ok().as_deref(),
        variable("PULSE_RUNTIME_PATH"),
        variable("XDG_RUNTIME_DIR"),
        user,
    )
}

fn candidates(
    server: Option<&str>,
    pulse_runtime: Option<PathBuf>,
    runtime: Option<PathBuf>,
    user: u32,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // `PULSE_SERVER` is a list of addresses; only local ones are usable.
    for address in server.unwrap_or_default().split_whitespace() {
        // An address may carry a `{machine-id}` prefix.
        let address = address.rsplit_once('}').map_or(address, |(_, address)| address);
        let path = address.strip_prefix("unix:").unwrap_or(address);
        if path.starts_with('/') {
            paths.push(PathBuf::from(path));
        }
    }
    paths.extend(pulse_runtime.map(|dir| dir.join("native")));
    paths.extend(runtime.map(|dir| dir.join("pulse/native")));
    paths.push(PathBuf::from(format!("/run/user/{user}/pulse/native")));
    paths
}

/// Timestamps each block's first sample. Blocks arrive in bursts, so each is taken to
/// follow the last and only leans toward its arrival, unless the two drift apart.
#[derive(Default)]
struct BlockTimes {
    /// Start of the next block if it follows the last.
    following: Option<f64>,
}

impl BlockTimes {
    /// Seconds the running time may differ from an arrival before it is reset.
    const SLACK: f64 = 0.05;
    /// Fraction of the difference corrected per block.
    const LEAN: f64 = 0.02;

    fn next(&mut self, arrival: f64, duration: f64) -> f64 {
        let arrived = arrival - duration;
        let time = match self.following {
            Some(following) if (arrived - following).abs() < Self::SLACK => {
                following + (arrived - following) * Self::LEAN
            }
            _ => arrived,
        };
        self.following = Some(time + duration);
        time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_monitor_is_named_after_the_sink() {
        assert_eq!(monitor_of(Some(c"alsa_output.analog-stereo")).as_c_str(), c"alsa_output.analog-stereo.monitor");
        assert_eq!(monitor_of(None).as_c_str(), c"@DEFAULT_MONITOR@");
    }

    #[test]
    fn the_socket_is_looked_for_where_libpulse_looks() {
        let found =
            candidates(Some("{abc}unix:/tmp/a tcp:host:4713 /tmp/b"), Some("/p".into()), Some("/run/user/7".into()), 7);
        let expected = ["/tmp/a", "/tmp/b", "/p/native", "/run/user/7/pulse/native", "/run/user/7/pulse/native"];
        assert_eq!(found, expected.map(PathBuf::from));
        assert_eq!(candidates(None, None, None, 1000), [PathBuf::from("/run/user/1000/pulse/native")]);
    }

    #[test]
    fn blocks_follow_one_another_however_unevenly_they_arrive() {
        let mut times = BlockTimes::default();
        let duration = 0.005;
        assert_eq!(times.next(1.0, duration), 0.995);
        // Three blocks that waited and came together are still 5 ms apart.
        let late = [times.next(1.015, duration), times.next(1.015, duration), times.next(1.015, duration)];
        for (index, time) in late.iter().enumerate() {
            let even = 1.0 + index as f64 * duration;
            assert!((time - even).abs() < 0.001, "block {index} at {time}, evenly {even}");
        }
        assert!(late[0] < late[1] && late[1] < late[2]);
    }

    #[test]
    fn a_gap_starts_the_block_times_again() {
        let mut times = BlockTimes::default();
        times.next(1.0, 0.005);
        assert_eq!(times.next(3.0, 0.005), 2.995);
    }
}
