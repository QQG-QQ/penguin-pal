// Whisper 功能暂时禁用的存根实现

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Channel;

use super::types::{DownloadProgress, ModelInfo, RecordingState, TranscriptionResult, WhisperModel, WhisperStatus};

pub struct TranscriberService {
    state: Arc<Mutex<RecordingState>>,
}

impl TranscriberService {
    pub fn new(_app_data_dir: PathBuf) -> Result<Self, String> {
        Ok(Self {
            state: Arc::new(Mutex::new(RecordingState::Idle)),
        })
    }

    pub fn get_status(&self) -> WhisperStatus {
        WhisperStatus {
            model_loaded: false,
            current_model: None,
            available_models: vec![],
            recording_state: *self.state.lock(),
        }
    }

    pub fn get_available_models(&self) -> Vec<ModelInfo> {
        vec![]
    }

    pub fn get_recording_state(&self) -> RecordingState {
        *self.state.lock()
    }

    pub fn load_model(&self, _model: WhisperModel) -> Result<(), String> {
        Err("Whisper 功能暂时禁用".to_string())
    }

    pub fn unload_model(&self) {
        // no-op
    }

    pub async fn download_model(
        &self,
        _model: WhisperModel,
        _progress_channel: Channel<DownloadProgress>,
    ) -> Result<PathBuf, String> {
        Err("Whisper 功能暂时禁用".to_string())
    }

    pub fn delete_model(&self, _model: WhisperModel) -> Result<(), String> {
        Err("Whisper 功能暂时禁用".to_string())
    }

    pub fn start_recording(&self) -> Result<(), String> {
        Err("Whisper 功能暂时禁用".to_string())
    }

    pub fn stop_recording(&self) -> Result<TranscriptionResult, String> {
        Err("Whisper 功能暂时禁用".to_string())
    }

    #[allow(dead_code)]
    pub fn is_model_downloaded(&self, _model: WhisperModel) -> bool {
        false
    }
}

unsafe impl Send for TranscriberService {}
unsafe impl Sync for TranscriberService {}
