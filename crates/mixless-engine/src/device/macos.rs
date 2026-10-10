//! Physical CoreAudio output formats. All property access stays on host threads.
//! AudioUnit's float client format is deliberately separate from the DAC format.
use super::{audio_error, AudioConfig, AudioOutputFormat, DeviceError};
use core_foundation_sys::{
    base::CFRelease,
    string::{kCFStringEncodingUTF8, CFStringGetCString, CFStringRef},
};
use coreaudio_sys::*;
use std::{
    mem::{size_of, MaybeUninit},
    ptr,
    time::{Duration, Instant},
};

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMaster,
    }
}

fn check(status: OSStatus) -> Result<(), DeviceError> {
    if status == 0 {
        Ok(())
    } else {
        Err(audio_error(format!("CoreAudio property error {status}")))
    }
}

// Private callers use only CoreAudio POD types (integers, doubles, ASBDs and pointers).
fn get<T: Copy>(id: u32, property: &AudioObjectPropertyAddress) -> Result<T, DeviceError> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut bytes = size_of::<T>() as u32;
    unsafe {
        check(AudioObjectGetPropertyData(
            id,
            property,
            0,
            ptr::null(),
            &mut bytes,
            value.as_mut_ptr().cast(),
        ))?;
        if bytes as usize != size_of::<T>() {
            return Err(audio_error("unexpected CoreAudio property size"));
        }
        Ok(value.assume_init())
    }
}

fn list<T: Copy>(id: u32, property: &AudioObjectPropertyAddress) -> Result<Vec<T>, DeviceError> {
    let mut bytes = 0;
    unsafe {
        check(AudioObjectGetPropertyDataSize(
            id,
            property,
            0,
            ptr::null(),
            &mut bytes,
        ))?;
    }
    if !(bytes as usize).is_multiple_of(size_of::<T>()) || bytes > 1024 * 1024 {
        return Err(audio_error("invalid CoreAudio list size"));
    }
    let count = bytes as usize / size_of::<T>();
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut values = Vec::<T>::with_capacity(count);
    unsafe {
        check(AudioObjectGetPropertyData(
            id,
            property,
            0,
            ptr::null(),
            &mut bytes,
            values.as_mut_ptr().cast(),
        ))?;
        if !(bytes as usize).is_multiple_of(size_of::<T>())
            || bytes as usize > count * size_of::<T>()
        {
            return Err(audio_error("CoreAudio list changed during discovery"));
        }
        values.set_len(bytes as usize / size_of::<T>());
    }
    Ok(values)
}

fn set<T: Copy>(
    id: u32,
    property: &AudioObjectPropertyAddress,
    value: &T,
) -> Result<(), DeviceError> {
    let mut settable = 0;
    unsafe {
        check(AudioObjectIsPropertySettable(id, property, &mut settable))?;
        if settable == 0 {
            return Err(audio_error("device output format is read-only"));
        }
        check(AudioObjectSetPropertyData(
            id,
            property,
            0,
            ptr::null(),
            size_of::<T>() as u32,
            (value as *const T).cast(),
        ))
    }
}

