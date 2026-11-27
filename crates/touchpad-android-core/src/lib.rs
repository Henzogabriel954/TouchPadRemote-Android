//! Android Rust core engine (bidirectional UDP, MultiPcSession, JNI bridge).

pub mod discovery;
pub mod engine;
pub mod error;
pub mod events;
pub mod graph;
pub mod jni_bridge;
pub mod models;
pub mod session;

pub use engine::AndroidEngine;
pub use error::AndroidEngineError;
pub use models::{
    ActiveSessionSnapshot, AndroidSettingsDto, AndroidSettingsPatch, ComputerInput,
    DiscoveredComputer, EngineTimings, HostInfoSnapshot, SessionPeerSnapshot, UiEvent,
    UiStateSnapshot,
};
pub use touchpad_protocol::{MouseButton, PairingStatus, ScreenEdge};
pub use touchpad_storage::{EdgeLink, SavedComputer};
