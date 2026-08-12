use crate::model::pomodoro::PomoState;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// 测试隔离：置为 true 时 `get_state()` 恒返回 Idle，
/// 避免 TUI 渲染测试读到真实 `~/.config/gtp/pomo.json` 里的运行中 daemon。
static POMO_IDLE_OVERRIDE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub fn set_pomo_idle_for_tests() {
    POMO_IDLE_OVERRIDE.store(true, Ordering::Relaxed);
}

pub fn pomo_file_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("gtp");
    let _ = fs::create_dir_all(&path);
    path.push("pomo.json");
    path
}

pub fn get_state() -> Result<PomoState> {
    if POMO_IDLE_OVERRIDE.load(Ordering::Relaxed) {
        return Ok(PomoState::default());
    }
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
