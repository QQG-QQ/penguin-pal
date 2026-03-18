use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Channel;

use super::model_manager::ModelManager;
use super::recorder::AudioRecorder;
use super::types::{DownloadProgress, ModelInfo, RecordingState, TranscriptionResult, WhisperModel, WhisperStatus};
use super::whisper::WhisperEngine;

pub struct TranscriberService {
    recorder: AudioRecorder,
    engine: WhisperEngine,
    model_manager: ModelManager,
    state: Arc<Mutex<RecordingState>>,
}

impl TranscriberService {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, String> {
        let recorder = AudioRecorder::new()?;
        let engine = WhisperEngine::new();
        let model_manager = ModelManager::new(app_data_dir);

        Ok(Self {
            recorder,
            engine,
            model_manager,
            state: Arc::new(Mutex::new(RecordingState::Idle)),
        })
    }

    pub fn get_status(&self) -> WhisperStatus {
        WhisperStatus {
            model_loaded: self.engine.is_loaded(),
            current_model: self.engine.current_model(),
            available_models: self.model_manager.get_available_models(),
            recording_state: *self.state.lock(),
        }
    }

    pub fn get_available_models(&self) -> Vec<ModelInfo> {
        self.model_manager.get_available_models()
    }

    pub fn get_recording_state(&self) -> RecordingState {
        *self.state.lock()
    }

    pub fn load_model(&self, model: WhisperModel) -> Result<(), String> {
        let path = self.model_manager.model_path(model);
        if !path.exists() {
            return Err(format!("模型 {} 未下载", model.label()));
        }
        self.engine.load_model(&path, model)
    }

    pub fn unload_model(&self) {
        self.engine.unload_model();
    }

    pub async fn download_model(
        &self,
        model: WhisperModel,
        progress_channel: Channel<DownloadProgress>,
    ) -> Result<PathBuf, String> {
        self.model_manager.download_model(model, progress_channel).await
    }

    pub fn delete_model(&self, model: WhisperModel) -> Result<(), String> {
        // 如果当前加载的是要删除的模型，先卸载
        if self.engine.current_model() == Some(model) {
            self.engine.unload_model();
        }
        self.model_manager.delete_model(model)
    }

    pub fn start_recording(&self) -> Result<(), String> {
        if !self.engine.is_loaded() {
            return Err("请先加载 Whisper 模型".to_string());
        }

        let current_state = *self.state.lock();
        if current_state != RecordingState::Idle {
            return Err("已经在录音中".to_string());
        }

        *self.state.lock() = RecordingState::Recording;
        self.recorder.start()
    }

    pub fn stop_recording(&self) -> Result<TranscriptionResult, String> {
        let current_state = *self.state.lock();
        if current_state != RecordingState::Recording {
            return Err("未在录音状态".to_string());
        }

        *self.state.lock() = RecordingState::Processing;

        // 停止录音并获取采样
        let samples = self.recorder.stop()?;

        if samples.is_empty() {
            *self.state.lock() = RecordingState::Idle;
            return Err("未采集到音频数据".to_string());
        }

        // 执行转写
        let result = self.engine.transcribe(&samples);

        *self.state.lock() = RecordingState::Idle;

        result
    }

    #[allow(dead_code)]
    pub fn is_model_downloaded(&self, model: WhisperModel) -> bool {
        self.model_manager.is_downloaded(model)
    }
}

unsafe impl Send for TranscriberService {}
unsafe impl Sync for TranscriberService {}
