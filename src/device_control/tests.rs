//! Regression coverage for the bounded DC-012 registration loop.

use super::*;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{atomic::AtomicUsize, mpsc},
};

struct FakeSocket {
    inbound: VecDeque<Inbound>,
    sent: Vec<Value>,
}

impl ControlSocket for FakeSocket {
    fn send_text(&mut self, text: String) -> Result<(), ControlError> {
        self.sent.push(serde_json::from_str(&text).unwrap());
        Ok(())
    }

    fn read(&mut self) -> Result<Option<Inbound>, ControlError> {
        Ok(self.inbound.pop_front())
    }

    fn close(&mut self) {}
}

fn message(kind: &str) -> Inbound {
    Inbound::Text(json!({ "protocol_version": 1, "type": kind, "payload": {} }).to_string())
}

#[test]
fn manifest_and_state_are_truthful_and_bounded() {
    let manifest = manifest();
    assert_eq!(manifest["roles"], json!(["player"]));
    assert!(
        manifest["capabilities"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| {
                !matches!(
                    item["name"].as_str(),
                    Some("media.chromecast") | Some("media.relay")
                )
            })
    );
    assert_eq!(
        PlayerState::idle(63).runtime_state()["volume"],
        json!({"level":63,"muted":false})
    );
}

#[test]
fn protocol_parser_rejects_malformed_and_oversized_frames() {
    assert_eq!(
        inbound_type(r#"{"protocol_version":1,"type":"future.notice","payload":{}}"#).unwrap(),
        Some("future.notice".into())
    );
    assert_eq!(
        inbound_type(r#"{"protocol_version":1,"type":"device.command","payload":[]}"#),
        Err(ControlError::Protocol)
    );
    assert_eq!(
        inbound_type(&"x".repeat(protocol::MAX_FRAME_BYTES + 1)),
        Err(ControlError::Protocol)
    );
}

#[test]
fn reconnect_backoff_is_bounded_and_deterministic() {
    assert_eq!(backoff(0), Duration::from_secs(1));
    assert_eq!(backoff(5), Duration::from_secs(30));
    assert_eq!(backoff(99), Duration::from_secs(30));
}

#[test]
fn hello_registration_and_fresh_snapshot_are_ordered() {
    let state = PublishedState {
        revision: 7,
        observed_at: timestamp(),
        state: PlayerState::idle(50),
    };
    let inner = ClientInner {
        config: RuntimeConfig::for_test("http://127.0.0.1".into(), None),
        auth: Arc::new(FakeAuth),
        transport: Arc::new(FakeTransport),
        state: Mutex::new(Some(state)),
        stopped: AtomicBool::new(false),
        running: AtomicBool::new(false),
        worker: Mutex::new(None),
    };
    let mut socket = FakeSocket {
        inbound: VecDeque::from([message("protocol.welcome"), message("device.registered")]),
        sent: vec![],
    };
    assert!(
        send(
            &mut socket,
            "protocol.hello",
            json!({"supported_protocol_versions":[1]}),
        )
        .is_ok()
    );
    wait_for(
        &mut socket,
        Instant::now() + Duration::from_secs(1),
        "protocol.welcome",
        &inner,
    )
    .unwrap();
    send(
        &mut socket,
        "device.register",
        json!({"device_type":"rockcast","app_version":"0","manifest":manifest()}),
    )
    .unwrap();
    wait_for(
        &mut socket,
        Instant::now() + Duration::from_secs(1),
        "device.registered",
        &inner,
    )
    .unwrap();
    send_full(&mut socket, &inner, 0).unwrap();
    assert_eq!(
        socket
            .sent
            .iter()
            .map(|message| message["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["protocol.hello", "device.register", "device.state_full"]
    );
}

#[test]
fn resync_always_publishes_another_full_snapshot() {
    let state = PublishedState {
        revision: 7,
        observed_at: timestamp(),
        state: PlayerState::idle(50),
    };
    let inner = ClientInner {
        config: RuntimeConfig::for_test("http://127.0.0.1".into(), None),
        auth: Arc::new(FakeAuth),
        transport: Arc::new(FakeTransport),
        state: Mutex::new(Some(state)),
        stopped: AtomicBool::new(false),
        running: AtomicBool::new(false),
        worker: Mutex::new(None),
    };
    let mut socket = FakeSocket {
        inbound: VecDeque::new(),
        sent: vec![],
    };
    let revision = send_full(&mut socket, &inner, 0).unwrap();
    send_full(&mut socket, &inner, revision).unwrap();
    assert_eq!(
        socket
            .sent
            .iter()
            .filter(|message| message["type"] == "device.state_full")
            .count(),
        2
    );
}

#[test]
fn server_disconnect_is_recoverable_and_does_not_execute_playback() {
    let inner = ClientInner {
        config: RuntimeConfig::for_test("http://127.0.0.1".into(), None),
        auth: Arc::new(FakeAuth),
        transport: Arc::new(FakeTransport),
        state: Mutex::new(None),
        stopped: AtomicBool::new(false),
        running: AtomicBool::new(false),
        worker: Mutex::new(None),
    };
    let mut socket = FakeSocket {
        inbound: VecDeque::from([Inbound::Close]),
        sent: vec![],
    };
    assert_eq!(
        wait_for(
            &mut socket,
            Instant::now() + Duration::from_secs(1),
            "protocol.welcome",
            &inner
        ),
        Err(ControlError::Unavailable)
    );
    assert!(socket.sent.is_empty());
}

#[test]
fn start_keeps_one_live_worker() {
    let (started_tx, started_rx) = mpsc::channel();
    let client = DeviceControlClient::with_parts(
        RuntimeConfig::for_test("http://127.0.0.1".into(), None),
        0,
        Arc::new(CountingAuth {
            calls: AtomicUsize::new(0),
            started: started_tx,
        }),
        Arc::new(FakeTransport),
    );
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    client.start();
    assert!(started_rx.recv_timeout(Duration::from_millis(100)).is_err());
    client.shutdown();
}

struct FakeAuth;

impl DeviceControlAuth for FakeAuth {
    fn access_token(&self, _: bool) -> Result<Option<String>, SessionError> {
        Ok(None)
    }
}

struct FakeTransport;

impl DeviceControlTransport for FakeTransport {
    fn connect(&self, _: &str, _: &str) -> Result<Box<dyn ControlSocket>, ControlError> {
        Err(ControlError::Unavailable)
    }
}

struct CountingAuth {
    calls: AtomicUsize,
    started: mpsc::Sender<()>,
}

impl DeviceControlAuth for CountingAuth {
    fn access_token(&self, _: bool) -> Result<Option<String>, SessionError> {
        if self.calls.fetch_add(1, Ordering::AcqRel) == 0 {
            let _ = self.started.send(());
        }
        Ok(None)
    }
}
