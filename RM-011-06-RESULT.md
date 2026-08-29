# RM-011 Wave 4 — result

## Implementation

Implementation commit: `08c9cc68ff3664d8b195027a477f19b654fc7f46`

Completed the isolated RockCast C4–C8 scope:

- C4: browser-approval waiting screen with localized steps, parsed-expiry countdown, secure-link
  open/copy actions, no-share copy warning, local cancel/close, and a 256–320 logical-pixel QR
  using M correction, a four-module quiet zone, and integer module scale.
- C5: successful pairing transitions atomically to `ConnectedFirstTime`; its primary action opens
  devices and its secondary action closes the dialog. Waiting QR, connect form, and error UI are
  not retained by the state transition.
- C6: connected account centre puts the current PC first, exposes only local logout for it, and
  offers confirmed revoke only for other devices. Empty and unavailable lists differ; dates are
  formatted for the selected RU/EN language; identifiers, session fields, and tokens are not drawn.
- C7: all account UI text now comes from the existing `i18n::Strings` and selected `Lang`.
- C8: the account/pairing path adds no diagnostics and does not log URLs, query strings, QR/link
  content, short codes, phrases, proofs, tokens, or account/device identifiers.

Changed files:

- `src/app/actions/poll.rs`
- `src/app/mod.rs`
- `src/app/ui/account.rs`
- `src/i18n.rs`
- `docs/status.md`
- `docs/tasks.md`

## Verification

Passed locally:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
git diff --check
```

`cargo test`: 93 passed; live-network tests remain intentionally ignored.

## Boundaries

- No RockMobile, RockServer, OpenAPI, staging, deployment, account/device data, or handoff
  URL/fragment contract was changed.
- No server cancel endpoint, dependencies, push, or deploy was added.
- Physical passkey/browser and camera QR scans remain manual staging checks and were not run.
