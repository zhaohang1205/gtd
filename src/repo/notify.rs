use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 心智维护提醒的去重状态：记录已经发送过的 `类型:日期` key。
/// 每日聚合摘要（digest）按本地日期去重，确保同一天至多提醒一次。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotifyState {
    #[serde(default)]
    pub sent: Vec<String>,
}

pub fn notify_file_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("gtp");
    let _ = fs::create_dir_all(&path);
    path.push("notify.json");
    path
}

pub fn get_state() -> Result<NotifyState> {
    let path = notify_file_path();
    if !path.exists() {
        return Ok(NotifyState::default());
    }
    let content = fs::read_to_string(&path)?;
    let state = serde_json::from_str(&content)?;
    Ok(state)
}

pub fn save_state(state: &NotifyState) -> Result<()> {
    let path = notify_file_path();
    let content = serde_json::to_string_pretty(state)?;
    // 先写临时文件再 rename，避免并发读方读到半写的文件
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}
