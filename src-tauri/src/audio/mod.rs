mod model_manager;
mod recorder;
mod transcriber;
mod tts;
pub mod types;
mod whisper;

pub use transcriber::TranscriberService;

use crate::app_state::AudioProfile;

pub fn default_audio_profile() -> AudioProfile {
    AudioProfile {
        input_mode: recorder::input_mode().to_string(),
        output_mode: tts::output_mode().to_string(),
        stages: vec![recorder::stage(), whisper::stage(), tts::stage()],
    }
}
