use futures_util::StreamExt;
use parking_lot::Mutex;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Channel;

use super::types::{DownloadProgress, ModelInfo, WhisperModel};

pub struct ModelManager {
    models_dir: PathBuf,
    downloading: Arc<Mutex<Option<WhisperModel>>>,
}

impl ModelManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let models_dir = app_data_dir.join("whisper-models");
        if !models_dir.exists() {
            let _ = fs::create_dir_all(&models_dir);
        }

        Self {
            models_dir,
            downloading: Arc::new(Mutex::new(None)),
        }
    }

    #[allow(dead_code)]
    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    pub fn model_path(&self, model: WhisperModel) -> PathBuf {
        self.models_dir.join(model.file_name())
    }

    pub fn is_downloaded(&self, model: WhisperModel) -> bool {
        self.model_path(model).exists()
    }

    pub fn get_available_models(&self) -> Vec<ModelInfo> {
        WhisperModel::all()
            .into_iter()
            .map(|model| ModelInfo {
                model,
                label: model.label().to_string(),
                size_bytes: model.size_bytes(),
                downloaded: self.is_downloaded(model),
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn is_downloading(&self) -> bool {
        self.downloading.lock().is_some()
    }

    #[allow(dead_code)]
    pub fn current_download(&self) -> Option<WhisperModel> {
        *self.downloading.lock()
    }

    pub async fn download_model(
        &self,
        model: WhisperModel,
        progress_channel: Channel<DownloadProgress>,
    ) -> Result<PathBuf, String> {
        // 检查是否已经在下载
        {
            let mut downloading = self.downloading.lock();
            if downloading.is_some() {
                return Err("另一个模型正在下载中".to_string());
            }
            *downloading = Some(model);
        }

        let result = self.do_download(model, progress_channel).await;

        // 清除下载状态
        *self.downloading.lock() = None;

        result
    }

    async fn do_download(
        &self,
        model: WhisperModel,
        progress_channel: Channel<DownloadProgress>,
    ) -> Result<PathBuf, String> {
        let url = model.download_url();
        let path = self.model_path(model);
        let temp_path = path.with_extension("bin.tmp");
        let result = async {
            let client = reqwest::Client::new();
            let response = client
                .get(url)
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;

            if !response.status().is_success() {
                return Err(format!("HTTP 错误: {}", response.status()));
            }

            let declared_size = response.content_length();
            let total_size = declared_size.unwrap_or(model.size_bytes());

            let mut file =
                fs::File::create(&temp_path).map_err(|e| format!("创建文件失败: {}", e))?;

            let mut downloaded: u64 = 0;
            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| format!("下载失败: {}", e))?;
                use std::io::Write;
                file.write_all(&chunk)
                    .map_err(|e| format!("写入失败: {}", e))?;

                downloaded += chunk.len() as u64;

                let progress = DownloadProgress {
                    model,
                    downloaded_bytes: downloaded,
                    total_bytes: total_size,
                    progress_percent: (downloaded as f64 / total_size as f64) * 100.0,
                };

                let _ = progress_channel.send(progress);
            }

            use std::io::Write;
            file.flush().map_err(|e| format!("刷新模型文件失败: {}", e))?;

            if downloaded == 0 {
                return Err("下载完成但模型文件为空".to_string());
            }

            if let Some(expected_size) = declared_size {
                if downloaded != expected_size {
                    return Err(format!(
                        "模型下载不完整：期望 {} 字节，实际 {} 字节",
                        expected_size, downloaded
                    ));
                }
            }

            fs::rename(&temp_path, &path).map_err(|e| format!("重命名文件失败: {}", e))?;
            Ok(path.clone())
        }
        .await;

        if result.is_err() && temp_path.exists() {
            let _ = fs::remove_file(&temp_path);
        }

        result
    }

    pub fn delete_model(&self, model: WhisperModel) -> Result<(), String> {
        let path = self.model_path(model);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("删除模型失败: {}", e))?;
        }
        Ok(())
    }
}

unsafe impl Send for ModelManager {}
unsafe impl Sync for ModelManager {}
