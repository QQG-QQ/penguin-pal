use crate::app_state::AudioStage;

pub fn output_mode() -> &'static str {
    "speech-synthesis"
}

pub fn stage() -> AudioStage {
    AudioStage {
        id: "tts".to_string(),
        title: "语音播报".to_string(),
        summary: "当前默认使用系统语音播报；后续可替换为本地 Piper 或自定义 TTS。"
            .to_string(),
        status: "ready".to_string(),
    }
}
