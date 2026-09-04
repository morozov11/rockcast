//! RockServer device-control v1 lifecycle for the local RockCast player.
//! It publishes local facts only; remote command execution remains outside DC-012.

#[path = "device_control/protocol.rs"]
mod protocol;
#[cfg(test)]
#[path = "device_control/tests.rs"]
mod tests;
#[path = "device_control/transport.rs"]
mod transport;

use crate::{
    rockserver::RuntimeConfig,
    session::{AccountClient, OsCredentialStore, SessionError},
};
use parking_lot::Mutex;
pub(crate) use protocol::PlayerState;
use protocol::{
    ControlError, HEARTBEAT, Inbound, NO_IDENTITY_DELAY, POLL, PublishedState, backoff,
    control_endpoint, inbound_type, is_auth_error, manifest, send, timestamp, wait_or_stop,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use transport::{ControlSocket, DeviceControlTransport, TungsteniteTransport};

trait DeviceControlAuth: Send + Sync {
    fn access_token(&self, force_renewal: bool) -> Result<Option<String>, SessionError>;
}

struct AccountAuth {
    config: RuntimeConfig,
}

impl DeviceControlAuth for AccountAuth {
    fn access_token(&self, force_renewal: bool) -> Result<Option<String>, SessionError> {
        AccountClient::new(self.config.clone(), OsCredentialStore)
            .device_control_access_token(force_renewal)
    }
}

struct ClientInner {
    config: RuntimeConfig,
    auth: Arc<dyn DeviceControlAuth>,
    transport: Arc<dyn DeviceControlTransport>,
    state: Mutex<Option<PublishedState>>,
    stopped: AtomicBool,
    running: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
}

/// One bounded, process-local connection loop. It neither creates nor stores
/// identity: pairing credentials remain exclusively in `session.dpapi`.
pub(crate) struct DeviceControlClient {
    inner: Arc<ClientInner>,
}

impl DeviceControlClient {
    pub(crate) fn new(config: RuntimeConfig, initial_revision: u64) -> Self {
        let auth = Arc::new(AccountAuth {
            config: config.clone(),
        });
        Self::with_parts(
            config,
            initial_revision,
            auth,
            Arc::new(TungsteniteTransport),
        )
    }

    fn with_parts(
        config: RuntimeConfig,
        initial_revision: u64,
        auth: Arc<dyn DeviceControlAuth>,
        transport: Arc<dyn DeviceControlTransport>,
    ) -> Self {
        let client = Self {
            inner: Arc::new(ClientInner {
                config,
                auth,
                transport,
                state: Mutex::new(None),
                stopped: AtomicBool::new(false),
                running: AtomicBool::new(false),
                worker: Mutex::new(None),
            }),
        };
        // The first UI snapshot becomes initial_revision + 1. Keeping the
        // counter in AppSettings prevents a restart from replaying stale state.
        client.inner.state.lock().replace(PublishedState {
            revision: initial_revision,
            observed_at: String::new(),
            state: PlayerState::idle(0),
        });
        client.start();
        client
    }

    fn start(&self) {
        if self.inner.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let worker = thread::Builder::new()
            .name("rockcast-device-control".into())
            .spawn(move || run_worker(inner));
        match worker {
            Ok(worker) => *self.inner.worker.lock() = Some(worker),
            Err(_) => self.inner.running.store(false, Ordering::Release),
        }
    }

    /// Replaces a single latest-value slot; there is no unbounded state queue.
    /// Returns the durable revision the caller should save with AppSettings.
    pub(crate) fn publish(&self, state: PlayerState) -> Option<u64> {
        let mut published = self.inner.state.lock();
        let current = published.as_mut()?;
        if current.state == state && !current.observed_at.is_empty() {
            return None;
        }
        current.revision = current.revision.saturating_add(1).max(1);
        current.observed_at = timestamp();
        current.state = state;
        Some(current.revision)
    }

    pub(crate) fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.inner.worker.lock().take() {
            let _ = worker.join();
        }
    }
}

impl Drop for DeviceControlClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_worker(inner: Arc<ClientInner>) {
    let endpoint = control_endpoint(inner.config.base_url());
    let mut forced_renewal = false;
    let mut retry = 0_u32;
    while !inner.stopped.load(Ordering::Acquire) {
        let token = match inner.auth.access_token(forced_renewal) {
            Ok(Some(token)) => token,
            Ok(None) | Err(SessionError::Unauthorized) | Err(SessionError::Rejected) => {
                forced_renewal = false;
                wait_or_stop(&inner.stopped, NO_IDENTITY_DELAY);
                continue;
            }
            Err(_) => {
                wait_or_stop(&inner.stopped, backoff(retry));
                retry = retry.saturating_add(1);
                continue;
            }
        };
        forced_renewal = false;
        match inner.transport.connect(&endpoint, &token) {
            Ok(socket) => match run_connection(&inner, socket) {
                Ok(()) => retry = 0,
                Err(ControlError::Authentication) => {
                    // A server-side auth rejection gets one renewal through the
                    // existing device-session endpoint; that endpoint owns revoke.
                    forced_renewal = true;
                    retry = 0;
                }
                Err(_) => {
                    wait_or_stop(&inner.stopped, backoff(retry));
                    retry = retry.saturating_add(1);
                }
            },
            Err(ControlError::Authentication) => forced_renewal = true,
            Err(_) => {
                wait_or_stop(&inner.stopped, backoff(retry));
                retry = retry.saturating_add(1);
            }
        }
    }
    inner.running.store(false, Ordering::Release);
}

