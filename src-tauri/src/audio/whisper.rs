use crate::app_state::AudioStage;

pub fn stage() -> AudioStage {
    AudioStage {
        id: "transcribe".to_string(),
        title: "语音转写".to_string(),
        summary: "预留本地 Whisper 接口，当前默认由前端语音识别回填文字。".to_string(),
        status: "planned".to_string(),
    }
}
