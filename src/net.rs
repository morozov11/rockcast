//! Shared HTTP policy for radio streams.

use std::time::Duration;

use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue},
};

pub fn stream_headers(connection_close: bool) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Icy-MetaData", HeaderValue::from_static("1"));
    headers.insert("Accept", HeaderValue::from_static("*/*"));
    if connection_close {
        headers.insert("Connection", HeaderValue::from_static("close"));
    }
    headers
}

/// Renew native credentials on each relay connection, including feeder reconnects.
pub fn stream_headers_for_url(url: &str, connection_close: bool) -> Result<HeaderMap, String> {
    let config = crate::rockserver::RuntimeConfig::for_app();
    stream_headers_with_auth(url, connection_close, &config, || {
        crate::session::AccountClient::new(config.clone(), crate::session::OsCredentialStore)
            .device_control_access_token(false)
            .map_err(|_| "Could not renew RockServer session".to_owned())
    })
}

fn stream_headers_with_auth(
    url: &str,
    connection_close: bool,
    config: &crate::rockserver::RuntimeConfig,
    access_token: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<HeaderMap, String> {
    let mut headers = stream_headers(connection_close);
    if config.is_station_stream_url(url) {
        let token = access_token()?.ok_or("Sign in to listen through RockServer")?;
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "Invalid RockServer session token")?;
        value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    Ok(headers)
}

pub fn stream_client(
    connect_timeout: Duration,
    overall_timeout: Option<Duration>,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .user_agent("RockCast/0.1")
        .connect_timeout(connect_timeout);
    builder = match overall_timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder.timeout(None),
    };
    builder.build().map_err(|e| e.to_string())
}

pub fn metadata_interval(headers: &HeaderMap) -> usize {
    headers
        .get("icy-metaint")
        .or_else(|| headers.get("Icy-MetaInt"))
        .or_else(|| headers.get("ice-metaint"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_server_relay_requests_receive_native_credentials() {
        let config = crate::rockserver::RuntimeConfig::for_test("https://server.test".into(), None);
        let relay_url = config.station_stream_url("station").unwrap();
        let headers = stream_headers_with_auth(&relay_url, false, &config, || {
            Ok(Some("synthetic-token".into()))
        })
        .unwrap();
        let auth = &headers[reqwest::header::AUTHORIZATION];
        assert_eq!(auth, "Bearer synthetic-token");
        assert!(auth.is_sensitive());
        assert_eq!(headers["icy-metadata"], "1");
        for url in [
            "https://upstream.test/live.mp3",
            "http://127.0.0.1:1234/tap",
            "https://server.test.evil/api/v1/stations/id/stream",
        ] {
            let headers = stream_headers_with_auth(url, false, &config, || {
                panic!("Credentials must not be read for {url}")
            })
            .unwrap();
            assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
        }
        assert!(
            stream_headers_with_auth(&relay_url, false, &config, || Ok(None))
                .unwrap_err()
                .contains("Sign in")
        );
        assert!(
            stream_headers_with_auth(&relay_url, false, &config, || Err("renewal failed".into()))
                .is_err()
        );
    }

    #[test]
    fn metadata_header_is_case_tolerant() {
        let mut headers = HeaderMap::new();
        headers.insert("ice-metaint", HeaderValue::from_static("16000"));
        assert_eq!(metadata_interval(&headers), 16_000);
    }
}