fn string_property(id: u32, selector: u32) -> Result<String, DeviceError> {
    let value: CFStringRef = get(id, &address(selector, kAudioObjectPropertyScopeGlobal))?;
    if value.is_null() {
        return Err(audio_error("device name unavailable"));
    }
    let mut text = vec![0i8; 4096];
    let success = unsafe {
        CFStringGetCString(
            value,
            text.as_mut_ptr(),
            text.len() as _,
            kCFStringEncodingUTF8,
        )
    };
    unsafe {
        CFRelease(value.cast());
    }
    if success == 0 {
        return Err(audio_error("device name is too long"));
    }
    Ok(unsafe { std::ffi::CStr::from_ptr(text.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

fn device_name(id: u32) -> Result<String, DeviceError> {
    string_property(id, kAudioObjectPropertyName)
}
pub(super) fn uid(name: &str) -> Result<String, DeviceError> {
    string_property(find_device(Some(name))?, kAudioDevicePropertyDeviceUID)
}
pub(super) fn name_for_uid(uid: &str) -> Result<String, DeviceError> {
    let ids: Vec<u32> = list(
        kAudioObjectSystemObject,
        &address(
            kAudioHardwarePropertyDevices,
            kAudioObjectPropertyScopeGlobal,
        ),
    )?;
    for id in ids {
        if string_property(id, kAudioDevicePropertyDeviceUID)
            .ok()
            .as_deref()
            == Some(uid)
        {
            return device_name(id);
        }
    }
    Err(audio_error("Assigned audio device is disconnected"))
}

fn find_device(name: Option<&str>) -> Result<u32, DeviceError> {
    let Some(name) = name else {
        let id = get(
            kAudioObjectSystemObject,
            &address(
                kAudioHardwarePropertyDefaultOutputDevice,
                kAudioObjectPropertyScopeGlobal,
            ),
        )?;
        return if id == kAudioObjectUnknown {
            Err(DeviceError::NoDevice)
        } else {
            Ok(id)
        };
    };
    let ids: Vec<u32> = list(
        kAudioObjectSystemObject,
        &address(
            kAudioHardwarePropertyDevices,
            kAudioObjectPropertyScopeGlobal,
        ),
    )?;
    let mut found = None;
    for id in ids {
        if device_name(id).ok().as_deref() == Some(name)
            && !list::<u32>(
                id,
                &address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
            )?
            .is_empty()
        {
            if found.is_some() {
                return Err(audio_error(
                    "multiple outputs have the same name; use System Default",
                ));
            }
            found = Some(id);
        }
    }
    found.ok_or_else(|| audio_error(format!("device unavailable: {name}")))
}

fn pcm(format: &AudioStreamBasicDescription) -> bool {
    format.mFormatID == kAudioFormatLinearPCM
        && format.mFormatFlags & kAudioFormatFlagIsNonMixable == 0
        && format.mChannelsPerFrame > 0
        && matches!(format.mBitsPerChannel, 16 | 24 | 32 | 64)
}

fn matches_range(
    range: &AudioStreamRangedDescription,
    rate: u32,
    bits: u32,
    channels: u32,
) -> bool {
    pcm(&range.mFormat)
        && range.mFormat.mBitsPerChannel == bits
        && range.mFormat.mChannelsPerFrame == channels
        && range.mSampleRateRange.mMinimum <= rate as f64
        && range.mSampleRateRange.mMaximum >= rate as f64
}

fn same_format(a: &AudioStreamBasicDescription, b: &AudioStreamBasicDescription) -> bool {
    (a.mSampleRate - b.mSampleRate).abs() < 0.01
        && a.mFormatID == b.mFormatID
        && a.mFormatFlags == b.mFormatFlags
        && a.mBitsPerChannel == b.mBitsPerChannel
        && a.mChannelsPerFrame == b.mChannelsPerFrame
        && a.mBytesPerFrame == b.mBytesPerFrame
        && a.mBytesPerPacket == b.mBytesPerPacket
        && a.mFramesPerPacket == b.mFramesPerPacket
}

struct HardwareStream {
    id: u32,
    current: AudioStreamBasicDescription,
    available: Vec<AudioStreamRangedDescription>,
}

pub(super) struct HardwareOutput {
    id: u32,
    rate: f64,
    streams: Vec<HardwareStream>,
}

impl HardwareOutput {
    pub fn sample_rate(&self) -> u32 {
        self.rate.round() as u32
    }
    pub fn query(name: Option<&str>) -> Result<Self, DeviceError> {
        let id = find_device(name)?;
        let rate: f64 = get(
            id,
            &address(
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            ),
        )?;
        let ids: Vec<u32> = list(
            id,
            &address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
        )?;
        let streams = ids
            .into_iter()
            .map(|id| {
                Ok(HardwareStream {
                    id,
                    current: get(
                        id,
                        &address(
                            kAudioStreamPropertyPhysicalFormat,
                            kAudioObjectPropertyScopeGlobal,
                        ),
                    )?,
                    available: list(
                        id,
                        &address(
                            kAudioStreamPropertyAvailablePhysicalFormats,
                            kAudioObjectPropertyScopeGlobal,
                        ),
                    )?,
                })
            })
            .collect::<Result<Vec<_>, DeviceError>>()?;
        if streams.is_empty() || !rate.is_finite() || rate <= 0. {
            return Err(audio_error("physical output format unavailable"));
        }
        Ok(Self { id, rate, streams })
    }

    pub fn available_formats(&self) -> Vec<AudioOutputFormat> {
        let mut rates = AudioConfig::SAMPLE_RATES.to_vec();
        rates.push(self.sample_rate());
        rates.sort_unstable();
        rates.dedup();
        let mut result = Vec::new();
        for rate in rates
            .into_iter()
            .filter(|r| AudioConfig::supports_sample_rate(*r))
        {
            for bits in [16, 24, 32, 64] {
                for float in [false, true] {
                    if self.streams.iter().all(|stream| {
                        stream.available.iter().any(|range| {
                            matches_range(range, rate, bits, stream.current.mChannelsPerFrame)
                                && (range.mFormat.mFormatFlags & kAudioFormatFlagIsFloat != 0)
                                    == float
                        })
                    }) {
                        result.push(AudioOutputFormat {
                            sample_rate: rate,
                            bit_depth: bits as u16,
                            is_float: float,
                        });
                    }
                }
            }
        }
        result
    }
}

pub(super) struct HardwareChange {
    original: HardwareOutput,
    target_rate: u32,
    targets: Vec<AudioStreamBasicDescription>,
    committed: bool,
    changed: bool,
}

impl HardwareChange {
    pub fn prepare(
        name: Option<&str>,
        rate: Option<u32>,
        bits: Option<u16>,
    ) -> Result<Self, DeviceError> {
        let original = HardwareOutput::query(name)?;
        let target_rate = rate.unwrap_or(original.rate.round() as u32);
        if !AudioConfig::supports_sample_rate(target_rate) {
            return Err(audio_error(
                "sample rate must be between 8000 and 192000 Hz",
            ));
        }
        let targets = original
            .streams
            .iter()
            .map(|stream| {
                let depth = bits.map_or(stream.current.mBitsPerChannel, u32::from);
                // Preserve the existing physical representation when matching its rate/depth.
                if (stream.current.mSampleRate - target_rate as f64).abs() < 0.01
                    && stream.current.mBitsPerChannel == depth
                    && pcm(&stream.current)
                {
                    return Ok(stream.current);
                }
                let mut target = stream
                    .available
                    .iter()
                    .filter(|range| {
                        matches_range(range, target_rate, depth, stream.current.mChannelsPerFrame)
                    })
                    .min_by_key(|range| {
                        (range.mFormat.mFormatFlags & kAudioFormatFlagIsFloat)
                            != (stream.current.mFormatFlags & kAudioFormatFlagIsFloat)
                    })
                    .map(|range| range.mFormat)
                    .ok_or_else(|| {
                        audio_error(format!(
                            "device does not support {target_rate} Hz / {depth}-bit output"
                        ))
                    })?;
                target.mSampleRate = target_rate as f64;
                Ok(target)
            })
            .collect::<Result<Vec<_>, DeviceError>>()?;
        let mut change = Self {
            original,
            target_rate,
            targets,
            committed: false,
            changed: false,
        };
        change.apply()?;
        Ok(change)
    }

    fn apply(&mut self) -> Result<(), DeviceError> {
        if (self.original.rate - self.target_rate as f64).abs() >= 0.01 {
            self.changed = true;
            set_rate(self.original.id, self.target_rate as f64)?;
        }
        for (stream, target) in self.original.streams.iter().zip(&self.targets) {
            let actual = get(
                stream.id,
                &address(
                    kAudioStreamPropertyPhysicalFormat,
                    kAudioObjectPropertyScopeGlobal,
                ),
            )?;
            if !same_format(&actual, target) {
                self.changed = true;
                set_format(stream.id, target)?;
            }
        }
        self.verify()
    }

    pub fn sample_rate(&self) -> u32 {
        self.target_rate
    }
    pub fn description(&self) -> String {
        let mut descriptions = self
            .targets
            .iter()
            .map(|format| {
                AudioOutputFormat {
                    sample_rate: self.target_rate,
                    bit_depth: format.mBitsPerChannel as u16,
                    is_float: format.mFormatFlags & kAudioFormatFlagIsFloat != 0,
                }
                .label()
            })
            .collect::<Vec<_>>();
        descriptions.sort();
        descriptions.dedup();
        descriptions.join(" / ")
    }

    pub fn verify(&self) -> Result<(), DeviceError> {
        let rate: f64 = get(
            self.original.id,
            &address(
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            ),
        )?;
        if (rate - self.target_rate as f64).abs() >= 0.01 {
            return Err(audio_error(
                "hardware sample rate did not match the audio stream",
            ));
        }
        for (stream, target) in self.original.streams.iter().zip(&self.targets) {
            let actual = get(
                stream.id,
                &address(
                    kAudioStreamPropertyPhysicalFormat,
                    kAudioObjectPropertyScopeGlobal,
                ),
            )?;
            if !same_format(&actual, target) {
                return Err(audio_error(
                    "hardware output format did not match the requested format",
                ));
            }
        }
        Ok(())
    }

    pub fn selected_device_matches(&self, name: Option<&str>) -> bool {
        find_device(name).ok() == Some(self.original.id)
    }

    pub fn commit(&mut self) {
        self.committed = true;
    }

    fn restore(&self) -> Result<(), DeviceError> {
        // Attempt every restoration even if a device disconnects partway through.
        let mut result = set_rate(self.original.id, self.original.rate);
        for stream in &self.original.streams {
            if let Err(error) = set_format(stream.id, &stream.current) {
                result = Err(error);
            }
        }
        result
    }
}

impl Drop for HardwareChange {
    fn drop(&mut self) {
        if self.changed && !self.committed {
            if let Err(error) = self.restore() {
                tracing::error!("physical output restore failed: {error}");
            }
        }
    }
}

fn wait_until(mut matched: impl FnMut() -> Result<bool, DeviceError>) -> Result<(), DeviceError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if matched()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(audio_error(
                "device did not adopt the requested output format",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn set_rate(id: u32, rate: f64) -> Result<(), DeviceError> {
    let property = address(
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
    );
    if (get::<f64>(id, &property)? - rate).abs() < 0.01 {
        return Ok(());
    }
    set(id, &property, &rate)?;
    wait_until(|| Ok((get::<f64>(id, &property)? - rate).abs() < 0.01))
}

fn set_format(id: u32, format: &AudioStreamBasicDescription) -> Result<(), DeviceError> {
    let property = address(
        kAudioStreamPropertyPhysicalFormat,
        kAudioObjectPropertyScopeGlobal,
    );
    if same_format(&get(id, &property)?, format) {
        return Ok(());
    }
    set(id, &property, format)?;
    wait_until(|| Ok(same_format(&get(id, &property)?, format)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "changes the system output format to verify automatic matching"]
    fn automatic_output_follows_system_changes_after_recording() {
        let engine = crate::Engine::new(crate::EngineConfig::default()).unwrap();
        assert!(engine.audio_available());
        let original = HardwareOutput::query(None).unwrap();
        let bits = original.streams[0].current.mBitsPerChannel as u16;
        let rate = original
            .available_formats()
            .into_iter()
            .find(|f| f.sample_rate != original.sample_rate() && f.bit_depth == bits)
            .expect("two physical sample rates needed for this smoke")
            .sample_rate;
        let directory = tempfile::tempdir().unwrap();
        engine
            .start_recording(&directory.path().join("system-format.wav"))
            .unwrap();
        let mut external = HardwareChange::prepare(None, Some(rate), Some(bits)).unwrap();
        external.commit();
        std::thread::sleep(Duration::from_millis(1200));
        assert_eq!(
            engine.sample_rate(),
            original.sample_rate(),
            "must not change WAV rate during recording"
        );
        engine.stop_recording().unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while engine.sample_rate() != rate && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(engine.sample_rate(), rate);
        assert!(engine.audio_available());
        assert_eq!(engine.audio_config(), super::super::AudioConfig::default());
        assert_eq!(engine.snapshot().master_output.unwrap().sample_rate, rate);
        eprintln!(
            "system rate {} -> {} Hz followed automatically after recording",
            original.sample_rate(),
            rate
        );
        engine
            .configure_audio(super::super::AudioConfig {
                sample_rate: Some(original.sample_rate()),
                bit_depth: Some(bits),
                ..Default::default()
            })
            .unwrap();
        engine
            .configure_audio(super::super::AudioConfig::default())
            .unwrap();
    }
    fn format(rate: f64, bits: u32, channels: u32) -> AudioStreamBasicDescription {
        AudioStreamBasicDescription {
            mSampleRate: rate,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
            mBytesPerPacket: channels * bits / 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: channels * bits / 8,
            mChannelsPerFrame: channels,
            mBitsPerChannel: bits,
            mReserved: 0,
        }
    }
    #[test]
    fn physical_formats_require_the_complete_rate_depth_channel_combination() {
        let mut range = AudioStreamRangedDescription {
            mFormat: format(48_000., 24, 2),
            mSampleRateRange: AudioValueRange {
                mMinimum: 48_000.,
                mMaximum: 48_000.,
            },
        };
        assert!(matches_range(&range, 48_000, 24, 2));
        assert!(!matches_range(&range, 44_100, 24, 2));
        assert!(!matches_range(&range, 48_000, 16, 2));
        assert!(!matches_range(&range, 48_000, 24, 4));
        range.mFormat.mFormatFlags |= kAudioFormatFlagIsNonMixable;
        assert!(!matches_range(&range, 48_000, 24, 2));
    }
    #[test]
    fn advertised_formats_are_the_intersection_of_all_output_streams() {
        let stream = |id, bits| HardwareStream {
            id,
            current: format(48_000., bits, 2),
            available: vec![AudioStreamRangedDescription {
                mFormat: format(48_000., bits, 2),
                mSampleRateRange: AudioValueRange {
                    mMinimum: 48_000.,
                    mMaximum: 48_000.,
                },
            }],
        };
        let hardware = HardwareOutput {
            id: 1,
            rate: 48_000.,
            streams: vec![stream(2, 24), stream(3, 24)],
        };
        assert_eq!(
            hardware.available_formats(),
            vec![AudioOutputFormat {
                sample_rate: 48_000,
                bit_depth: 24,
                is_float: false
            }]
        );
        let hardware = HardwareOutput {
            streams: vec![stream(2, 24), stream(3, 16)],
            ..hardware
        };
        assert!(hardware.available_formats().is_empty());
    }
}
