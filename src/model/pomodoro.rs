use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Work,
    ShortBreak,
    LongBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PomoState {
    pub phase: Phase,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub cycle: u32,
    pub total_count: u32,
}

impl Default for PomoState {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            task_id: None,
            task_title: None,
            start_ts: None,
            end_ts: None,
            cycle: 0,
            total_count: 0,
        }
    }
}
