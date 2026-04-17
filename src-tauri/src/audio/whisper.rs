use crate::app_state::AudioStage;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::types::{TranscriptionResult, WhisperModel};

pub fn stage() -> AudioStage {
    AudioStage {
        id: "transcribe".to_string(),
        title: "Whisper 璇煶杞啓".to_string(),
        summary: "浣跨敤鏈湴 Whisper 妯″瀷杩涜璇煶璇嗗埆銆?".to_string(),
        status: "ready".to_string(),
    }
}

enum WhisperCommand {
    Load {
        path: PathBuf,
        model: WhisperModel,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Unload,
    Transcribe {
        samples: Vec<f32>,
        reply: mpsc::Sender<Result<TranscriptionResult, String>>,
    },
    Shutdown,
}

pub struct WhisperEngine {
    command_tx: mpsc::Sender<WhisperCommand>,
    current_model: Arc<Mutex<Option<WhisperModel>>>,
    worker_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl WhisperEngine {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel::<WhisperCommand>();
        let current_model = Arc::new(Mutex::new(None));
        let current_model_clone = current_model.clone();

        let handle = thread::spawn(move || Self::worker_loop(command_rx, current_model_clone));

        Self {
            command_tx,
            current_model,
            worker_handle: Mutex::new(Some(handle)),
        }
    }

    pub fn load_model(&self, path: &Path, model: WhisperModel) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("妯″瀷鏂囦欢涓嶅瓨鍦? {:?}", path));
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(WhisperCommand::Load {
                path: path.to_path_buf(),
                model,
                reply: reply_tx,
            })
            .map_err(|error| format!("Whisper worker 涓嶅彲鐢? {}", error))?;

        reply_rx
            .recv()
            .map_err(|error| format!("Whisper worker 鍥炲簲澶辫触: {}", error))?
    }

    pub fn unload_model(&self) {
        let _ = self.command_tx.send(WhisperCommand::Unload);
    }

    pub fn is_loaded(&self) -> bool {
        self.current_model.lock().is_some()
    }

    pub fn current_model(&self) -> Option<WhisperModel> {
        *self.current_model.lock()
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<TranscriptionResult, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(WhisperCommand::Transcribe {
                samples: samples.to_vec(),
                reply: reply_tx,
            })
            .map_err(|error| format!("Whisper worker 涓嶅彲鐢? {}", error))?;

        reply_rx
            .recv()
            .map_err(|error| format!("Whisper worker 鍥炲簲澶辫触: {}", error))?
    }

    fn worker_loop(
        command_rx: mpsc::Receiver<WhisperCommand>,
        current_model: Arc<Mutex<Option<WhisperModel>>>,
    ) {
        let mut context: Option<WhisperContext> = None;

        loop {
            match command_rx.recv() {
                Ok(WhisperCommand::Load { path, model, reply }) => {
                    let result = Self::create_context(&path).map(|next_context| {
                        context = Some(next_context);
                        *current_model.lock() = Some(model);
                    });
                    let _ = reply.send(result);
                }
                Ok(WhisperCommand::Unload) => {
                    context = None;
                    *current_model.lock() = None;
                }
                Ok(WhisperCommand::Transcribe { samples, reply }) => {
                    let result = match context.as_ref() {
                        Some(context) => Self::transcribe_with_context(context, &samples),
                        None => Err("妯″瀷鏈姞杞?".to_string()),
                    };
                    let _ = reply.send(result);
                }
                Ok(WhisperCommand::Shutdown) | Err(_) => {
                    *current_model.lock() = None;
                    break;
                }
            }
        }
    }

    fn create_context(path: &Path) -> Result<WhisperContext, String> {
        let path_str = path.to_str().ok_or_else(|| "鏃犳晥鐨勮矾寰?".to_string())?;

        let params = WhisperContextParameters::default();
        WhisperContext::new_with_params(path_str, params)
            .map_err(|error| format!("鍔犺浇妯″瀷澶辫触: {}", error))
    }

    fn transcribe_with_context(
        context: &WhisperContext,
        samples: &[f32],
    ) -> Result<TranscriptionResult, String> {
        let start = Instant::now();

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(None);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(false);

        let mut state = context
            .create_state()
            .map_err(|error| format!("鍒涘缓鐘舵€佸け璐? {}", error))?;

        state
            .full(params, samples)
            .map_err(|error| format!("鎺ㄧ悊澶辫触: {}", error))?;

        let num_segments = state
            .full_n_segments()
            .map_err(|error| format!("鑾峰彇娈垫暟澶辫触: {}", error))?;

        let mut text = String::new();
        for index in 0..num_segments {
            if let Ok(segment_text) = state.full_get_segment_text(index) {
                text.push_str(&segment_text);
            }
        }

        Ok(TranscriptionResult {
            text: text.trim().to_string(),
            language: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

impl Default for WhisperEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        let _ = self.command_tx.send(WhisperCommand::Shutdown);
        if let Some(handle) = self.worker_handle.lock().take() {
            let _ = handle.join();
        }
    }
}
