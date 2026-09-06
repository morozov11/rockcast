//! Ephemeral Chromecast discovery handles for device-control commands.
//!
//! A handle deliberately contains no network address or Cast identity.  It is
//! only meaningful to this running player until its short local cache expires.

use crate::cast::{CastDeviceInfo, CastService};
use serde::Serialize;
use std::time::Duration;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub(crate) const DISCOVERY_TTL_SECONDS: i64 = 60;
const MAX_RECEIVERS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ChromecastReceiver {
    pub(crate) receiver_id: String,
    pub(crate) display_name: String,
    pub(crate) discovered_at: String,
    pub(crate) expires_at: String,
}

#[derive(Clone)]
struct CachedReceiver<T> {
    receiver_id: String,
    key: String,
    device: T,
    receiver: ChromecastReceiver,
    expires_at: OffsetDateTime,
}

/// Small deterministic cache with an injectable timestamp. `T` is intentionally
/// opaque to this module, so the protocol never learns hostnames or credentials.
pub(crate) struct ReceiverCache<T> {
    entries: Vec<CachedReceiver<T>>,
}

impl<T> Default for ReceiverCache<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T: Clone> ReceiverCache<T> {
    pub(crate) fn replace_at(
        &mut self,
        devices: impl IntoIterator<Item = (String, String, T)>,
        now: OffsetDateTime,
    ) -> Vec<ChromecastReceiver> {
        let discovered_at = format_time(now);
        let expires_at = now + time::Duration::seconds(DISCOVERY_TTL_SECONDS);
        let expires_at_text = format_time(expires_at);
        let mut incoming: Vec<_> = devices.into_iter().collect();
        incoming.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
        incoming.dedup_by(|left, right| left.0 == right.0);
        incoming.truncate(MAX_RECEIVERS);

        let mut next = Vec::with_capacity(incoming.len());
        for (key, display_name, device) in incoming {
            let receiver_id = self
                .entries
                .iter()
                .find(|cached| cached.key == key && cached.expires_at > now)
                .map(|cached| cached.receiver_id.clone())
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let receiver = ChromecastReceiver {
                receiver_id: receiver_id.clone(),
                display_name: safe_display_name(&display_name),
                discovered_at: discovered_at.clone(),
                expires_at: expires_at_text.clone(),
            };
            next.push(CachedReceiver {
                receiver_id,
                key,
                device,
                receiver,
                expires_at,
            });
        }
        self.entries = next;
        self.receivers_at(now)
    }

    pub(crate) fn get_fresh_at(&self, receiver_id: &str, now: OffsetDateTime) -> Option<T> {
        Uuid::parse_str(receiver_id).ok()?;
        self.entries
            .iter()
            .find(|cached| cached.receiver_id == receiver_id && cached.expires_at > now)
            .map(|cached| cached.device.clone())
    }

    pub(crate) fn receivers_at(&self, now: OffsetDateTime) -> Vec<ChromecastReceiver> {
        self.entries
            .iter()
            .filter(|cached| cached.expires_at > now)
            .map(|cached| cached.receiver.clone())
            .collect()
    }
}

/// The real discovery adapter is tiny on purpose; tests use a fake implementation
/// of this trait and inject cache timestamps through `replace_at`.
pub(crate) trait ChromecastDiscovery {
    fn discover(&self, timeout: Duration) -> Result<Vec<CastDeviceInfo>, ()>;
}

pub(crate) struct LocalChromecastDiscovery;

impl ChromecastDiscovery for LocalChromecastDiscovery {
    fn discover(&self, timeout: Duration) -> Result<Vec<CastDeviceInfo>, ()> {
        CastService::scan(timeout).map_err(|error| {
            log::warn!("Chromecast discovery failed: {error}");
        })
    }
}

fn format_time(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_default()
}

fn safe_display_name(value: &str) -> String {
    let value: String = value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(128)
        .collect();
    if value.trim().is_empty() {
        "Chromecast".into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_is_bounded_opaque_and_expires_with_the_injected_clock() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let mut cache = ReceiverCache::default();
        let receivers = cache.replace_at([("private-host".into(), "Kitchen".into(), 7_u8)], now);
        assert_eq!(receivers.len(), 1);
        assert!(!receivers[0].receiver_id.contains("private-host"));
        assert_eq!(cache.get_fresh_at(&receivers[0].receiver_id, now), Some(7));
        assert_eq!(
            cache.get_fresh_at(
                &receivers[0].receiver_id,
                now + time::Duration::seconds(DISCOVERY_TTL_SECONDS),
            ),
            None
        );
    }

    #[test]
    fn refresh_keeps_a_live_handle_but_never_uses_names_as_identity() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let mut cache = ReceiverCache::default();
        let first = cache.replace_at([(String::from("a"), String::from("Same"), 1)], now);
        let refreshed = cache.replace_at(
            [(String::from("a"), String::from("Same"), 2)],
            now + time::Duration::seconds(1),
        );
        assert_eq!(first[0].receiver_id, refreshed[0].receiver_id);
        assert_eq!(cache.get_fresh_at(&first[0].receiver_id, now), Some(2));
    }
}
