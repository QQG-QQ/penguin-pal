//! Local output-only audio features. No microphone, recording, network or song titles.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MusicMode {
    #[default]
    #[serde(alias = "auto")]
    Audio,
    Off,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub available: bool,
    pub active: bool,
    pub level: f32,
    pub bass: f32,
    pub treble: f32,
    pub pulse: f32,
    pub bpm: f32,
    pub confidence: f32,
    pub source: String,
    pub issue: String,
}

/// Fixed 10ms feature windows; sample rate and driver packet sizes cannot change tempo.
struct Analyzer {
    rate: u32,
    count: u32,
    sum: f32,
    low_sum: f32,
    high_sum: f32,
    low_coefficient: f32,
    mid_coefficient: f32,
    low: f32,
    mid: f32,
    envelope: f32,
    previous: f32,
    history: VecDeque<f32>,
    hops: u64,
    last_beat: u64,
    audible: u32,
    silence: u32,
    output: Snapshot,
}
impl Analyzer {
    fn new(rate: u32) -> Self {
        Self {
            rate,
            count: 0,
            sum: 0.,
            low_sum: 0.,
            high_sum: 0.,
            low: 0.,
            mid: 0.,
            low_coefficient: 1. - (-std::f32::consts::TAU * 180. / rate as f32).exp(),
            mid_coefficient: 1. - (-std::f32::consts::TAU * 2200. / rate as f32).exp(),
            envelope: 0.,
            previous: 0.,
            history: VecDeque::new(),
            hops: 0,
            last_beat: 0,
            audible: 0,
            silence: 0,
            output: Snapshot {
                available: true,
                ..Snapshot::default()
            },
        }
    }
    fn sample(&mut self, left: f32, right: f32) {
        let l = if left.is_finite() {
            left.clamp(-1., 1.)
        } else {
            0.
        };
        let r = if right.is_finite() {
            right.clamp(-1., 1.)
        } else {
            0.
        };
        // Stereo power avoids cancelling out-of-phase music when downmixing.
        let energy = (l * l + r * r) * 0.5;
        let x = (l + r) * 0.5;
        self.low += self.low_coefficient * (x - self.low);
        self.mid += self.mid_coefficient * (x - self.mid);
        self.sum += energy;
        self.low_sum += self.low * self.low;
        self.high_sum += (x - self.mid).powi(2);
        self.count += 1;
        if self.count >= self.rate / 100 {
            self.hop(
                (self.sum / self.count as f32).sqrt(),
                (self.low_sum / self.count as f32).sqrt(),
                (self.high_sum / self.count as f32).sqrt(),
            );
            self.count = 0;
            self.sum = 0.;
            self.low_sum = 0.;
            self.high_sum = 0.;
        }
    }
    fn hop(&mut self, rms: f32, bass: f32, high: f32) {
        self.hops += 1;
        let audible = rms > 0.0008;
        if audible {
            self.silence = 0;
        } else {
            self.silence += 1;
        }
        if audible {
            self.audible += 1;
        }
        // Require sustained output; ignore isolated notifications. Pause after a short tail.
        if self.silence > 60 {
            self.audible = 0;
            self.output.active = false;
        }
        if self.audible >= 150 {
            self.output.active = true;
        }
        let energy = (rms * 20.).ln_1p();
        let novelty = (energy - self.previous).max(0.);
        self.previous = energy;
        self.envelope = self.envelope * 0.97 + novelty * 0.03;
        if novelty > 0.025 && novelty > self.envelope * 2. && self.hops - self.last_beat >= 25 {
            self.output.pulse = 1.;
            self.last_beat = self.hops;
        } else {
            self.output.pulse *= 0.93;
        }
        self.history.push_back(novelty);
        if self.history.len() > 800 {
            self.history.pop_front();
        }
        if self.hops.is_multiple_of(100) {
            self.tempo();
        }
        self.output.level = (rms * 10.).sqrt().clamp(0., 1.);
        self.output.bass = (bass * 14.).sqrt().clamp(0., 1.);
        self.output.treble = (high * 20.).sqrt().clamp(0., 1.);
        if self.silence > 200 {
            self.output.bpm = 0.;
            self.output.confidence = 0.;
        }
    }
    fn tempo(&mut self) {
        if self.history.len() < 300 {
            return;
        }
        let raw: Vec<f32> = self.history.iter().copied().collect();
        // Tolerate a one-hop onset jitter without preferring a two-beat harmonic.
        let h: Vec<f32> = (0..raw.len())
            .map(|i| {
                raw[i] * 0.5
                    + raw[i.saturating_sub(1)] * 0.25
                    + raw[(i + 1).min(raw.len() - 1)] * 0.25
            })
            .collect();
        let mut best = (0., 0);
        let mut scores = vec![];
        for lag in 33..=90 {
            let mut cross = 0.;
            let mut a = 0.;
            let mut b = 0.;
            for i in lag..h.len() {
                cross += h[i] * h[i - lag];
                a += h[i] * h[i];
                b += h[i - lag] * h[i - lag];
            }
            let score = cross / (a * b).sqrt().max(1e-8);
            scores.push((score, lag));
            if score > best.0 {
                best = (score, lag);
            }
        }
        self.output.confidence = best.0.clamp(0., 1.);
        if best.0 > 0.3 {
            // Prefer the shorter local correlation peak when two beats score alike.
            if let Some(candidate) = scores
                .iter()
                .enumerate()
                .find(|(i, (s, _))| {
                    *s >= best.0 * 0.94
                        && (*i == 0 || *s >= scores[*i - 1].0)
                        && (*i + 1 == scores.len() || *s >= scores[*i + 1].0)
                })
                .map(|(_, v)| *v)
            {
                best = candidate;
            }
            let next = 6000. / best.1 as f32;
            self.output.bpm = if self.output.bpm == 0. || (next - self.output.bpm).abs() > 25. {
                next
            } else {
                self.output.bpm * 0.7 + next * 0.3
            };
        }
    }
}

