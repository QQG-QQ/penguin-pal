use crate::app_state::AudioStage;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use parking_lot::Mutex;
use ringbuf::{
    traits::{Consumer, Producer, Split},
    HeapRb,
};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const MAX_CAPTURE_SECONDS: usize = 30;

pub fn input_mode() -> &'static str {
    "whisper-local"
}

pub fn stage() -> AudioStage {
    AudioStage {
        id: "recorder".to_string(),
        title: "本地麦克风采集".to_string(),
        summary: "使用 cpal 采集麦克风音频，16kHz mono PCM。".to_string(),
        status: "ready".to_string(),
    }
}

#[derive(Debug)]
pub enum AudioCommand {
    Start(mpsc::Sender<Result<(), String>>),
    Stop(mpsc::Sender<Result<Vec<f32>, String>>),
    Shutdown,
}

#[derive(Debug, Clone)]
struct SelectedInputConfig {
    config: StreamConfig,
    sample_format: SampleFormat,
}

pub struct AudioRecorder {
    command_tx: mpsc::Sender<AudioCommand>,
    samples: Arc<Mutex<Vec<f32>>>,
    #[allow(dead_code)]
    is_recording: Arc<Mutex<bool>>,
    input_ready: Arc<Mutex<bool>>,
    last_error: Arc<Mutex<Option<String>>>,
    worker_handle: Option<thread::JoinHandle<()>>,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AudioCommand>();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let is_recording = Arc::new(Mutex::new(false));
        let input_ready = Arc::new(Mutex::new(false));
        let last_error = Arc::new(Mutex::new(None));

        let samples_clone = samples.clone();
        let is_recording_clone = is_recording.clone();
        let input_ready_clone = input_ready.clone();
        let last_error_clone = last_error.clone();

        let handle = thread::spawn(move || {
            Self::worker_loop(
                cmd_rx,
                samples_clone,
                is_recording_clone,
                input_ready_clone,
                last_error_clone,
            );
        });

