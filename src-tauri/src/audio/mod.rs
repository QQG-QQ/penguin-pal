mod model_manager;
mod recorder;
mod transcriber;
mod tts;
pub mod types;
// mod whisper; // 暂时禁用

pub use transcriber::TranscriberService;

use crate::app_state::{AudioProfile, AudioStage};

pub fn default_audio_profile() -> AudioProfile {
    AudioProfile {
        input_mode: recorder::input_mode().to_string(),
        output_mode: tts::output_mode().to_string(),
        stages: vec![
            recorder::stage(),
            // Whisper 暂时禁用
            AudioStage {
                id: "whisper".to_string(),
                title: "Whisper 语音识别".to_string(),
                summary: "本地 Whisper 语音识别（暂时禁用）".to_string(),
                status: "disabled".to_string(),
            },
            tts::stage(),
        ],
    }
}
