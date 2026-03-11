use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io::Write,
};
use tauri::{AppHandle, Manager};

pub fn append_log(
    app: &AppHandle,
    tool: &str,
    outcome: &str,
    detail: impl Into<String>,
) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("control")
        .join("logs");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let file_path = dir.join(format!(
        "control-{}.log",
        Local::now().format("%Y-%m-%d")
    ));

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .map_err(|error| error.to_string())?;

    let line = format!(
        "{} tool={} outcome={} detail={}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        tool,
        outcome,
        sanitize(detail.into())
    );
    file.write_all(line.as_bytes())
        .map_err(|error| error.to_string())
}

fn sanitize(value: String) -> String {
    value.replace('\n', "\\n").replace('\r', "\\r")
}