        Ok(Self {
            command_tx: cmd_tx,
            samples,
            is_recording,
            input_ready,
            last_error,
            worker_handle: Some(handle),
        })
    }

    fn set_input_status(
        input_ready: &Arc<Mutex<bool>>,
        last_error: &Arc<Mutex<Option<String>>>,
        ready: bool,
        error: Option<String>,
    ) {
        *input_ready.lock() = ready;
        *last_error.lock() = error;
    }

    fn choose_input_config(device: &cpal::Device) -> Result<SelectedInputConfig, String> {
        let supported_configs = device
            .supported_input_configs()
            .map_err(|error| format!("枚举麦克风输入配置失败: {}", error))?;

        let mut fallback_multi_channel = None;
        for range in supported_configs {
            if range.min_sample_rate().0 <= TARGET_SAMPLE_RATE
                && range.max_sample_rate().0 >= TARGET_SAMPLE_RATE
            {
                let candidate = SelectedInputConfig {
                    config: StreamConfig {
                        channels: range.channels(),
                        sample_rate: cpal::SampleRate(TARGET_SAMPLE_RATE),
                        buffer_size: cpal::BufferSize::Default,
                    },
                    sample_format: range.sample_format(),
                };

                if range.channels() == 1 {
                    return Ok(candidate);
                }

                if fallback_multi_channel.is_none() {
                    fallback_multi_channel = Some(candidate);
                }
            }
        }

        if let Some(candidate) = fallback_multi_channel {
            return Ok(candidate);
        }

        let default_config = device
            .default_input_config()
            .map_err(|error| format!("读取默认麦克风配置失败: {}", error))?;

        if default_config.sample_rate().0 != TARGET_SAMPLE_RATE {
            return Err(format!(
                "当前默认麦克风采样率为 {}Hz，且未发现支持 16000Hz 的输入配置。",
                default_config.sample_rate().0
            ));
        }

        Ok(SelectedInputConfig {
            config: default_config.config(),
            sample_format: default_config.sample_format(),
        })
    }

    fn push_input_data<T, P>(data: &[T], channels: u16, producer: &Arc<Mutex<P>>)
    where
        T: Sample + Copy,
        f32: FromSample<T>,
        P: Producer<Item = f32>,
    {
        let channel_count = channels.max(1) as usize;
        let mut prod = producer.lock();
        for frame in data.chunks(channel_count) {
            let mut sum = 0.0_f32;
            for sample in frame {
                sum += f32::from_sample(*sample);
            }
            let mono = sum / frame.len() as f32;
            let _ = prod.try_push(mono);
        }
    }

    fn build_probe_stream_for_format<T>(
        device: &cpal::Device,
        config: &StreamConfig,
    ) -> Result<Stream, String>
    where
        T: SizedSample,
    {
        device
            .build_input_stream(
                config,
                move |_data: &[T], _: &cpal::InputCallbackInfo| {},
                move |error| eprintln!("[AudioRecorder] Probe stream error: {}", error),
                None,
            )
            .map_err(|error| format!("初始化麦克风输入失败: {}", error))
    }

    fn build_probe_stream(
        device: &cpal::Device,
        selected: SelectedInputConfig,
    ) -> Result<Stream, String> {
        match selected.sample_format {
            SampleFormat::F32 => Self::build_probe_stream_for_format::<f32>(device, &selected.config),
            SampleFormat::I16 => Self::build_probe_stream_for_format::<i16>(device, &selected.config),
            SampleFormat::U16 => Self::build_probe_stream_for_format::<u16>(device, &selected.config),
            other => Err(format!("当前麦克风采样格式 {:?} 暂不支持。", other)),
        }
    }

    fn build_stream_for_format<T, P>(
        device: &cpal::Device,
        config: &StreamConfig,
        producer: Arc<Mutex<P>>,
    ) -> Result<Stream, String>
    where
        T: SizedSample + Sample + Copy,
        f32: FromSample<T>,
        P: Producer<Item = f32> + Send + 'static,
    {
        let channels = config.channels;
        let err_fn = |err| eprintln!("[AudioRecorder] Stream error: {}", err);
        device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    Self::push_input_data(data, channels, &producer);
                },
                err_fn,
                None,
            )
            .map_err(|error| format!("初始化麦克风输入失败: {}", error))
    }

    fn build_stream<P>(
        device: &cpal::Device,
        selected: SelectedInputConfig,
        producer: Arc<Mutex<P>>,
    ) -> Result<Stream, String>
    where
        P: Producer<Item = f32> + Send + 'static,
    {
        match selected.sample_format {
            SampleFormat::F32 => Self::build_stream_for_format::<f32, P>(
                device,
                &selected.config,
                producer,
            ),
            SampleFormat::I16 => Self::build_stream_for_format::<i16, P>(
                device,
                &selected.config,
                producer,
            ),
            SampleFormat::U16 => Self::build_stream_for_format::<u16, P>(
                device,
                &selected.config,
                producer,
            ),
            other => Err(format!("当前麦克风采样格式 {:?} 暂不支持。", other)),
        }
    }

    fn probe_input_device() -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "未检测到可用麦克风输入设备".to_string())?;
        let selected = Self::choose_input_config(&device)?;
        let _stream = Self::build_probe_stream(&device, selected)?;
        Ok(())
    }

    fn worker_loop(
        cmd_rx: mpsc::Receiver<AudioCommand>,
        samples: Arc<Mutex<Vec<f32>>>,
        is_recording: Arc<Mutex<bool>>,
        input_ready: Arc<Mutex<bool>>,
        last_error: Arc<Mutex<Option<String>>>,
    ) {
        let rb = HeapRb::<f32>::new(TARGET_SAMPLE_RATE as usize * MAX_CAPTURE_SECONDS);
        let (producer, mut consumer) = rb.split();
        let producer = Arc::new(Mutex::new(producer));

        let mut active_stream: Option<Stream> = None;

        loop {
            match cmd_rx.recv() {
                Ok(AudioCommand::Start(reply_tx)) => {
                    samples.lock().clear();
                    *is_recording.lock() = false;

                    while consumer.try_pop().is_some() {}

                    let host = cpal::default_host();
                    let device = match host.default_input_device() {
                        Some(device) => device,
                        None => {
                            let error = "未检测到可用麦克风输入设备".to_string();
                            eprintln!("[AudioRecorder] {}", error);
                            Self::set_input_status(
                                &input_ready,
                                &last_error,
                                false,
                                Some(error.clone()),
                            );
                            let _ = reply_tx.send(Err(error));
                            continue;
                        }
                    };

                    let selected = match Self::choose_input_config(&device) {
                        Ok(selected) => selected,
                        Err(error) => {
                            eprintln!("[AudioRecorder] {}", error);
                            Self::set_input_status(
                                &input_ready,
                                &last_error,
                                false,
                                Some(error.clone()),
                            );
                            let _ = reply_tx.send(Err(error));
                            continue;
                        }
                    };

                    match Self::build_stream(&device, selected, producer.clone()) {
                        Ok(stream) => {
                            if let Err(error) = stream.play() {
                                let message = format!("启动麦克风输入失败: {}", error);
                                eprintln!("[AudioRecorder] {}", message);
                                *is_recording.lock() = false;
                                Self::set_input_status(
                                    &input_ready,
                                    &last_error,
                                    false,
                                    Some(message.clone()),
                                );
                                let _ = reply_tx.send(Err(message));
                            } else {
                                active_stream = Some(stream);
                                *is_recording.lock() = true;
                                Self::set_input_status(&input_ready, &last_error, true, None);
                                let _ = reply_tx.send(Ok(()));
                            }
                        }
                        Err(error) => {
                            eprintln!("[AudioRecorder] {}", error);
                            *is_recording.lock() = false;
                            Self::set_input_status(
                                &input_ready,
                                &last_error,
                                false,
                                Some(error.clone()),
                            );
                            let _ = reply_tx.send(Err(error));
                        }
                    }
                }
                Ok(AudioCommand::Stop(reply_tx)) => {
                    drop(active_stream.take());
                    *is_recording.lock() = false;

                    let mut collected = Vec::new();
                    while let Some(sample) = consumer.try_pop() {
                        collected.push(sample);
                    }
                    *samples.lock() = collected.clone();
                    Self::set_input_status(&input_ready, &last_error, true, None);
                    let _ = reply_tx.send(Ok(collected));
                }
                Ok(AudioCommand::Shutdown) | Err(_) => {
                    drop(active_stream.take());
                    break;
                }
            }
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(AudioCommand::Start(reply_tx))
            .map_err(|error| self.disconnected_error(error.to_string()))?;
        reply_rx
            .recv()
            .map_err(|error| self.disconnected_error(error.to_string()))?
    }

    pub fn stop(&self) -> Result<Vec<f32>, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(AudioCommand::Stop(reply_tx))
            .map_err(|error| self.disconnected_error(error.to_string()))?;
        reply_rx
            .recv()
            .map_err(|error| self.disconnected_error(error.to_string()))?
    }

    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock()
    }

    pub fn input_status(&self) -> (bool, Option<String>) {
        if self.is_recording() {
            return (true, None);
        }

        match Self::probe_input_device() {
            Ok(()) => {
                if self.last_error.lock().is_some() {
                    *self.last_error.lock() = None;
                }
                *self.input_ready.lock() = true;
                (true, None)
            }
            Err(error) => {
                Self::set_input_status(
                    &self.input_ready,
                    &self.last_error,
                    false,
                    Some(error.clone()),
                );
                (false, Some(error))
            }
        }
    }

    fn disconnected_error(&self, fallback: String) -> String {
        self.last_error
            .lock()
            .clone()
            .unwrap_or_else(|| format!("Whisper 录音线程不可用: {}", fallback))
    }
}

impl Drop for AudioRecorder {
    fn drop(&mut self) {
        let _ = self.command_tx.send(AudioCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

unsafe impl Send for AudioRecorder {}
unsafe impl Sync for AudioRecorder {}
