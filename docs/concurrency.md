# Concurrency and cancellation

## Incident: Play/Stop made the window unresponsive

The UI freeze was caused by `StreamObservers::stop`: both observer implementations moved their
`JoinHandle` to a helper thread and then synchronously waited up to two seconds for that helper.
Play and Stop call both observers from the egui thread, so an ICY read plus a spectrum read could
block Windows message processing for about four seconds.

There was a second, independent serialization bug in the background runtime. In
`match rx.lock().recv()`, Rust kept the temporary mutex guard alive until the end of the complete
`match`, including execution of the job arm. A blocking job therefore prevented every other worker
from receiving work. The receive now happens in its own scope, so the guard is dropped before the
job starts. A regression test blocks one worker and verifies that a second worker still completes.

A live GUI run later exposed the same temporary-guard lifetime pattern in device-control state
publication: the connection worker retained `state.lock()` through an `if let` body and called
`send_full`, which tried to lock `state` again. The first playback state change therefore
self-deadlocked the connection worker and then blocked egui in `publish`. State is now cloned in a
separate scope before the send decision. The settings writer uses the same explicit extraction so
it never holds its pending-slot mutex during filesystem I/O. All analogous LocalPlayer error-slot
conditions also extract their value before entering the branch.

## Current model

- The egui thread owns `RockCastApp` and `PlaybackController`. It never waits for observer, network,
  decoder, Cast, or settings I/O during ordinary interaction.
- `rockcast-playback-*` is a three-worker executor reserved for playback transitions and volume.
- `rockcast-io-*` is a four-worker executor for catalog, icons, account, voice, and discovery work.
- Both executors have bounded queues. Submitting work never blocks the UI.
- Every Play owns an immutable per-generation cancellation token. A later Play, Stop, or shutdown
  can only change that token from active to cancelled; no worker can reset an older cancellation.
- A playback transition mutex linearizes cross-output teardown/start. Waiting operations are
  cancelled before they wait for this mutex, so a long Cast or local operation can relinquish it.
- ICY and spectrum Stop only set their stop flag and detach the old worker. The worker owns its
  resources and exits on the flag or a bounded network timeout.
- Settings are written by one worker through a single latest-value slot and a one-item wake queue.
  Rapid UI changes coalesce; shutdown uses a bounded flush.

## Required invariants

1. Check both generation and its cancellation token after acquiring a transition/device lock and
   before any side effect.
2. Never set a cancellation token back to `false`; create a new token for a new generation.
3. Never call `join`, blocking `recv`, HTTP, Cast I/O, `sync_all`, or a device scan from egui.
4. Playback work must not use the general I/O executor.
5. Relay pre-buffer waits must observe both operation cancellation and relay-session stop.
6. Do not hold the device-command ledger mutex while sending a WebSocket frame.
7. Keep queues bounded and cap remote-command work per frame.

## Regression checks

- Two blocking runtime jobs must start concurrently.
- Observer Stop must return well before its reader thread exits.
- Superseded playback events and operations must not alter the current generation.
- Taking the device-control state snapshot must release its mutex before `send_full`.
- A full device-command queue must reject additional unique commands without blocking egui.