struct Control {
    mode: MusicMode,
    touched: Instant,
}
pub struct MusicService {
    control: Arc<Mutex<Control>>,
    snapshot: Arc<Mutex<Snapshot>>,
    alive: Arc<AtomicBool>,
}
impl MusicService {
    pub fn new() -> Self {
        let service = Self {
            control: Arc::new(Mutex::new(Control {
                mode: MusicMode::Off,
                touched: Instant::now(),
            })),
            snapshot: Arc::new(Mutex::new(Snapshot::default())),
            alive: Arc::new(AtomicBool::new(true)),
        };
        #[cfg(windows)]
        {
            let control = service.control.clone();
            let snapshot = service.snapshot.clone();
            let alive = service.alive.clone();
            std::thread::spawn(move || platform::worker(control, snapshot, alive));
        }
        service
    }
    pub fn poll(&self, mode: MusicMode) -> Snapshot {
        if let Ok(mut c) = self.control.lock() {
            c.mode = mode;
            c.touched = Instant::now();
        }
        if mode == MusicMode::Off {
            return Snapshot::default();
        }
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn stop(&self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}
impl Drop for MusicService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::{
        core::Result,
        Win32::{
            Media::Audio::*,
            System::{
                Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
                WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
            },
        },
    };

    struct Capture {
        client: IAudioClient,
        reader: IAudioCaptureClient,
        volume: Endpoints::IAudioEndpointVolume,
        device: String,
        rate: u32,
        channels: usize,
        bytes: usize,
        float: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            unsafe {
                let _ = self.client.Stop();
            }
        }
    }
    fn endpoint() -> Result<IMMDevice> {
        unsafe {
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            e.GetDefaultAudioEndpoint(eRender, eMultimedia)
                .or_else(|_| e.GetDefaultAudioEndpoint(eRender, eConsole))
        }
    }
    fn device_id(device: &IMMDevice) -> Result<String> {
        unsafe {
            let id = device.GetId()?;
            let result = id.to_string();
            CoTaskMemFree(Some(id.0.cast()));
            Ok(result?)
        }
    }
    impl Capture {
        fn open() -> Result<Self> {
            unsafe {
                let device = endpoint()?;
                let id = device_id(&device)?;
                let volume: Endpoints::IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
                let format = client.GetMixFormat()?;
                let parsed = (|| {
                    let f = *format;
                    let (channels, rate, bits) = (f.nChannels, f.nSamplesPerSec, f.wBitsPerSample);
                    let tag = if f.wFormatTag == 0xfffe && f.cbSize >= 22 {
                        std::ptr::read_unaligned((format as *const u8).add(24).cast::<u32>())
                    } else {
                        f.wFormatTag as u32
                    };
                    if !(1..=16).contains(&channels)
                        || !(8000..=192000).contains(&rate)
                        || !((tag == 3 && bits == 32) || (tag == 1 && [16, 24, 32].contains(&bits)))
                        || f.nBlockAlign as usize
                            != f.nChannels as usize * (f.wBitsPerSample / 8) as usize
                    {
                        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                            0x80070032u32 as i32,
                        )));
                    }
                    client.Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        AUDCLNT_STREAMFLAGS_LOOPBACK,
                        1_000_000,
                        0,
                        format,
                        None,
                    )?;
                    Ok((
                        f.nSamplesPerSec,
                        f.nChannels as usize,
                        (f.wBitsPerSample / 8) as usize,
                        tag == 3,
                    ))
                })();
                CoTaskMemFree(Some(format.cast()));
                let (rate, channels, bytes, float) = parsed?;
                let reader = client.GetService()?;
                client.Start()?;
                Ok(Self {
                    client,
                    reader,
                    volume,
                    device: id,
                    rate,
                    channels,
                    bytes,
                    float,
                })
            }
        }
        fn read(&self, analyzer: &mut Analyzer) -> Result<bool> {
            unsafe {
                let mut got = false;
                let muted = self.volume.GetMute()?.as_bool()
                    || self.volume.GetMasterVolumeLevelScalar()? <= 0.001;
                for _ in 0..32 {
                    if self.reader.GetNextPacketSize()? == 0 {
                        break;
                    }
                    let (mut data, mut count, mut flags) = (std::ptr::null_mut(), 0, 0);
                    self.reader
                        .GetBuffer(&mut data, &mut count, &mut flags, None, None)?;
                    let silent =
                        muted || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
                    let size = count as usize * self.channels * self.bytes;
                    if count > self.rate || size > 16 * 1024 * 1024 {
                        self.reader.ReleaseBuffer(count)?;
                        break;
                    }
                    let buffer = if silent {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts(data, size)
                    };
                    for frame in 0..count as usize {
                        let get = |ch: usize| {
                            if silent {
                                return 0.;
                            }
                            let p = (frame * self.channels + ch) * self.bytes;
                            let b = &buffer[p..p + self.bytes];
                            if self.float {
                                f32::from_le_bytes(b.try_into().unwrap())
                            } else {
                                match self.bytes {
                                    2 => i16::from_le_bytes(b.try_into().unwrap()) as f32 / 32768.,
                                    3 => {
                                        ((b[0] as i32
                                            | ((b[1] as i32) << 8)
                                            | ((b[2] as i32) << 16))
                                            << 8) as f32
                                            / 2147483648.
                                    }
                                    _ => {
                                        i32::from_le_bytes(b.try_into().unwrap()) as f32
                                            / 2147483648.
                                    }
                                }
                            }
                        };
                        analyzer.sample(get(0), get(if self.channels > 1 { 1 } else { 0 }));
                    }
                    self.reader.ReleaseBuffer(count)?;
                    got = true;
                }
                Ok(got)
            }
        }
    }
    pub(super) fn worker(
        control: Arc<Mutex<Control>>,
        snapshot: Arc<Mutex<Snapshot>>,
        alive: Arc<AtomicBool>,
    ) {
        if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
            if let Ok(mut s) = snapshot.lock() {
                s.issue = "Windows 音频服务初始化失败，请重启企鹅".into();
            }
            return;
        }
        {
            let mut capture: Option<Capture> = None;
            let mut analyzer = Analyzer::new(48000);
            let mut checked = Instant::now() - Duration::from_secs(10);
            let mut retry = Instant::now();
            let mut mode_was = MusicMode::Off;
            let mut packet_at = Instant::now();
            while alive.load(Ordering::Relaxed) {
                let mode = control
                    .lock()
                    .map(|c| {
                        if c.touched.elapsed() < Duration::from_secs(3) {
                            c.mode
                        } else {
                            MusicMode::Off
                        }
                    })
                    .unwrap_or(MusicMode::Off);
                if mode == MusicMode::Off {
                    capture = None;
                    mode_was = mode;
                    if let Ok(mut s) = snapshot.lock() {
                        *s = Snapshot::default();
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if mode != mode_was {
                    analyzer = Analyzer::new(capture.as_ref().map(|c| c.rate).unwrap_or(48000));
                    mode_was = mode;
                    checked = Instant::now() - Duration::from_secs(10);
                }
                if checked.elapsed() > Duration::from_secs(1) {
                    checked = Instant::now();
                    if let Some(c) = &capture {
                        if endpoint().and_then(|d| device_id(&d)).ok().as_deref()
                            != Some(c.device.as_str())
                        {
                            capture = None;
                        }
                    }
                }
                if capture.is_none() && Instant::now() >= retry {
                    match Capture::open() {
                        Ok(c) => {
                            analyzer = Analyzer::new(c.rate);
                            capture = Some(c);
                            packet_at = Instant::now();
                        }
                        Err(error) => {
                            #[cfg(test)]
                            eprintln!("loopback initialization: {error:?}");
                            retry = Instant::now() + Duration::from_secs(2);
                            if let Ok(mut s) = snapshot.lock() {
                                *s = Snapshot {
                                    issue: if error.code().0 == 0x80070490u32 as i32 {
                                        "未检测到播放设备，请连接扬声器或耳机"
                                    } else {
                                        "音频输出暂不可用，正在重试"
                                    }
                                    .into(),
                                    ..Snapshot::default()
                                };
                            }
                        }
                    }
                }
                if let Some(c) = &capture {
                    match c.read(&mut analyzer) {
                        Ok(false) => {
                            if packet_at.elapsed() > Duration::from_millis(120) {
                                analyzer.hop(0., 0., 0.);
                            }
                        }
                        Ok(true) => {
                            packet_at = Instant::now();
                        }
                        Err(_) => {
                            capture = None;
                            retry = Instant::now() + Duration::from_millis(500);
                            analyzer = Analyzer::new(48000);
                        }
                    }
                    if let Ok(mut s) = snapshot.lock() {
                        *s = analyzer.output.clone();
                        s.source = if s.active {
                            "电脑播放声音"
                        } else {
                            "等待音乐或浏览器播放声音"
                        }
                        .into();
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        unsafe {
            RoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn beats(bpm: f32) -> Analyzer {
        let mut a = Analyzer::new(48000);
        for i in 0..1000 {
            let t = i as f32 * 0.01;
            let phase = (t * bpm / 60.).fract();
            a.hop(if phase < 0.04 { 0.1 } else { 0.003 }, 0.01, 0.01);
        }
        a
    }
    #[test]
    fn tempo_tracks_pulses_instead_of_loudness() {
        for bpm in [80., 100., 120., 150., 180.] {
            let a = beats(bpm);
            assert!((a.output.bpm - bpm).abs() < 4., "{bpm}: {:?}", a.output);
            assert!(a.output.confidence > 0.4);
        }
        let mut flat = Analyzer::new(48000);
        for _ in 0..800 {
            flat.hop(0.2, 0.2, 0.2);
        }
        assert!(flat.output.confidence < 0.3);
    }
    #[test]
    fn notification_is_rejected_and_pause_clears_activity() {
        let mut a = Analyzer::new(48000);
        for _ in 0..40 {
            a.hop(0.1, 0.1, 0.1);
        }
        for _ in 0..100 {
            a.hop(0., 0., 0.);
        }
        assert!(!a.output.active);
        let mut a = beats(120.);
        assert!(a.output.active);
        for _ in 0..220 {
            a.hop(0., 0., 0.);
        }
        assert!(!a.output.active);
        assert_eq!(a.output.bpm, 0.);
    }
    #[test]
    fn stereo_cancellation_and_nonfinite_samples_are_safe() {
        let mut a = Analyzer::new(48000);
        for _ in 0..480 {
            a.sample(0.1, -0.1);
        }
        assert!(a.output.level > 0.1);
        for _ in 0..480 {
            a.sample(f32::NAN, f32::INFINITY);
        }
        assert!(a.output.level.is_finite());
    }
    #[test]
    fn tempo_is_stable_across_output_sample_rates() {
        for rate in [44100, 48000, 96000] {
            let mut a = Analyzer::new(rate);
            for i in 0..rate * 8 {
                let t = i as f32 / rate as f32;
                let amplitude = if (t * 2.).fract() < 0.08 { 0.1 } else { 0.003 };
                let sample = amplitude * (std::f32::consts::TAU * 100. * t).sin();
                a.sample(sample, sample);
            }
            assert!(a.output.active, "{rate}");
            assert!((a.output.bpm - 120.).abs() < 3., "{rate}: {:?}", a.output);
            assert!(a.output.confidence > 0.4, "{rate}");
        }
    }
    #[test]
    fn old_preferences_migrate_to_audio_and_unknown_mode_is_rejected() {
        assert_eq!(MusicMode::default(), MusicMode::Audio);
        for legacy in ["auto", "audio"] {
            let mode: MusicMode = serde_json::from_value(serde_json::json!(legacy)).unwrap();
            assert_eq!(mode, MusicMode::Audio);
            assert_eq!(serde_json::to_value(mode).unwrap(), "audio");
        }
        assert_eq!(
            serde_json::from_str::<MusicMode>("\"off\"").unwrap(),
            MusicMode::Off
        );
        assert!(serde_json::from_str::<MusicMode>("\"execute\"").is_err());
        let s = MusicService::new();
        assert!(!s.poll(MusicMode::Off).active);
        s.stop();
    }
    #[test]
    #[ignore = "requires explicit live output playback"]
    fn live_output_probe() {
        let s = MusicService::new();
        let start = Instant::now();
        let mut peak = 0f32;
        let mut active = false;
        let mut bpm = 0.;
        let mut last = Snapshot::default();
        while start.elapsed() < Duration::from_secs(12) {
            let v = s.poll(MusicMode::Audio);
            peak = peak.max(v.level);
            active |= v.active;
            if v.confidence > 0.3 {
                bpm = v.bpm;
            }
            last = v;
            std::thread::sleep(Duration::from_millis(50));
        }
        s.poll(MusicMode::Off);
        std::thread::sleep(Duration::from_millis(150));
        assert!(!s.snapshot.lock().unwrap().active);
        s.stop();
        println!("loopback peak={peak:.3} active={active} bpm={bpm:.1} last={last:?}");
        assert!(peak > 0.05 && active, "No actual output captured");
    }
}
