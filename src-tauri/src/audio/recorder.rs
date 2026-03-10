use crate::app_state::AudioStage;

pub fn input_mode() -> &'static str {
    "auto-listen"
}

pub fn stage() -> AudioStage {
    AudioStage {
        id: "recorder".to_string(),
        title: "自动语音监听".to_string(),
        summary: "当前版本检测到麦克风后会优先使用 Web Speech 自动监听，后续可以切换为本地采集管线。"
            .to_string(),
        status: "ready".to_string(),
    }
}
