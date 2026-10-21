use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub v: u32,
    pub id: String,
    pub host: String,
    pub pid: u32,
    pub pid_start: u64,
    pub tool: String,
    #[serde(default)]
    pub app: Option<String>,
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub invoker: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    pub state: String,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<String>,
    #[serde(default)]
    pub stage_note: Option<String>,
    #[serde(default)]
    pub progress: Option<Progress>,
    #[serde(default)]
    pub rate: Option<f64>,
    #[serde(default)]
    pub pgpu: Option<Pgpu>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub out: Option<String>,
    pub started: u64,
    pub updated: u64,
    #[serde(default)]
    pub ended: Option<u64>,
    #[serde(default)]
    pub exit: Option<i32>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub done: u64,
    #[serde(default)]
    pub total: Option<u64>,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pgpu {
    pub pid: u32,
    #[serde(rename = "as")]
    pub as_name: String,
}
