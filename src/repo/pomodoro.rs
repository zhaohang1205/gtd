use crate::model::pomodoro::PomoState;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub fn pomo_file_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("gtp");
    let _ = fs::create_dir_all(&path);
    path.push("pomo.json");
    path
}

pub fn get_state() -> Result<PomoState> {
    let path = pomo_file_path();
    if !path.exists() {
        return Ok(PomoState::default());
    }
    let content = fs::read_to_string(&path)?;
    let state = serde_json::from_str(&content)?;
    Ok(state)
}

pub fn save_state(state: &PomoState) -> Result<()> {
    let path = pomo_file_path();
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&path, content)?;
    Ok(())
}
