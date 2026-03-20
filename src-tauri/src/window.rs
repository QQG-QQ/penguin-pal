use tauri::{PhysicalPosition, Runtime, WebviewWindow};

use crate::app_state::SavedWindowPosition;

pub fn setup_window<R: Runtime>(
    window: &WebviewWindow<R>,
    saved_position: Option<&SavedWindowPosition>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_decorations(false);
    }

    window.set_always_on_top(true)?;
    let _ = window.set_title("PenguinPal");
    if let Some(position) = saved_position {
        let _ = window.set_position(PhysicalPosition::new(position.x, position.y));
    }

    Ok(())
}
