# RockCast tasks

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
- [x] Call public `/v1/search` and `/v1/voice/stream` without Bearer authorization.
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
