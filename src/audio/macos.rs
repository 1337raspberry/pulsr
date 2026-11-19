//! System audio on macOS through a Core Audio process tap (macOS 14.2+).
//! Until the terminal is allowed to record system audio the tap delivers silence.
//! The tap is on the mix, not on a device, so it survives a change of output device.

use std::{
    ffi::{CStr, c_void},
    ptr::{self, NonNull},
};

use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData, CATapDescription, CATapMuteBehavior,
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceIsStackedKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyLatency,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertySafetyOffset,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject, kAudioStreamPropertyLatency,
    kAudioSubTapUIDKey, kAudioTapPropertyFormat,
};
use objc2_core_audio_types::{
    AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSCopying, NSDictionary, NSNumber, NSString, NSUUID};

use super::{OutputInfo, PcmHandler};
use crate::clock;

/// Frames per IO cycle asked of the aggregate device; it may refuse.
const PREFERRED_BUFFER_FRAMES: u32 = 256;

/// Read only on the IO thread while the device runs.
struct Context {
    handler: PcmHandler,
    sample_rate: f64,
    channels: usize,
    interleaved: bool,
    // Scratch for de-interleaving.
    left: Vec<f32>,
    right: Vec<f32>,
}

/// A running capture of the system's output. Dropping it stops the capture.
pub struct SystemTap {
    tap: AudioObjectID,
    device: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    context: *mut Context,
}

impl SystemTap {
    pub fn start(handler: PcmHandler) -> Result<Self, String> {
        // SAFETY: plain Core Audio calls with valid arguments; each object
        // created is destroyed on the error paths and in `Drop`.
        unsafe {
            let description = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &NSArray::new(),
            );
            let uuid = NSUUID::new();
            description.setUUID(&uuid);
            description.setName(&NSString::from_str("pulsr"));
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);

            let mut tap: AudioObjectID = 0;
            check(
                AudioHardwareCreateProcessTap(Some(&description), &mut tap),
                "creating the process tap",
            )?;

            let format = match read_format(tap) {
                Ok(format) => format,
                Err(error) => {
                    AudioHardwareDestroyProcessTap(tap);
                    return Err(error);
                }
            };

            let mut device: AudioObjectID = 0;
            let properties = aggregate_properties(&uuid.UUIDString());
            let status = AudioHardwareCreateAggregateDevice(
                &*(Retained::as_ptr(&properties).cast::<CFDictionary>()),
                NonNull::from(&mut device),
            );
            if let Err(error) = check(status, "creating the aggregate device") {
                AudioHardwareDestroyProcessTap(tap);
                return Err(error);
            }

            // Best effort: the device keeps its own cycle length if it refuses.
            let frames_address = address(kAudioDevicePropertyBufferFrameSize, kAudioObjectPropertyScopeGlobal);
            let mut wanted = PREFERRED_BUFFER_FRAMES;
            AudioObjectSetPropertyData(
                device,
                NonNull::from(&frames_address),
                0,
                ptr::null(),
                size_of::<u32>() as u32,
                NonNull::from(&mut wanted).cast(),
            );

