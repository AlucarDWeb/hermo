# Hermo

Native Android and iOS clients for a self-hosted Hermes instance. hermo is a third renderer over
the same JSON-RPC WebSocket backend that the Hermes TUI, Desktop app and dashboard already speak
(`hermes dashboard`, `/api/ws`). The phone talks to that protocol directly: no platform adapter,
no changes on the Python side.

## What it does today

Both clients render the same transcript from the same core, so this section describes either one.

Pairing happens once. You scan the QR code the host shows or type host, port and password by hand. After that the app
reconnects on its own, and when you want a different gateway there is a log out path from the
drawer, from the password sheet and from the offline screen.

The chat screen behaves like the TUI. Text streams in as the agent writes it, thinking rows
show the reasoning live and collapse to a label when the answer settles, tools appear as cards
with their arguments and results, and approval or clarify prompts land as answerable cards.
The composer carries the current model name, offers a slash command popup with completion, and
dismisses the keyboard when you send.

Sessions are tabs. A tab takes the real session title and you can rename it from the title
bar. A picker lets you reopen past chats, closing the last tab mints a fresh blank one, and
the open tab set survives an app restart because it lives in the session registry on disk.
A hamburger drawer lists every Hermes profile as a bot; tapping one opens that profile's
canonical chat (titled "Bot Chat"), created on first use and resumed afterwards on the profile
that owns it.

Theme follows the system by default, with a persisted light/dark/system selector. When the
network drops you get an offline screen with retry, plus a destructive "reset sessions" escape
for when the local state is beyond repair.

## How it is put together

Most of the logic lives in a shared Rust crate, `hermes_core`:

- JSON-RPC framing over newline-delimited WebSocket frames, with the 15 s heartbeat and the
  45 s dead-socket detection the Desktop client uses.
- Auth against the dashboard `basic` provider: password login, a cookie jar persisted on disk
  (12 h access, 30 day rotating refresh, so re-login is rare), and single-use WebSocket
  tickets for the socket upgrade.
- One transcript reducer per open chat, fed by the server event stream. The reducer emits
  indexed row changes and those changes are the single source of rows for the UI: there is no
  client-side echo and no local snapshot replay, which is what keeps a resumed transcript from
  doubling or collapsing.
- A session registry on disk: durable session ids, per-session replay watermarks, the open tab
  set. This is why tabs and transcripts survive a restart.
- Reconnect: on socket loss the core resumes every open session, fills the gap from the event
  log, rebuilds the transcript if the server truncated it, and replays any approval that is
  still pending.

UniFFI exposes the crate to Kotlin and to Swift as one `HermesCore` object plus an `EventSink`
callback the core pushes changes through. The Android app (`android/`, Jetpack Compose) is
deliberately thin: it owns tab and drawer state, routes connection phases (unpaired, password,
connecting, ready, offline) and renders whatever the change stream delivers. Bazel builds the
whole graph, with Cargo compiling the Rust through genrules into the Android ABIs and into the
iOS device, iOS simulator and macOS static archives.

The iOS app (`ios/`, SwiftUI with the Composable Architecture) splits the same job across five
modules:

- `ios/Sources/HermesCore` — the generated UniFFI Swift bindings over the Rust archives.
- `ios/Sources/HermoLogic` — the state machines, policies, row shapes and formatters ported
  one to one from the Kotlin files, with no dependencies at all.
- `ios/Sources/HermoGateway` — the `GatewayClient` dependency wrapping the core: connect,
  resume, send, the event stream, and the error mapping that turns a `CoreError` back into the
  Rust message a user should read.
- `ios/Sources/HermoFeatures` — the TCA reducers (`AppFeature`, `ChatFeature`, the session
  picker, the bot drawer, the appearance sheet).
- `ios/Sources/HermoUI` — the SwiftUI views, Liquid Glass chrome over the Desktop layout.

The first four import no SwiftUI, which is what lets the whole suite run on the macOS host in
about a second with no simulator, and `HermoUI` takes plain values and closures rather than a
`Store`, so the store wiring stays in `ios/App`.

The UI copies Hermes Desktop: same tokens, same row shapes, same interaction. The few places
where the phone diverges on purpose (bigger type, a drawer instead of the Desktop sidebar,
thinking rows that do not keep their preview after settling) are tracked as declared
divergences, not accidents.

## Layout

```
hermes_core/   Rust core: codec, WebSocket client, auth, per-session reducer, session registry (UniFFI)
android/       Jetpack Compose app, packaged as a Bazel android_binary
ios/           SwiftUI + TCA app (App, Sources, Tests), packaged as a Bazel ios_application
tools/         dev-only crates outside the shipped core, currently the fake gateway
scripts/       build, device install and host QR helpers
```

## Scripts

```
scripts/simulator_install.sh        build the Android app and deploy it to a booted emulator
scripts/device_install.sh           build the Android app and install it on the USB device
scripts/ios_simulator_install.sh    build the iOS app and deploy it to a booted simulator
                                    (--run launches it, --screenshot writes a PNG,
                                     NO_SHUTDOWN=1 keeps the Bazel server alive)
scripts/fake_gateway.sh             start|stop|status for the recorded stand-in gateway
scripts/hermes-qr.sh                render the hermes://connect pairing QR for a gateway
scripts/make_debug_keystore.sh      generate the Android debug signing keystore
scripts/record_fixture.sh           re-record the protocol fixture against a throwaway backend
```
