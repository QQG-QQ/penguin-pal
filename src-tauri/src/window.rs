use tauri::{Runtime, WebviewWindow};

pub fn setup_window<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_decorations(false);
    }

    window.set_always_on_top(true)?;
    let _ = window.set_title("PenguinPal");

    Ok(())
}
