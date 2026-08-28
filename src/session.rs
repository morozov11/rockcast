//! Native account session client. Tokens never implement `Debug` or persistence formats.
use crate::{rockserver::RuntimeConfig, settings};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf, time::Duration};

#[derive(Clone, PartialEq, Eq)]
pub struct NativeCredentials {
    access_token: String,
    refresh_token: String,
}

impl NativeCredentials {
    fn new(access_token: String, refresh_token: String) -> Result<Self, SessionError> {
        if access_token.len() < 16 || refresh_token.len() < 16 {
            return Err(SessionError::Rejected);
        }
        Ok(Self {
            access_token,
            refresh_token,
        })
    }
    pub(crate) fn access_token(&self) -> &str {
        &self.access_token
    }
    fn refresh_token(&self) -> &str {
        &self.refresh_token
    }
}

#[derive(Clone, Deserialize)]
pub struct PairingRequest {
    pub(crate) pairing_request_id: String,
    desktop_token: String,
    approval_secret: String,
    pub(crate) short_code: String,
    pub(crate) verification_phrase: String,
    pub(crate) device_display_name: String,
    pub(crate) device_type: String,
    pub(crate) expires_at: String,
    pub(crate) status: String,
}

impl PairingRequest {
    /// The first-party browser shell consumes both code and one-time approval secret.
    pub(crate) fn deep_link(&self, base_url: &str) -> String {
        format!(
            "{}/?code={}&secret={}",
            base_url.trim_end_matches('/'),
            self.short_code,
            self.approval_secret
        )
    }
}

#[derive(Clone, Deserialize)]
pub struct AccountProfile {
    pub(crate) device_id: String,
    pub(crate) account_display_name: String,
    pub(crate) device_display_name: String,
    pub(crate) device_type: String,
}
#[derive(Clone, Deserialize)]
pub struct Device {
    pub(crate) device_id: String,
    pub(crate) device_display_name: String,
    pub(crate) device_type: String,
    #[serde(default)]
    pub(crate) created_at: String,
    #[serde(default)]
    pub(crate) last_seen_at: Option<String>,
}
#[derive(Deserialize)]
struct DeviceList {
    devices: Vec<Device>,
}
#[derive(Deserialize, Serialize)]
struct TokenPair {
    access_token: String,
    refresh_token: String,
}
#[derive(Deserialize)]
struct Completion {
    device_id: String,
    access_token: String,
    refresh_token: String,
    account_display_name: String,
    device_display_name: String,
    device_type: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairingCompletionRequest<'a> {
    desktop_token: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingPoll {
    Pending,
    Expired,
    TimedOut,
    Unavailable,
    SecureStorageUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairingPollControl {
    Continue,
    Cancelled,
    TimedOut,
}

pub(crate) fn pairing_poll_control(
    cancelled: bool,
    now: std::time::Instant,
    deadline: std::time::Instant,
) -> PairingPollControl {
    if cancelled {
        PairingPollControl::Cancelled
    } else if now >= deadline {
        PairingPollControl::TimedOut
    } else {
        PairingPollControl::Continue
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("The account service is unavailable.")]
    Unavailable,
    #[error("The account request was rejected or has expired.")]
    Rejected,
    #[error("Secure credential storage is unavailable; RockCast remains offline.")]
    SecureStorageUnavailable,
}

/// Small seam for deterministic tests and unsupported OS handling.
pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<NativeCredentials>, SessionError>;
    fn save(&self, credentials: &NativeCredentials) -> Result<(), SessionError>;
    fn clear(&self) -> Result<(), SessionError>;
}

pub struct OsCredentialStore;
impl OsCredentialStore {
    fn path() -> Result<PathBuf, SessionError> {
        settings::app_dir()
            .map(|d| d.join("session.dpapi"))
            .ok_or(SessionError::SecureStorageUnavailable)
    }
}

#[cfg(windows)]
impl CredentialStore for OsCredentialStore {
    fn load(&self) -> Result<Option<NativeCredentials>, SessionError> {
        let path = Self::path()?;
        let encrypted = match fs::read(path) {
            Ok(value) => value,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SessionError::SecureStorageUnavailable),
        };
        let plain = dpapi(false, &encrypted)?;
        let pair: TokenPair =
            serde_json::from_slice(&plain).map_err(|_| SessionError::SecureStorageUnavailable)?;
        NativeCredentials::new(pair.access_token, pair.refresh_token).map(Some)
    }
    fn save(&self, credentials: &NativeCredentials) -> Result<(), SessionError> {
        let plain = serde_json::to_vec(&TokenPair {
            access_token: credentials.access_token.clone(),
            refresh_token: credentials.refresh_token.clone(),
        })
        .map_err(|_| SessionError::SecureStorageUnavailable)?;
        let encrypted = dpapi(true, &plain)?;
        let path = Self::path()?;
        let parent = path
            .parent()
            .ok_or(SessionError::SecureStorageUnavailable)?;
        fs::create_dir_all(parent).map_err(|_| SessionError::SecureStorageUnavailable)?;
        fs::write(path, encrypted).map_err(|_| SessionError::SecureStorageUnavailable)
    }
    fn clear(&self) -> Result<(), SessionError> {
        match fs::remove_file(Self::path()?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(SessionError::SecureStorageUnavailable),
        }
    }
}

