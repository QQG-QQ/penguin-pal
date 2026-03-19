use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    pub current_dir: PathBuf,
    pub workspace_root: Option<PathBuf>,
    pub markers: Vec<String>,
}

impl WorkspaceContext {
    pub fn default_workdir(&self) -> &Path {
        self.workspace_root
            .as_deref()
            .unwrap_or(self.current_dir.as_path())
    }

    pub fn render_for_prompt(&self) -> String {
        let marker_lines = if self.markers.is_empty() {
            "- 未检测到 .git / Cargo.toml / package.json 等工作区标记。".to_string()
        } else {
            self.markers
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let workspace_root = self
            .workspace_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| "未检测到".to_string());

        format!(
            "## 当前工作区\n- currentDir: {}\n- workspaceRoot: {}\n- defaultWorkdir: {}\n- markers:\n{}\n",
            self.current_dir.to_string_lossy(),
            workspace_root,
            self.default_workdir().to_string_lossy(),
            marker_lines
        )
    }
}

pub fn detect_workspace_context(preferred_root: Option<&str>) -> Option<WorkspaceContext> {
    let current_dir = env::current_dir().ok()?;
    let preferred_root = preferred_root
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists() && path.is_dir());
    let (workspace_root, markers) = if let Some(path) = preferred_root {
        let markers = collect_markers(&path);
        (Some(path), markers)
    } else {
        detect_workspace_root(&current_dir)
    };

    Some(WorkspaceContext {
        current_dir,
        workspace_root,
        markers,
    })
}

fn detect_workspace_root(start: &Path) -> (Option<PathBuf>, Vec<String>) {
    let mut current = Some(start);
    while let Some(path) = current {
        let markers = collect_markers(path);
        if !markers.is_empty() {
            return (Some(path.to_path_buf()), markers);
        }
        current = path.parent();
    }

    (None, Vec::new())
}

fn collect_markers(path: &Path) -> Vec<String> {
    let candidates = [
        ".git",
        "Cargo.toml",
        "package.json",
        "pnpm-workspace.yaml",
        "turbo.json",
        "tsconfig.json",
        "pyproject.toml",
    ];

    candidates
        .iter()
        .filter_map(|candidate| {
            let target = path.join(candidate);
            if target.exists() {
                Some((*candidate).to_string())
            } else {
                None
            }
        })
        .collect()
}
