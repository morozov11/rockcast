# RockCast source map

## Root responsibilities

| Area | Owns |
|---|---|
| `app/` | egui state, rendering, and adaptation of background events |
| `playback/`, `runtime.rs` | playback lifecycle, bounded background work, cancellation |
| `local/`, `cast/`, `relay/`, `audio/` | output adapters and stream transport/decoding |
| `stations/`, `personal_data.rs`, `settings.rs` | catalog and durable local user data |
| `session.rs`, `rockserver.rs`, `voice/` | paired identity and RockServer HTTP/WSS clients |
| `device_control.rs` | DC-012 player registration lifecycle; `device_control/protocol.rs` owns v1 JSON, `transport.rs` owns tungstenite, and `tests.rs` holds its regression coverage |

## Dependency direction

`main → app → playback → {local, cast, relay}`. `app` may use settings, catalog,
session, and device-control; protocol and output adapters must not depend on egui.
`device_control` may depend on `session` and `rockserver`, but never on app or playback.

## Lifecycle and reading order

Start with `lib.rs`, `main.rs`, then `app/mod.rs` and `playback/mod.rs`; follow the selected
output adapter. For paired features read `session.rs` before `voice/` or `device_control.rs`.
The device-control loop is `DeviceControlClient::new → publish → shutdown`; it reuses the
existing paired credentials, publishes facts only, and does not execute DC-013 commands.

## Tests

Focused domain tests live beside their module; large device-control regression coverage is in
`device_control/tests.rs`. Broader integration tests remain under `tests/`.