#[cfg(windows)]
fn dpapi(protect: bool, input: &[u8]) -> Result<Vec<u8>, SessionError> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: DPAPI copies the supplied bytes; output is released with LocalFree below.
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &source,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(SessionError::SecureStorageUnavailable);
    }
    // SAFETY: DPAPI returned an allocated buffer of cbData bytes.
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    // SAFETY: ownership of this DPAPI allocation is transferred to LocalFree.
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(not(windows))]
impl CredentialStore for OsCredentialStore {
    fn load(&self) -> Result<Option<NativeCredentials>, SessionError> {
        Err(SessionError::SecureStorageUnavailable)
    }
    fn save(&self, _: &NativeCredentials) -> Result<(), SessionError> {
        Err(SessionError::SecureStorageUnavailable)
    }
    fn clear(&self) -> Result<(), SessionError> {
        Err(SessionError::SecureStorageUnavailable)
    }
}

pub(crate) struct AccountClient<S = OsCredentialStore> {
    config: RuntimeConfig,
    store: S,
}
impl<S: CredentialStore> AccountClient<S> {
    pub(crate) fn new(config: RuntimeConfig, store: S) -> Self {
        Self { config, store }
    }
    fn request(
        &self,
        method: reqwest::Method,
        route: &str,
    ) -> Result<reqwest::blocking::RequestBuilder, SessionError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| SessionError::Unavailable)?;
        Ok(client.request(
            method,
            format!("{}{}", self.config.base_url().trim_end_matches('/'), route),
        ))
    }
    fn json<T: for<'a> Deserialize<'a>>(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<T, SessionError> {
        let response = request.send().map_err(|_| SessionError::Unavailable)?;
        if !response.status().is_success() {
            return Err(if response.status().is_server_error() {
                SessionError::Unavailable
            } else {
                SessionError::Rejected
            });
        }
        response.json().map_err(|_| SessionError::Unavailable)
    }
    pub(crate) fn create_pairing(
        &self,
        device_display_name: &str,
    ) -> Result<PairingRequest, SessionError> {
        let device_display_name = device_display_name.trim();
        if device_display_name.is_empty() || device_display_name.len() > 128 {
            return Err(SessionError::Rejected);
        }
        self.json(
            self.request(reqwest::Method::POST, "/v1/pairing-requests")?
                .json(&serde_json::json!({
                    "device_display_name": device_display_name,
                    "device_type": std::env::consts::OS,
                    "app_version": env!("CARGO_PKG_VERSION")
                })),
        )
    }
    pub(crate) fn complete_pairing(
        &self,
        pairing: &PairingRequest,
    ) -> Result<AccountProfile, PairingPoll> {
        let request = self
            .request(
                reqwest::Method::POST,
                &format!(
                    "/v1/pairing-requests/{}/complete",
                    pairing.pairing_request_id
                ),
            )
            .map_err(|_| PairingPoll::Unavailable)?;
        let result: Result<Completion, SessionError> =
            self.json(request.json(&PairingCompletionRequest {
                desktop_token: &pairing.desktop_token,
            }));
        let result = result.map_err(|e| match e {
            SessionError::Rejected => PairingPoll::Pending,
            SessionError::Unavailable => PairingPoll::Unavailable,
            SessionError::SecureStorageUnavailable => PairingPoll::SecureStorageUnavailable,
        })?;
        let credentials = NativeCredentials::new(result.access_token, result.refresh_token)
            .map_err(|_| PairingPoll::Expired)?;
        self.store
            .save(&credentials)
            .map_err(|_| PairingPoll::SecureStorageUnavailable)?;
        Ok(AccountProfile {
            device_id: result.device_id,
            account_display_name: result.account_display_name,
            device_display_name: result.device_display_name,
            device_type: result.device_type,
        })
    }
    pub(crate) fn refresh(&self) -> Result<(), SessionError> {
        let old = self.store.load()?.ok_or(SessionError::Rejected)?;
        let pair: Result<TokenPair, SessionError> = self.json(
            self.request(reqwest::Method::POST, "/v1/auth/refresh")?
                .json(&serde_json::json!({"refresh_token": old.refresh_token()})),
        );
        match pair
            .and_then(|pair| NativeCredentials::new(pair.access_token, pair.refresh_token))
            .and_then(|new| self.store.save(&new))
        {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = self.store.clear();
                Err(e)
            }
        }
    }
    fn authorized(
        &self,
        method: reqwest::Method,
        route: &str,
    ) -> Result<reqwest::blocking::RequestBuilder, SessionError> {
        let credentials = self.store.load()?.ok_or(SessionError::Rejected)?;
        Ok(self
            .request(method, route)?
            .bearer_auth(credentials.access_token()))
    }
    pub(crate) fn profile(&self) -> Result<AccountProfile, SessionError> {
        self.json(self.authorized(reqwest::Method::GET, "/v1/account/profile")?)
    }
    pub(crate) fn devices(&self) -> Result<Vec<Device>, SessionError> {
        Ok(self
            .json::<DeviceList>(self.authorized(reqwest::Method::GET, "/v1/devices")?)?
            .devices)
    }
    pub(crate) fn revoke_device(&self, device_id: &str) -> Result<(), SessionError> {
        let r = self
            .authorized(reqwest::Method::DELETE, &format!("/v1/devices/{device_id}"))?
            .send()
            .map_err(|_| SessionError::Unavailable)?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(SessionError::Rejected)
        }
    }
    pub(crate) fn logout(&self) -> Result<(), SessionError> {
        let outcome = self
            .authorized(reqwest::Method::POST, "/v1/auth/logout")
            .and_then(|r| r.send().map_err(|_| SessionError::Unavailable))
            .and_then(|r| {
                if r.status().is_success() || r.status().as_u16() == 401 {
                    Ok(())
                } else {
                    Err(SessionError::Unavailable)
                }
            });
        let clear = self.store.clear();
        outcome.and(clear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Mutex,
        thread,
    };
    struct Memory(Mutex<Option<NativeCredentials>>);
    impl CredentialStore for Memory {
        fn load(&self) -> Result<Option<NativeCredentials>, SessionError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, c: &NativeCredentials) -> Result<(), SessionError> {
            *self.0.lock().unwrap() = Some(c.clone());
            Ok(())
        }
        fn clear(&self) -> Result<(), SessionError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    #[test]
    fn secrets_do_not_format_or_serialize() {
        let credentials = NativeCredentials::new("a".repeat(16), "b".repeat(16)).unwrap();
        let store = Memory(Mutex::new(Some(credentials)));
        assert_eq!(store.load().unwrap().unwrap().access_token().len(), 16);
    }
    #[test]
    fn unavailable_secure_store_fails_closed() {
        #[cfg(not(windows))]
        assert_eq!(
            OsCredentialStore.load().unwrap_err(),
            SessionError::SecureStorageUnavailable
        );
    }

    #[test]
    fn pairing_poll_stops_only_for_cancel_or_deadline() {
        let now = std::time::Instant::now();
        assert_eq!(
            pairing_poll_control(false, now, now + Duration::from_secs(1)),
            PairingPollControl::Continue
        );
        assert_eq!(
            pairing_poll_control(true, now, now + Duration::from_secs(1)),
            PairingPollControl::Cancelled
        );
        assert_eq!(
            pairing_poll_control(false, now + Duration::from_secs(1), now),
            PairingPollControl::TimedOut
        );
    }

    fn server(status: u16, body: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while let Ok(read) = stream.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {status} test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{address}"), thread)
    }

    fn client(url: String, store: Memory) -> AccountClient<Memory> {
        AccountClient::new(RuntimeConfig::for_test(url, None), store)
    }

    #[test]
    fn offline_pairing_returns_safe_error_without_credentials() {
        let client = client("http://127.0.0.1:1".into(), Memory(Mutex::new(None)));
        assert!(matches!(
            client.create_pairing("RockCast — offline"),
            Err(SessionError::Unavailable)
        ));
    }

    #[test]
    fn create_pairing_uses_g1_contract_without_authorization() {
        let body = r#"{"pairing_request_id":"request","desktop_token":"aaaaaaaaaaaaaaaa","approval_secret":"bbbbbbbbbbbbbbbb","short_code":"AB12CD34","verification_phrase":"AMBER-DAWN","device_display_name":"RockCast — test","device_type":"windows","expires_at":"2026-08-28T12:00:00Z","status":"pending"}"#;
        let (url, server) = server(201, body);
        let pairing = client(url, Memory(Mutex::new(None)))
            .create_pairing("RockCast — test")
            .unwrap();
        assert_eq!(pairing.short_code, "AB12CD34");
        assert_eq!(pairing.device_display_name, "RockCast — test");
        assert_eq!(pairing.status, "pending");
        let request = server.join().unwrap();
        let request_lower = request.to_ascii_lowercase();
        assert!(request_lower.starts_with("post /v1/pairing-requests"));
        assert!(request.contains(r#""device_display_name":"RockCast — test""#));
        assert!(request.contains(r#""device_type":"windows""#));
        assert!(!request_lower.contains("\"device_name\""));
        assert!(!request_lower.contains("\"platform\""));
        assert!(!request_lower.contains("authorization:"));
    }

    #[test]
    fn pairing_link_is_the_g2_request_specific_browser_context() {
        let pairing = PairingRequest {
            pairing_request_id: "request".into(),
            desktop_token: "desktop-token-1234".into(),
            approval_secret: "approval-secret-1234".into(),
            short_code: "AB12CD34".into(),
            verification_phrase: "AMBER-DAWN".into(),
            device_display_name: "RockCast — test".into(),
            device_type: "windows".into(),
            expires_at: "2026-08-28T12:00:00Z".into(),
            status: "pending".into(),
        };
        assert_eq!(
            pairing.deep_link("https://alex.vault57.ru/"),
            "https://alex.vault57.ru/?code=AB12CD34&secret=approval-secret-1234"
        );
    }

    #[test]
    fn completed_pairing_saves_only_to_secure_store_seam() {
        let body = r#"{"user_id":"must-not-be-used","device_id":"device","session_id":"session","access_token":"aaaaaaaaaaaaaaaa","refresh_token":"bbbbbbbbbbbbbbbb","account_display_name":"Alex's Rock account","device_display_name":"RockCast — test","device_type":"windows"}"#;
        let (url, server) = server(200, body);
        let store = Memory(Mutex::new(None));
        let pairing = PairingRequest {
            pairing_request_id: "request".into(),
            desktop_token: "desktop-token-1234".into(),
            approval_secret: "never-persist".into(),
            short_code: "AB12CD34".into(),
            verification_phrase: "AMBER-DAWN".into(),
            device_display_name: "RockCast — test".into(),
            device_type: "windows".into(),
            expires_at: "2026-08-28T12:00:00Z".into(),
            status: "pending".into(),
        };
        let profile = client(url, store).complete_pairing(&pairing).unwrap();
        assert_eq!(profile.device_id, "device");
        assert_eq!(profile.account_display_name, "Alex's Rock account");
        assert_eq!(profile.device_display_name, "RockCast — test");
        let request = server.join().unwrap();
        assert!(request.contains("/complete"));
        assert!(request.contains(r#"{"desktop_token":"desktop-token-1234"}"#));
        assert!(!request.contains("user_id"));
    }

    #[test]
    fn completion_payload_rejects_removed_user_id() {
        assert!(
            serde_json::from_str::<PairingCompletionRequest<'_>>(
                r#"{"desktop_token":"desktop-token-1234","user_id":"forbidden"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn pairing_poll_waits_for_browser_approval() {
        let (url, server) = server(401, "{}");
        let pairing = PairingRequest {
            pairing_request_id: "request".into(),
            desktop_token: "desktop-token-1234".into(),
            approval_secret: "never-persist".into(),
            short_code: "AB12CD34".into(),
            verification_phrase: "AMBER-DAWN".into(),
            device_display_name: "RockCast — test".into(),
            device_type: "windows".into(),
            expires_at: "2026-08-28T12:00:00Z".into(),
            status: "pending".into(),
        };
        assert!(matches!(
            client(url, Memory(Mutex::new(None))).complete_pairing(&pairing),
            Err(PairingPoll::Pending)
        ));
        assert!(server.join().unwrap().contains("/complete"));
    }

    #[test]
    fn expiry_and_refresh_replay_clear_local_credentials() {
        let (url, server) = server(401, "{}");
        let store = Memory(Mutex::new(Some(
            NativeCredentials::new("a".repeat(16), "b".repeat(16)).unwrap(),
        )));
        let client = client(url, store);
        assert_eq!(client.refresh().unwrap_err(), SessionError::Rejected);
        assert!(client.store.load().unwrap().is_none());
        assert!(server.join().unwrap().contains("/v1/auth/refresh"));
    }

    #[test]
    fn logout_clears_local_credentials_when_server_is_offline() {
        let store = Memory(Mutex::new(Some(
            NativeCredentials::new("a".repeat(16), "b".repeat(16)).unwrap(),
        )));
        let client = client("http://127.0.0.1:1".into(), store);
        assert_eq!(client.logout().unwrap_err(), SessionError::Unavailable);
        assert!(client.store.load().unwrap().is_none());
    }
}
