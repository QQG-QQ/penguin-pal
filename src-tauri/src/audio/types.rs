use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperModel {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

impl WhisperModel {
    pub fn file_name(&self) -> &'static str {
        match self {
            Self::Tiny => "ggml-tiny.bin",
            Self::Base => "ggml-base.bin",
            Self::Small => "ggml-small.bin",
            Self::Medium => "ggml-medium.bin",
            Self::Large => "ggml-large.bin",
        }
    }

    pub fn download_url(&self) -> &'static str {
        match self {
            Self::Tiny => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
            Self::Base => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
            Self::Small => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
            Self::Medium => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
            Self::Large => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large.bin",
        }
    }

    pub fn size_bytes(&self) -> u64 {
        match self {
            Self::Tiny => 75_000_000,    // ~75MB
            Self::Base => 142_000_000,   // ~142MB
            Self::Small => 466_000_000,  // ~466MB
            Self::Medium => 1_500_000_000, // ~1.5GB
            Self::Large => 2_900_000_000,  // ~2.9GB
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Tiny => "Tiny (75MB)",
            Self::Base => "Base (142MB)",
            Self::Small => "Small (466MB)",
            Self::Medium => "Medium (1.5GB)",
            Self::Large => "Large (2.9GB)",
        }
    }

    pub fn all() -> Vec<WhisperModel> {
        vec![Self::Tiny, Self::Base, Self::Small, Self::Medium, Self::Large]
    }
}

impl Default for WhisperModel {
    fn default() -> Self {
        Self::Base
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperStatus {
    pub model_loaded: bool,
    pub current_model: Option<WhisperModel>,
    pub available_models: Vec<ModelInfo>,
    pub recording_state: RecordingState,
    pub input_ready: bool,
    pub input_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub model: WhisperModel,
    pub label: String,
    pub size_bytes: u64,
    pub downloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model: WhisperModel,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f64,
}
