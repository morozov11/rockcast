//! Device-control v1 frame bounds, player facts, and wire JSON.
//! Protocol parsing is bounded; command execution remains outside DC-012.

use super::transport::ControlSocket;
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub(super) const MAX_FRAME_BYTES: usize = 65_536;
const MAX_PAYLOAD_BYTES: usize = 61_440;
pub(super) const HEARTBEAT: Duration = Duration::from_secs(20);
pub(super) const POLL: Duration = Duration::from_millis(200);
pub(super) const NO_IDENTITY_DELAY: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerState {
    pub(crate) playback_status: &'static str,
    pub(crate) station_id: Option<String>,
    pub(crate) volume: u8,
}

impl PlayerState {
    pub(crate) fn idle(volume: u8) -> Self {
        Self {
            playback_status: "idle",
            station_id: None,
            volume,
        }
    }

    pub(super) fn runtime_state(&self) -> Value {
        json!({
            "playback": { "status": self.playback_status, "station_id": self.station_id },
            // RockCast has no mute operation today; false is a factual local state,
            // not an advertised remote mute command.
            "volume": { "level": self.volume, "muted": false }
        })
    }
}

#[derive(Clone)]
pub(super) struct PublishedState {
    pub(super) revision: u64,
    pub(super) observed_at: String,
    pub(super) state: PlayerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlError {
    Unavailable,
    Authentication,
    Protocol,
}

pub(super) enum Inbound {
    Text(String),
    Ping(Vec<u8>),
    Close,
}

pub(super) fn send(
    socket: &mut dyn ControlSocket,
    kind: &str,
    payload: Value,
) -> Result<(), ControlError> {
    let payload_size = serde_json::to_vec(&payload)
        .map_err(|_| ControlError::Protocol)?
        .len();
    if payload_size > MAX_PAYLOAD_BYTES {
        return Err(ControlError::Protocol);
    }
    let frame = json!({
        "protocol_version": 1,
        "message_id": Uuid::new_v4(),
        "type": kind,
        "sent_at": timestamp(),
        "payload": payload,
    });
    let frame = serde_json::to_string(&frame).map_err(|_| ControlError::Protocol)?;
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ControlError::Protocol);
    }
    socket.send_text(frame)
}

pub(super) fn inbound_type(frame: &str) -> Result<Option<String>, ControlError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ControlError::Protocol);
    }
    let value: Value = serde_json::from_str(frame).map_err(|_| ControlError::Protocol)?;
    if value.get("protocol_version").and_then(Value::as_i64) != Some(1)
        || !value.get("payload").is_some_and(Value::is_object)
    {
        return Err(ControlError::Protocol);
    }
    Ok(value.get("type").and_then(Value::as_str).map(str::to_owned))
}

pub(super) fn is_auth_error(frame: &str) -> bool {
    serde_json::from_str::<Value>(frame)
        .ok()
        .and_then(|value| {
            value
                .pointer("/payload/error/code")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|code| matches!(code.as_str(), "authentication_required" | "forbidden"))
}

pub(super) fn control_endpoint(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let ws = base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{ws}/api/v1/devices/connect")
}

pub(super) fn manifest() -> Value {
    json!({
        "manifest_revision": 1,
        "roles": ["player"],
        "capabilities": {
            "revision": 1,
            "items": [
                { "name": "media.playback", "version": 1, "actions": ["play", "stop", "next", "previous"] },
                { "name": "media.station", "version": 1, "sources": ["rockserver_catalog"] },
                { "name": "media.volume", "version": 1, "minimum": 0, "maximum": 100, "step": 1, "mute": false }
            ]
        },
        "entities": [],
        "surfaces": []
    })
}

pub(super) fn backoff(attempt: u32) -> Duration {
    let seconds = 1_u64 << attempt.min(5);
    // Deterministic bounded jitter avoids synchronized reconnects without
    // introducing timing-flaky tests.
    Duration::from_millis(
        ((seconds * 1_000).min(MAX_BACKOFF.as_millis() as u64) + u64::from(attempt % 4) * 137)
            .min(MAX_BACKOFF.as_millis() as u64),
    )
}

pub(super) fn wait_or_stop(stopped: &AtomicBool, duration: Duration) {
    let until = Instant::now() + duration;
    while !stopped.load(Ordering::Acquire) && Instant::now() < until {
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
