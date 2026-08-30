# RockCast status

## RM-011 device-secret native sessions (implemented locally, 2026-08-30)

RockCast now treats pairing as a durable device binding. The DPAPI-protected credential contains a
`device_id`, a persistent `device_secret`, and a replaceable access token. On an expired access
token it calls `POST /v1/auth/device-session`; network and server failures keep the binding, while
only `401 device_credential_invalid` clears it. The legacy refresh-token endpoint and rotating
refresh-token recovery path are no longer used by this client. This requires the corresponding
RockServer API change before end-to-end use. Verified: `cargo fmt --check`, strict Clippy, and
`cargo test` (94 unit tests plus non-live integration coverage) passed.

## RM-011-09 — Wave 9 A4 secure pairing handoff (complete locally, 2026-08-29)

RockCast now constructs its QR, copy and open-link payload through the existing `PairingRequest`
helper as `?code=<code>#secret=<proof>`. The one-time approval secret remains only in process
memory, is never shown as text or logged, and is no longer in the URL query. A deterministic unit
test locks the exact fragment shape.

Local checks passed: `cargo fmt --check`, strict `cargo clippy --all-targets --all-features -- -D
warnings`, `cargo test` (93 unit tests; live-network tests intentionally ignored), and final
`git diff --check`. No server/API/OpenAPI change, push, deploy, staging mutation or real pairing
flow occurred.

## RM-011 Wave 4 — C4–C8 account UX (complete, 2026-08-29)

The Account & devices dialog now renders one localized state at a time in both
Russian and English. The browser-approval screen explains the next steps,
shows an expiry countdown, and provides a primary secure-link action plus a
copy warning. Its QR is rendered with error correction M, a four-module quiet
zone, and integer-size modules in a 256–320 logical-pixel target; the link and
QR payload are never logged or shown as text.

Successful pairing moves atomically to a dedicated success screen, whose
primary action opens devices and whose secondary action closes the dialog.
The connected centre keeps the current PC first, gives it only local logout,
and offers confirmed disconnect only for other devices. It distinguishes an
empty list from an unavailable list, formats dates for the selected language,
and never renders identifiers, sessions, proofs, or tokens. Closing or
cancelling the waiting screen stops the local polling job; no server cancel
endpoint was added.

Local verification: `cargo fmt --check`, `cargo clippy --all-targets
--all-features -- -D warnings`, `cargo test`, and `git diff --check`.

## RM-011-G4 — clear PC connection UX (complete, 2026-08-28)

The Account & devices dialog now connects the current PC to an existing Rock account instead of
presenting RockCast as a separate registration. It starts the published G1 pairing request with
`device_display_name` and `device_type`, offers the default `RockCast — <PC name>` for editing,
and renders the G2 request-specific QR/deep-link fallback, short code, verification phrase,
expiry, status and cancel action.

Completion sends only the desktop proof and accepts the server-derived account/device display
context. The UI does not render UUIDs, `user_id`, device proofs or native tokens. It shows
`This PC is connected to account <account_display_name>` and the current device name, and uses
the published native device list/revoke endpoints. Polling stops on server errors, cancellation
or a bounded timeout; anonymous/offline playback remains independent. Browser rename and any
additional device-center operations remain the G3 browser dependency, and physical passkey/phone
acceptance remains G7.

Local verification for this change: `cargo fmt --check`, `cargo check --all-targets`, and
`cargo test --all-targets` (88 passed; live network probes remain ignored). No RockServer,
deployment, commit or push was made.

## RM-011-E — account and secure session UX (complete, 2026-08-26)

RockCast now has an optional Account & devices dialog. It creates a desktop pairing request via
the deployed `/v1/pairing-requests` contract, renders the one-time browser deep link as a QR code,
and displays the short code and verification phrase. The desktop proof and approval secret remain
only in process memory. Native access/refresh credentials are stored only in a Windows DPAPI
protected blob (`session.dpapi`); a DPAPI failure leaves RockCast anonymous/offline rather than
falling back to plaintext settings. The dialog supports silent refresh before profile/device reads,
remote logout followed by local cleanup, and owner-scoped device revoke. No token is shown or
written to logs.

RockCast polls completion automatically after browser/passkey approval. Its exact request body is
only `{ "desktop_token": "…" }`; it never asks for or sends a user ID, and the returned profile
is accepted only from the server. Local mock HTTP tests cover create/poll/complete, rejection of
the former extra `user_id`, refresh replay cleanup and offline logout cleanup; full `cargo test`
passed (85 unit tests and 2 local relay integration tests; 10 live-network tests remain ignored).
The Windows DPAPI calls compile on this host
but have not been exercised against a real Windows user profile in CI.

`cargo fmt --check` and `git diff --check` pass. Strict all-target Clippy reaches one pre-existing,
unrelated `clippy::too_many_arguments` diagnostic in `src/local/mod.rs::play` (8 arguments); no
new RM-011-E diagnostics remain.

## Station icons MVP

Implemented for the pre-RockServer-icon phase (2026-08-26).

- RockCast fetches a valid station `favicon_url` directly from the station's
  HTTP(S) server. If that field is absent, it may fetch the conventional
  `/favicon.ico` from the configured official `homepage_url`; it does not
  scrape homepage HTML.
- Fetching, bounded response reads, image decoding, and disk cache I/O run in
  the existing `BackgroundRuntime`, never on the egui thread.
- ICO, JPEG, and PNG payloads are accepted, bounded to 512 KiB on the wire and
  decoded to a maximum 64px thumbnail. Invalid, oversized, unsupported, or
  failed payloads keep the existing text-only station row.
- Successful thumbnails are cached in the platform app-data directory under
  `station-icons`. The cache filename is a safe hex-encoded station key and
  the stored source URL invalidates stale metadata. Requests are attempted at
  most once per station/source identity per app session.
- RockServer and voice station DTO adapters preserve optional `homepage` and
  `favicon` fields for this client-side MVP. No RockServer endpoint or database
  migration is part of this change.

The embedded catalog currently has no homepage/favicon metadata for its
stations, so those rows intentionally remain text-only until catalog or
RockServer metadata supplies a permitted source URL.

## MVP-001-C — zero-configuration official RockServer client

Implemented and locally verified on 2026-08-26.

- Official releases use `https://alex.vault57.ru` without user configuration.
- Public search uses `POST /v1/search` without Bearer authorization. Voice
  preserves TLS by mapping HTTPS to WSS and uses `/v1/voice/stream`, also
  without Bearer authorization.
- RockServer URL/token controls and persisted RockServer settings were removed.
  Legacy JSON fields are ignored and scrubbed during settings migration.
- Endpoint, optional Bearer token, and streaming-mode overrides exist only for
  debug/test runtime through `ROCKCAST_DEV_ROCKSERVER_*`; release builds ignore
  them, and their values are neither displayed nor logged.
- The embedded catalog is delivered before the public request. A failed or
  empty public response continues through the existing local catalog + Radio
  Browser path, so local selection and playback do not depend on RockServer.

The client follows the deployed RockServer runtime contract from MVP-001-B.
Legacy `/api/v1` aliases remain intentionally unused because they are
Bearer-protected. No RockServer or OpenAPI repository was changed here. If the
published OpenAPI still applies global Bearer security to these allowlisted
`/v1` operations, that documentation/runtime mismatch remains an external
contract-documentation issue, not a reason for the client to send a token.