            let context = Box::into_raw(Box::new(Context {
                handler,
                sample_rate: format.mSampleRate,
                channels: format.mChannelsPerFrame as usize,
                interleaved: format.mFormatFlags & kAudioFormatFlagIsNonInterleaved == 0,
                left: vec![0.0; 8192],
                right: vec![0.0; 8192],
            }));
            let mut proc_id: AudioDeviceIOProcID = None;
            let created = check(
                AudioDeviceCreateIOProcID(device, Some(io_proc), context.cast(), NonNull::from(&mut proc_id)),
                "creating the IO proc",
            )
            .and_then(|()| check(AudioDeviceStart(device, proc_id), "starting the aggregate device"));
            let tap = Self { tap, device, proc_id, context };
            // Dropping `tap` on an error tears everything down.
            created.map(|()| tap)
        }
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        // SAFETY: `AudioDeviceStop` returns once the IO thread has left the proc,
        // so nothing reads the context when it is freed.
        unsafe {
            if self.proc_id.is_some() {
                AudioDeviceStop(self.device, self.proc_id);
                AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            }
            AudioHardwareDestroyAggregateDevice(self.device);
            AudioHardwareDestroyProcessTap(self.tap);
            drop(Box::from_raw(self.context));
        }
    }
}

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the context `start` registered, alive until the
    // device is stopped; the buffer list and timestamps are valid for the
    // length of the call, and the list holds `mNumberBuffers` buffers.
    unsafe {
        let context = &mut *client.cast::<Context>();
        let list = input.as_ref();
        let buffers =
            std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize);
        // The tap's stream comes after any other input streams.
        let tap_buffers = if context.interleaved { 1 } else { context.channels };
        if tap_buffers == 0 || buffers.len() < tap_buffers {
            return 0;
        }
        let first = buffers.len() - tap_buffers;

        let start = if input_time.as_ref().mFlags.contains(AudioTimeStampFlags::HostTimeValid) {
            clock::from_host_ticks(input_time.as_ref().mHostTime)
        } else if now.as_ref().mFlags.contains(AudioTimeStampFlags::HostTimeValid) {
            clock::from_host_ticks(now.as_ref().mHostTime)
        } else {
            clock::now()
        };

        let samples = |index: usize| -> &[f32] {
            let buffer = &buffers[index];
            if buffer.mData.is_null() {
                return &[];
            }
            std::slice::from_raw_parts(
                buffer.mData.cast::<f32>(),
                buffer.mDataByteSize as usize / size_of::<f32>(),
            )
        };
        if context.interleaved && context.channels > 1 {
            let stride = context.channels;
            let data = samples(first);
            let count = data.len() / stride;
            if count == 0 {
                return 0;
            }
            if context.left.len() < count {
                context.left.resize(count, 0.0);
                context.right.resize(count, 0.0);
            }
            for (i, frame) in data.chunks_exact(stride).enumerate() {
                context.left[i] = frame[0];
                context.right[i] = frame[1];
            }
            (context.handler)(&context.left[..count], &context.right[..count], context.sample_rate, start);
        } else {
            let left = samples(first);
            let right = if context.channels > 1 { samples(first + 1) } else { left };
            let count = left.len().min(right.len());
            if count > 0 {
                (context.handler)(&left[..count], &right[..count], context.sample_rate, start);
            }
        }
    }
    0
}

/// The tap's stream format, which must be 32-bit float PCM.
unsafe fn read_format(tap: AudioObjectID) -> Result<AudioStreamBasicDescription, String> {
    // SAFETY: an all-zero stream description is a valid value to read into.
    let zero: AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
    let format = unsafe { get(tap, kAudioTapPropertyFormat, kAudioObjectPropertyScopeGlobal, zero) }
        .ok_or("reading the tap format failed")?;
    if format.mFormatID != kAudioFormatLinearPCM
        || format.mFormatFlags & kAudioFormatFlagIsFloat == 0
        || format.mBitsPerChannel != 32
    {
        return Err(format!(
            "the tap's format is not 32-bit float PCM (flags {}, {} bits)",
            format.mFormatFlags, format.mBitsPerChannel
        ));
    }
    if !(format.mSampleRate > 0.0) || format.mChannelsPerFrame == 0 {
        return Err(format!(
            "the tap reports {} channels at {} Hz",
            format.mChannelsPerFrame, format.mSampleRate
        ));
    }
    Ok(format)
}

