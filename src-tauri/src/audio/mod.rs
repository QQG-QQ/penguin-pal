mod model_manager;
mod recorder;
mod transcriber;
mod tts;
pub mod types;
mod whisper;

pub use transcriber::TranscriberService;

use crate::app_state::{AudioProfile, AudioStage, VoiceInputMode};

fn input_stage(mode: VoiceInputMode, shortcut: &str) -> AudioStage {
    let (summary, status) = match mode {
        VoiceInputMode::Disabled => (
            "本地 Whisper 语音输入已关闭，仅保留文字输入。".to_string(),
            "disabled".to_string(),
        ),
        VoiceInputMode::Continuous => (
            "本地 Whisper 以短窗循环录音实现常驻监听，不依赖前台窗口。".to_string(),
            "continuous".to_string(),
        ),
        VoiceInputMode::PushToTalk => (
            format!("按住全局快捷键 {shortcut} 时开始录音，松开后本地 Whisper 转写。"),
            "pushToTalk".to_string(),
        ),
    };

    AudioStage {
        id: "voice_input_mode".to_string(),
        title: "语音输入策略".to_string(),
        summary,
        status,
    }
}

pub fn default_audio_profile(mode: VoiceInputMode, shortcut: &str) -> AudioProfile {
    let input_mode = match mode {
        VoiceInputMode::Disabled => "disabled",
        VoiceInputMode::Continuous => "whisper-continuous",
        VoiceInputMode::PushToTalk => "whisper-push-to-talk",
    };

    AudioProfile {
        input_mode: input_mode.to_string(),
        output_mode: tts::output_mode().to_string(),
        stages: vec![
            input_stage(mode, shortcut),
            recorder::stage(),
            whisper::stage(),
            tts::stage(),
        ],
    }
}
