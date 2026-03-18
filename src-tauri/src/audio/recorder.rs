use crate::app_state::AudioStage;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use ringbuf::{traits::{Consumer, Producer, Split}, HeapRb};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

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
    Start,
    Stop,
    Shutdown,
}

pub struct AudioRecorder {
    command_tx: mpsc::Sender<AudioCommand>,
    samples: Arc<Mutex<Vec<f32>>>,
    #[allow(dead_code)]
    is_recording: Arc<Mutex<bool>>,
    worker_handle: Option<thread::JoinHandle<()>>,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AudioCommand>();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let is_recording = Arc::new(Mutex::new(false));

        let samples_clone = samples.clone();
        let is_recording_clone = is_recording.clone();

        let handle = thread::spawn(move || {
            Self::worker_loop(cmd_rx, samples_clone, is_recording_clone);
        });

        Ok(Self {
            command_tx: cmd_tx,
            samples,
            is_recording,
            worker_handle: Some(handle),
        })
    }

    fn worker_loop(
        cmd_rx: mpsc::Receiver<AudioCommand>,
        samples: Arc<Mutex<Vec<f32>>>,
        is_recording: Arc<Mutex<bool>>,
    ) {
        let host = cpal::default_host();
        let device = match host.default_input_device() {
            Some(d) => d,
            None => {
                eprintln!("[AudioRecorder] No input device available");
                return;
            }
        };

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(16000),
            buffer_size: cpal::BufferSize::Default,
        };

        // 环形缓冲区: 30秒 * 16000 采样/秒
        let rb = HeapRb::<f32>::new(16000 * 30);
        let (producer, mut consumer) = rb.split();

        // 使用 Arc<Mutex> 包装 producer，使其可以在多次迭代中共享
        let producer = Arc::new(Mutex::new(producer));

        // _stream 变量必须保持存活，否则录音会停止
        let mut _stream: Option<cpal::Stream> = None;

        loop {
            match cmd_rx.recv() {
                Ok(AudioCommand::Start) => {
                    // 清空之前的采样
                    samples.lock().clear();
                    *is_recording.lock() = true;

                    // 清空 consumer 缓冲区
                    while consumer.try_pop().is_some() {}

                    let err_fn = |err| eprintln!("[AudioRecorder] Stream error: {}", err);

                    let producer_clone = producer.clone();
                    let stream_result = device.build_input_stream(
                        &config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mut prod = producer_clone.lock();
                            for &sample in data {
                                let _ = prod.try_push(sample);
                            }
                        },
                        err_fn,
                        None,
                    );

                    match stream_result {
                        Ok(s) => {
                            if let Err(e) = s.play() {
                                eprintln!("[AudioRecorder] Failed to play stream: {}", e);
                            }
                            _stream = Some(s);
                        }
                        Err(e) => {
                            eprintln!("[AudioRecorder] Failed to build stream: {}", e);
                            *is_recording.lock() = false;
                        }
                    }
                }
                Ok(AudioCommand::Stop) => {
                    // 停止录音，收集采样
                    _stream = None;
                    *is_recording.lock() = false;

                    // 从 consumer 收集所有采样
                    let mut collected = Vec::new();
                    while let Some(sample) = consumer.try_pop() {
                        collected.push(sample);
                    }
                    *samples.lock() = collected;
                }
                Ok(AudioCommand::Shutdown) => {
                    _stream = None;
                    break;
                }
                Err(_) => {
                    // Channel closed
                    break;
                }
            }
        }
    }

    pub fn start(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Start)
            .map_err(|e| e.to_string())
    }

    pub fn stop(&self) -> Result<Vec<f32>, String> {
        self.command_tx
            .send(AudioCommand::Stop)
            .map_err(|e| e.to_string())?;

        // 等待一小段时间让 worker 收集采样
        thread::sleep(std::time::Duration::from_millis(100));

        let samples = self.samples.lock().clone();
        Ok(samples)
    }

    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock()
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
