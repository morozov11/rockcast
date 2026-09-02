# RockCast tasks

## Voice route selection — 2026-09-02

- Goal: keep anonymous voice usable while sending paired RockCast voice sessions with a current
  native-session token.
- Result: all voice sessions use `/api/v1/voice/stream`; a persisted credential is renewed through
  `/api/v1/auth/device-session` and sent as Bearer when available. Renewal failures preserve the
  binding and use that same voice path anonymously.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed; external stream probes remain ignored.
- Status: **implemented locally.**

## RM-011 — 2026-08-30 — durable device-secret client sessions

- Goal: remove refresh-token rotation from RockCast so a transient renewal failure cannot disconnect
  a paired PC.
- Scope: replace the persisted refresh token with `device_id` and `device_secret`, request a short
  access token from `/api/v1/auth/device-session`, and revoke the device for explicit disconnect.
- Result: only an explicit `device_credential_invalid` response clears the local DPAPI credential;
  unavailable or malformed renewal responses preserve it.
- Checks: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test` passed.
- Status: local client implementation complete; matching RockServer contract remains required for E2E.

## RM-011 Wave 9 — A4 secure pairing handoff (complete, 2026-08-29)

- [x] Generate all existing QR/open/copy handoffs through the shared fragment-based link helper.
- [x] Keep the approval secret out of query URLs, UI text and diagnostics; retain it in memory
      only until normal pairing completion/cancellation.
- [x] Add a deterministic exact-shape test and pass Rust formatting, strict Clippy and tests.
- [ ] Production App Link association remains external to RockCast and was not deployed or claimed.

## RM-011 Wave 4 — C4–C8 account UX (complete, 2026-08-29)

- [x] Provide localized browser-approval steps, parsed-expiry countdown, secure-link open/copy
      actions, and a no-share copy warning; cancel/close stops only the local polling job.
- [x] Render a 256–320 logical-pixel QR with M correction, four quiet-zone modules, and integer
      module scale without logging or displaying the link payload.
- [x] Transition pairing success atomically to its own screen with primary device-centre and
      secondary done actions.
- [x] Keep the current PC first in the account centre; use local logout for it and confirmed
      remote disconnect only for other devices, with localized dates and distinct empty/unavailable
      device-list states.
- [x] Localize all account UI through `i18n::Strings` and `Lang`; account paths emit no URL,
      query, QR payload, short code, phrase, proof, token, or account/device identifier diagnostics.

## RM-011-G4 — clear PC connection UX (complete, 2026-08-28)

- [x] Call the action “Connect this PC to an account”; pair with a user-editable,
      server-validated default `RockCast — <PC name>`.
- [x] Use the G1 create/complete DTOs and the G2 request-specific browser link; show QR,
      fallback link action, short code, verification phrase, device name, status, expiry and cancel.
- [x] Show the approved account/device display names without rendering UUIDs, `user_id`, proofs or tokens.
- [x] Keep anonymous playback independent from account availability; timeout, cancellation,
      secure-storage failure and offline errors leave local radio available.
- [x] Connect the published native device list/revoke endpoints to Account & devices.
- [ ] Browser-side rename and richer device management remain the G3 surface; no new server API
      was invented here.
- [ ] Physical staging phone/passkey smoke test remains part of G7.

## RM-011-E — account and secure session UX (complete)

- [x] Create/complete native desktop pairing through the RM-011-C `/v1` contract.
- [x] Show a QR/deep link, short code and verification phrase; keep pairing proofs memory-only.
- [x] Store native credentials with Windows DPAPI, fail closed if protected storage fails,
      and support refresh, local cleanup/logout, device list and revoke.
- [x] Cover client requests with local mock HTTP tests; no live RockServer is used.
- [x] Finish pairing automatically using only the request ID and one-time desktop proof; the
      approved owner is derived by RockServer.

## MVP-001-C — official RockServer defaults

- [x] Use the production HTTPS RockServer base URL in official releases.
- [x] Remove RockServer URL/token requirements from ordinary user settings and UI.
- [x] Call public `/api/v1/search` and `/api/v1/voice/stream` without Bearer authorization.
- [x] Preserve HTTPS-to-WSS TLS and avoid legacy protected `/api/v1` aliases.
- [x] Isolate endpoint/token/voice-mode overrides to debug/test runtime without
      displaying or logging their values.
- [x] Preserve local catalog, Radio Browser fallback, and playback when the
      public API is unavailable.
- [ ] Reconcile published RockServer OpenAPI security metadata with the
      endpoint-level public allowlist in the RockServer repository, if still stale.

## Station icons

- [x] Add direct client-side favicon/logo loading for the MVP.
- [x] Keep HTTP, decoding, and cache work off the UI thread.
- [x] Add bounded response/image limits, HTTP(S)-only validation, safe cache
      keys, URL-based cache invalidation, and deterministic offline tests.
- [x] Preserve optional homepage/favicon metadata from RockServer search and
      voice responses.
- [ ] Populate the shared catalog with reviewed station favicon/logo metadata.
- [ ] Replace the direct-client source with the RockServer-hosted icon contract
      once server support lands.