/// Describes a private aggregate device whose only stream is the tap `tap_uid`.
fn aggregate_properties(tap_uid: &NSString) -> Retained<NSDictionary<NSString, AnyObject>> {
    let key = |name: &CStr| NSString::from_str(&name.to_string_lossy());
    let object = |value: Retained<NSString>| -> Retained<AnyObject> { value.into() };
    let flag = |value: bool| -> Retained<AnyObject> { NSNumber::numberWithBool(value).into() };

    let tap = NSDictionary::<NSString, AnyObject>::from_retained_objects(
        &[&*key(kAudioSubTapUIDKey)],
        &[object(tap_uid.copy())],
    );
    let taps: Retained<AnyObject> = NSArray::from_retained_slice(&[tap]).into();
    let uid = format!("com.raspsoft.pulsr.tap.{}", NSUUID::new().UUIDString());
    NSDictionary::from_retained_objects(
        &[
            &*key(kAudioAggregateDeviceNameKey),
            &*key(kAudioAggregateDeviceUIDKey),
            &*key(kAudioAggregateDeviceIsPrivateKey),
            &*key(kAudioAggregateDeviceIsStackedKey),
            &*key(kAudioAggregateDeviceTapAutoStartKey),
            &*key(kAudioAggregateDeviceTapListKey),
        ],
        &[
            object(NSString::from_str("pulsr tap")),
            object(NSString::from_str(&uid)),
            flag(true),
            flag(false),
            flag(true),
            taps,
        ],
    )
}

/// The default output device's latency (device, safety offset and first stream).
pub fn output_info() -> Option<OutputInfo> {
    // SAFETY: property reads into correctly sized values.
    unsafe {
        let device = get(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyDefaultOutputDevice,
            kAudioObjectPropertyScopeGlobal,
            0u32,
        )
        .filter(|&id| id != 0)?;
        let rate = get(device, kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal, 0f64)
            .unwrap_or(0.0);
        let device_frames =
            get(device, kAudioDevicePropertyLatency, kAudioObjectPropertyScopeOutput, 0u32).unwrap_or(0);
        let safety_frames =
            get(device, kAudioDevicePropertySafetyOffset, kAudioObjectPropertyScopeOutput, 0u32).unwrap_or(0);
        let stream_frames = first_output_stream(device)
            .and_then(|stream| get(stream, kAudioStreamPropertyLatency, kAudioObjectPropertyScopeGlobal, 0u32))
            .unwrap_or(0);
        let frames = f64::from(device_frames) + f64::from(safety_frames) + f64::from(stream_frames);
        Some(OutputInfo { latency: if rate > 0.0 { frames / rate } else { 0.0 } })
    }
}

unsafe fn first_output_stream(device: AudioObjectID) -> Option<AudioObjectID> {
    let streams = address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput);
    let mut size = 0u32;
    // SAFETY: the size and data pointers are valid, and the data buffer is
    // as large as the size reported.
    unsafe {
        if AudioObjectGetPropertyDataSize(device, NonNull::from(&streams), 0, ptr::null(), NonNull::from(&mut size)) != 0 {
            return None;
        }
        let count = size as usize / size_of::<AudioObjectID>();
        if count == 0 {
            return None;
        }
        let mut ids = vec![0 as AudioObjectID; count];
        let status = AudioObjectGetPropertyData(
            device,
            NonNull::from(&streams),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(ids.as_mut_ptr())?.cast(),
        );
        (status == 0).then(|| ids[0])
    }
}

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property; `None` when the object lacks it.
/// Safety: `T` must be the type Core Audio writes for `selector`.
unsafe fn get<T: Copy>(object: AudioObjectID, selector: u32, scope: u32, initial: T) -> Option<T> {
    let address = address(selector, scope);
    let mut value = initial;
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` is `size` bytes of writable memory.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    (status == 0).then_some(value)
}

fn check(status: i32, step: &str) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    // An `OSStatus` as its four-character code when it is one.
    let bytes = (status as u32).to_be_bytes();
    let code = if bytes.iter().all(|b| (0x20..0x7F).contains(b)) {
        format!("'{}'", String::from_utf8_lossy(&bytes))
    } else {
        status.to_string()
    };
    Err(format!("{step} failed ({code})"))
}
