use std::fs;
use std::path::PathBuf;
use anyhow::Result;
use crate::model::pomodoro::PomoState;

pub fn pomo_file_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("gtp");
    fs::create_dir_all(&path).ok();
    path.push("pomo.json");
    path
}

pub fn get_state() -> Result<PomoState> {
    let path = pomo_file_path();
    if !path.exists() {
        return Ok(PomoState::default());
    }
    let content = fs::read_to_string(&path)?;
    let state = serde_json::from_str(&content).unwrap_or_else(|_| PomoState::default());
    Ok(state)
}

pub fn save_state(state: &PomoState) -> Result<()> {
    let path = pomo_file_path();
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&path, content)?;
    Ok(())
}
