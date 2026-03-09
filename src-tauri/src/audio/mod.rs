mod recorder;
mod tts;
mod whisper;

use crate::app_state::AudioProfile;

pub fn default_audio_profile() -> AudioProfile {
    AudioProfile {
        input_mode: recorder::input_mode().to_string(),
        output_mode: tts::output_mode().to_string(),
        stages: vec![recorder::stage(), whisper::stage(), tts::stage()],
    }
}
