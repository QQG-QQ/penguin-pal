// 录音功能暂时禁用的存根实现

use crate::app_state::AudioStage;

pub fn input_mode() -> &'static str {
    "disabled"
}

pub fn stage() -> AudioStage {
    AudioStage {
        id: "recorder".to_string(),
        title: "本地麦克风采集".to_string(),
        summary: "麦克风采集功能暂时禁用".to_string(),
        status: "disabled".to_string(),
    }
}
