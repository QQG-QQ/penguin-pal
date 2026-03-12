use serde::{Deserialize, Serialize};

pub const VISION_SCHEMA_VERSION: &str = "vision-summary.v1";
pub const VISION_CACHE_TTL_MS: u64 = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VisionProviderStatusKind {
    Supported,
    Unknown,
    Unsupported,
    Timeout,
    DisabledOffline,
    AnalysisFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionProviderStatus {
    pub kind: VisionProviderStatusKind,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionRegionSummary {
    pub region_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualElementSummary {
    pub role: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub location_hint: Option<String>,
    pub is_interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionWindowSummary {
    pub schema_version: String,
    pub window_kind: String,
    #[serde(default)]
    pub page_kind: Option<String>,
    #[serde(default)]
    pub certainty: Option<String>,
    #[serde(default)]
    pub primary_regions: Vec<VisionRegionSummary>,
    #[serde(default)]
    pub key_elements: Vec<VisualElementSummary>,
    pub has_obvious_interactive_target: bool,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub uia_consistency_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScreenContextConsistencyKind {
    Consistent,
    UiaOnly,
    VisionOnly,
    SoftConflict,
    HardConflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenContextConsistency {
    pub status: ScreenContextConsistencyKind,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionCaptureInfo {
    pub image_path: String,
    pub width: i64,
    pub height: i64,
    pub window_title: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionContext {
    pub provider_status: VisionProviderStatus,
    pub cache_hit: bool,
    #[serde(default)]
    pub capture: Option<VisionCaptureInfo>,
    #[serde(default)]
    pub summary: Option<VisionWindowSummary>,
}

#[derive(Debug, Clone)]
pub struct CachedVisionContext {
    pub window_title: String,
    pub window_class_name: Option<String>,
    pub created_at: u64,
    pub context: VisionContext,
}
