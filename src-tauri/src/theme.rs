use shared::{get_accent as lw_get_accent, get_theme as lw_get_theme};
use tauri::command;

#[command]
pub fn get_theme() -> Result<String, String> {
    Ok(lw_get_theme())
}

#[command]
pub fn get_accent() -> Result<String, String> {
    Ok(lw_get_accent())
}
