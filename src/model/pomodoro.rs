use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Work,
    ShortBreak,
    LongBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PomoConfig {
    pub work_mins: u32,
    pub short_break_mins: u32,
    pub long_break_mins: u32,
}

impl Default for PomoConfig {
    fn default() -> Self {
        Self {
            work_mins: 25,
            short_break_mins: 5,
            long_break_mins: 15,
        }
    }
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
    #[serde(default)]
    pub today_count: u32,
    #[serde(default)]
    pub streak: u32,
    #[serde(default)]
    pub last_completed_task_title: Option<String>,
    #[serde(default)]
    pub config: PomoConfig,
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
            today_count: 0,
            streak: 0,
            last_completed_task_title: None,
            config: PomoConfig::default(),
        }
    }
}
