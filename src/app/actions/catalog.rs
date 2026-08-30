//! Station catalog and output device refresh.

use std::time::Duration;

use crate::{
    i18n::Lang,
    output::scan_streaming,
    stations::{enrich_stations, load_catalog},
};

use super::super::{RockCastApp, messages::UiMsg};

impl RockCastApp {
    pub(in crate::app) fn bootstrap(&mut self) {
        if self.bootstrapped {
            return;
        }
        self.bootstrapped = true;
        self.refresh_stations();
        self.refresh_devices();
        self.ensure_account_loaded();
    }

    pub(in crate::app) fn refresh_stations(&mut self) {
        self.search_stations(String::new());
    }

    pub(in crate::app) fn search_stations(&mut self, query: String) {
        if self.loading_stations {
            // The previous request cannot be cancelled once HTTP is in flight, but
            // its messages are tagged and ignored below.
        }
        self.station_request_id = self.station_request_id.wrapping_add(1);
        let request_id = self.station_request_id;
        self.loading_stations = true;
        self.status = self.lang.t().loading_stations_status.into();
        let tx = self.ui_tx.clone();
        let lang = self.lang;
        let rockserver = self.rockserver.clone();
        let query = query.trim().to_owned();
        let _ = self.playback.spawn_job(move |cancel| {
            if cancel.is_cancelled() {
                return;
            }
            let locale = match lang {
                Lang::Ru => "ru",
                Lang::En => "en",
            };
            if query.is_empty() {
                let (catalog, source) = load_catalog(lang);
                let _ = tx.send(UiMsg::Stations {
                    list: catalog.clone(),
                    source,
                    request_id,
                    finished: false,
                });
            }
            match crate::rockserver::search(&rockserver, &query, locale) {
                Ok(stations) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let n = stations.len();
                    let _ = tx.send(UiMsg::Stations {
                        list: stations,
                        source: format!("RockServer · {n}"),
                        request_id,
                        finished: true,
                    });
                    return;
                }
                Err(e) => log::warn!("RockServer search failed: {e}; falling back"),
            }
            if !query.is_empty() {
                let (catalog, source) = load_catalog(lang);
                let list = catalog
                    .into_iter()
                    .filter(|station| station_matches(station, &query))
                    .collect();
                let _ = tx.send(UiMsg::Stations {
                    list,
                    source: format!("{source} · offline results"),
                    request_id,
                    finished: true,
                });
                return;
            }
            let (catalog, _) = load_catalog(lang);
            let (merged, source) = enrich_stations(catalog, lang);
            if cancel.is_cancelled() {
                return;
            }
            let _ = tx.send(UiMsg::Stations {
                list: merged,
                source,
                request_id,
                finished: true,
            });
        });
    }

    pub(in crate::app) fn refresh_devices(&mut self) {
        if self.loading_devices {
            return;
        }
        self.loading_devices = true;
        self.status = self.lang.t().searching_devices.into();
        let tx = self.ui_tx.clone();
        let lang = self.lang;
        let _ = self.playback.spawn_job(move |cancel| {
            if cancel.is_cancelled() {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scan_streaming(Duration::from_secs(6), lang, |device| {
                    if !cancel.is_cancelled() {
                        let _ = tx.send(UiMsg::DeviceFound(device));
                    }
                })
            }));
            match result {
                Ok(status) => {
                    let _ = tx.send(UiMsg::DevicesFinished(status));
                }
                Err(_) => {
                    let _ = tx.send(UiMsg::DevicesFinished(lang.t().scan_panic.into()));
                }
            }
        });
    }
}

fn station_matches(station: &crate::stations::Station, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        station.name,
        station.tags,
        station.country,
        station.aliases.join(" ")
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|term| haystack.contains(term))
}
