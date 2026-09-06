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

/// The only controller intents RockCast can map to its existing local player.
/// Parsing them here keeps network input away from the UI and playback layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlayerCommand {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    PlayStation { station_id: String },
    PlayStream { stream_uri: String },
    SetVolume { level: u8 },
    ChangeVolume { delta: i8 },
    SetMute { muted: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeviceCommand {
    pub(crate) id: String,
    pub(crate) command: PlayerCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandReject {
    pub(super) command_id: Option<String>,
    pub(super) code: &'static str,
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

/// Strictly accepts only a command for the authenticated device.  The server
/// routes by identity, but the target is still checked locally so it can never
/// retarget this player or become an arbitrary URL/action channel.
pub(super) fn command_from_frame(
    frame: &str,
    authenticated_device_id: Option<&str>,
) -> Result<DeviceCommand, CommandReject> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(CommandReject {
            command_id: None,
            code: "frame_too_large",
        });
    }
    let frame: Value = serde_json::from_str(frame).map_err(|_| CommandReject {
        command_id: None,
        code: "invalid_message",
    })?;
    let envelope = frame.as_object().ok_or(CommandReject {
        command_id: None,
        code: "invalid_message",
    })?;
    let message_id = envelope.get("message_id").and_then(Value::as_str);
    let sent_at = envelope.get("sent_at").and_then(Value::as_str);
    if envelope.get("protocol_version").and_then(Value::as_i64) != Some(1)
        || envelope.get("type").and_then(Value::as_str) != Some("device.command")
        || message_id.and_then(|id| Uuid::parse_str(id).ok()).is_none()
        || sent_at
            .and_then(|at| OffsetDateTime::parse(at, &Rfc3339).ok())
            .is_none()
    {
        return Err(CommandReject {
            command_id: None,
            code: "invalid_message",
        });
    }
    let payload = envelope
        .get("payload")
        .and_then(Value::as_object)
        .ok_or(CommandReject {
            command_id: None,
            code: "invalid_payload",
        })?;
    let command_id = payload
        .get("command_id")
        .and_then(Value::as_str)
        .filter(|id| Uuid::parse_str(id).is_ok())
        .map(str::to_owned);
    if payload.len() > 4
        || !payload.keys().all(|key| {
            matches!(
                key.as_str(),
                "command_id" | "target" | "deadline_at" | "body"
            )
        })
    {
        return Err(CommandReject {
            command_id: command_id.clone(),
            code: "invalid_payload",
        });
    }
    let command_id = command_id.ok_or(CommandReject {
        command_id: None,
        code: "invalid_payload",
    })?;
    if let Some(deadline_at) = payload.get("deadline_at")
        && deadline_at
            .as_str()
            .and_then(|at| OffsetDateTime::parse(at, &Rfc3339).ok())
            .is_none()
    {
        return Err(CommandReject {
            command_id: Some(command_id),
            code: "invalid_payload",
        });
    }
    let target = payload
        .get("target")
        .and_then(Value::as_object)
        .ok_or_else(|| CommandReject {
            command_id: Some(command_id.clone()),
            code: "invalid_payload",
        })?;
    if target.len() != 1
        || target
            .get("device_id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .is_none()
        || authenticated_device_id != target.get("device_id").and_then(Value::as_str)
    {
        return Err(CommandReject {
            command_id: Some(command_id),
            code: "invalid_payload",
        });
    }
    let body = payload
        .get("body")
        .and_then(Value::as_object)
        .ok_or_else(|| CommandReject {
            command_id: Some(command_id.clone()),
            code: "invalid_payload",
        })?;
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| CommandReject {
            command_id: Some(command_id.clone()),
            code: "invalid_payload",
        })?;
    let exact = |keys: &[&str]| {
        body.len() == keys.len() && body.keys().all(|key| keys.contains(&key.as_str()))
    };
    let command = match name {
        "playback.play" if exact(&["name"]) => Some(PlayerCommand::Play),
        "playback.pause" if exact(&["name"]) => Some(PlayerCommand::Pause),
        "playback.stop" if exact(&["name"]) => Some(PlayerCommand::Stop),
        "playback.next" if exact(&["name"]) => Some(PlayerCommand::Next),
        "playback.previous" if exact(&["name"]) => Some(PlayerCommand::Previous),
        "station.play_station" if exact(&["name", "station_id"]) => body
            .get("station_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .map(|station_id| PlayerCommand::PlayStation {
                station_id: station_id.into(),
            }),
        "station.play_stream"
            if exact(&["name", "source", "stream_uri"])
                && body.get("source").and_then(Value::as_str) == Some("rockserver_catalog") =>
        {
            body.get("stream_uri")
                .and_then(Value::as_str)
                .filter(|uri| !uri.is_empty() && uri.len() <= 2048)
                .map(|stream_uri| PlayerCommand::PlayStream {
                    stream_uri: stream_uri.into(),
                })
        }
        "volume.set_volume" if exact(&["name", "level"]) => body
            .get("level")
            .and_then(Value::as_u64)
            .and_then(|level| u8::try_from(level).ok())
            .filter(|level| *level <= 100)
            .map(|level| PlayerCommand::SetVolume { level }),
        "volume.change_volume" if exact(&["name", "delta"]) => body
            .get("delta")
            .and_then(Value::as_i64)
            .and_then(|delta| i8::try_from(delta).ok())
            .filter(|delta| *delta != 0 && (-100..=100).contains(delta))
            .map(|delta| PlayerCommand::ChangeVolume { delta }),
        "volume.set_mute" if exact(&["name", "muted"]) => body
            .get("muted")
            .and_then(Value::as_bool)
            .map(|muted| PlayerCommand::SetMute { muted }),
        _ => {
            return Err(CommandReject {
                command_id: Some(command_id),
                code: "unsupported_command",
            });
        }
    }
    .ok_or_else(|| CommandReject {
        command_id: Some(command_id.clone()),
        code: "invalid_payload",
    })?;
    Ok(DeviceCommand {
        id: command_id,
        command,
    })
}

pub(super) fn command_accepted(
    socket: &mut dyn ControlSocket,
    command_id: &str,
) -> Result<(), ControlError> {
    send(
        socket,
        "command.accepted",
        json!({ "command_id": command_id, "accepted_at": timestamp() }),
    )
}

pub(super) fn command_result(
    socket: &mut dyn ControlSocket,
    command_id: &str,
    result: &CommandResult,
) -> Result<(), ControlError> {
    let error = result.error.as_ref().map(|error| {
        json!({
            "code": error.code,
            "message": error.message,
            "request_id": command_id,
            "details": {}
        })
    });
    send(
        socket,
        "command.result",
        json!({
            "command_id": command_id,
            "status": if result.error.is_some() { "failed" } else { "succeeded" },
            "completed_at": timestamp(),
            "error": error,
        }),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandResult {
    pub(crate) error: Option<CommandFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandFailure {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

impl CommandResult {
    pub(crate) fn succeeded() -> Self {
        Self { error: None }
    }
    pub(crate) fn failed(code: &'static str, message: &'static str) -> Self {
        Self {
            error: Some(CommandFailure { code, message }),
        }
    }
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