fn run_connection(
    inner: &ClientInner,
    mut socket: Box<dyn ControlSocket>,
) -> Result<(), ControlError> {
    send(
        &mut *socket,
        "protocol.hello",
        json!({ "supported_protocol_versions": [1] }),
    )?;
    let registration_deadline = Instant::now() + Duration::from_secs(10);
    wait_for(
        &mut *socket,
        registration_deadline,
        "protocol.welcome",
        inner,
    )?;
    send(
        &mut *socket,
        "device.register",
        json!({
            "device_type": "rockcast",
            "app_version": env!("CARGO_PKG_VERSION"),
            "manifest": manifest(),
        }),
    )?;
    wait_for(
        &mut *socket,
        registration_deadline,
        "device.registered",
        inner,
    )?;

    // A full snapshot is sent for every fresh connection, even if no local
    // state changed while the socket was down.
    let mut sent_revision = send_full(&mut *socket, inner, 0)?;
    let mut heartbeat_sequence = 0_u64;
    let mut next_heartbeat = Instant::now() + HEARTBEAT;
    loop {
        if inner.stopped.load(Ordering::Acquire) {
            socket.close();
            return Ok(());
        }
        if let Some(current) = inner.state.lock().clone()
            && current.revision > sent_revision
        {
            sent_revision = send_full(&mut *socket, inner, sent_revision)?;
        }
        if Instant::now() >= next_heartbeat {
            send(
                &mut *socket,
                "device.heartbeat",
                json!({ "sequence": heartbeat_sequence }),
            )?;
            heartbeat_sequence = heartbeat_sequence.saturating_add(1);
            next_heartbeat = Instant::now() + HEARTBEAT;
        }
        match socket.read()? {
            None => thread::sleep(POLL),
            Some(Inbound::Ping(payload)) => {
                // Tungstenite normally responds during read; an explicit pong is
                // not required for the RockServer application heartbeat.
                let _ = payload;
            }
            Some(Inbound::Close) => return Err(ControlError::Unavailable),
            Some(Inbound::Text(text)) => match inbound_type(&text)? {
                Some(kind) if kind == "device.resync_requested" => {
                    sent_revision = send_full(&mut *socket, inner, sent_revision)?;
                }
                Some(kind) if kind == "protocol.error" && is_auth_error(&text) => {
                    return Err(ControlError::Authentication);
                }
                Some(kind) if kind == "device.command" => {
                    // DC-013 owns parsing, authorization acknowledgements and
                    // terminal results. Do not pretend this frame executed.
                    log::debug!("device-control command deferred to DC-013");
                }
                Some(_) | None => {}
            },
        }
    }
}

fn wait_for(
    socket: &mut dyn ControlSocket,
    deadline: Instant,
    expected: &str,
    inner: &ClientInner,
) -> Result<(), ControlError> {
    while Instant::now() < deadline {
        if inner.stopped.load(Ordering::Acquire) {
            socket.close();
            return Err(ControlError::Unavailable);
        }
        match socket.read()? {
            None => thread::sleep(POLL),
            Some(Inbound::Close) => return Err(ControlError::Unavailable),
            Some(Inbound::Ping(_)) => {}
            Some(Inbound::Text(text)) => match inbound_type(&text)? {
                Some(kind) if kind == expected => return Ok(()),
                Some(kind) if kind == "protocol.error" && is_auth_error(&text) => {
                    return Err(ControlError::Authentication);
                }
                Some(_) | None => {}
            },
        }
    }
    Err(ControlError::Protocol)
}

fn send_full(
    socket: &mut dyn ControlSocket,
    inner: &ClientInner,
    previous: u64,
) -> Result<u64, ControlError> {
    let current = inner.state.lock().clone().ok_or(ControlError::Protocol)?;
    if current.revision == 0 || current.observed_at.is_empty() {
        return Err(ControlError::Protocol);
    }
    let revision = current.revision.max(previous.saturating_add(1));
    send(
        socket,
        "device.state_full",
        json!({ "snapshot": {
            "state_revision": revision,
            "observed_at": current.observed_at,
            "state": current.state.runtime_state(),
        }}),
    )?;
    Ok(revision)
}
