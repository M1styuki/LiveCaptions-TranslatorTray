use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol_version: u32,
    pub request_id: u64,
    #[serde(flatten)]
    pub message: Message,
}

impl Envelope {
    pub fn new(request_id: u64, message: Message) -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION,
            request_id,
            message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum Message {
    StartEngine(WindowState),
    StopEngine(Empty),
    ShowSettings(WindowState),
    ShowMain(WindowState),
    Exit(Empty),
    Ready(Ready),
    Ack(Empty),
    EngineStateChanged {
        state: EngineState,
    },
    WindowsChanged {
        visible_windows: u32,
        live_captions_hidden: bool,
    },
    Error {
        code: String,
        message: String,
    },
    Exiting(Empty),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ready {
    pub worker_pid: u32,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub live_captions_hidden: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            live_captions_hidden: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineState {
    Disabled,
    Starting,
    Running,
    Stopping,
    Faulted,
}
