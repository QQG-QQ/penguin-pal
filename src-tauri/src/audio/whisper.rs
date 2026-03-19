use crate::app_state::AudioStage;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::types::{TranscriptionResult, WhisperModel};

pub fn stage() -> AudioStage {
    AudioStage {
        id: "transcribe".to_string(),
        title: "Whisper 语音转写".to_string(),
        summary: "使用本地 Whisper 模型进行语音识别。".to_string(),
        status: "ready".to_string(),
    }
}

pub struct WhisperEngine {
    ctx: Arc<Mutex<Option<WhisperContext>>>,
    current_model: Arc<Mutex<Option<WhisperModel>>>,
}

impl WhisperEngine {
    pub fn new() -> Self {
        Self {
            ctx: Arc::new(Mutex::new(None)),
            current_model: Arc::new(Mutex::new(None)),
        }
    }

    pub fn load_model(&self, path: &Path, model: WhisperModel) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("模型文件不存在: {:?}", path));
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| "无效的路径".to_string())?;

        let params = WhisperContextParameters::default();
        let context = WhisperContext::new_with_params(path_str, params)
            .map_err(|e| format!("加载模型失败: {}", e))?;

        *self.ctx.lock() = Some(context);
        *self.current_model.lock() = Some(model);

        Ok(())
    }

    pub fn unload_model(&self) {
        *self.ctx.lock() = None;
        *self.current_model.lock() = None;
    }

    pub fn is_loaded(&self) -> bool {
        self.ctx.lock().is_some()
    }

    pub fn current_model(&self) -> Option<WhisperModel> {
        *self.current_model.lock()
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<TranscriptionResult, String> {
        let ctx_guard = self.ctx.lock();
        let ctx = ctx_guard
            .as_ref()
            .ok_or_else(|| "模型未加载".to_string())?;

        let start = Instant::now();

        // 配置参数
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(None);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(false);

        // 创建状态并执行推理
        let mut state = ctx
            .create_state()
            .map_err(|e| format!("创建状态失败: {}", e))?;

        state
            .full(params, samples)
            .map_err(|e| format!("推理失败: {}", e))?;

        // 收集结果
        let num_segments = state
            .full_n_segments()
            .map_err(|e| format!("获取段数失败: {}", e))?;

        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(segment_text) = state.full_get_segment_text(i) {
                text.push_str(&segment_text);
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text: text.trim().to_string(),
            language: None,
            duration_ms,
        })
    }
}

impl Default for WhisperEngine {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for WhisperEngine {}
unsafe impl Sync for WhisperEngine {}
