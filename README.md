# hermo

A native Android client for a self-hosted Hermes instance. hermo is a third renderer over the
same JSON-RPC WebSocket backend that the Hermes TUI, Desktop app and dashboard already speak
(`hermes dashboard`, `/api/ws`). The phone talks to that protocol directly: no platform adapter,
no changes on the Python side.

## What it does today

- Pairing by scanning the QR code the host shows, or by typing host, port and password by hand.
- A chat screen that behaves like the TUI: streaming text, thinking labels, tool cards, approval
  and clarify prompts, slash commands.
- Multiple sessions as tabs. Switching tabs never reloads a session, closing one never deletes
  the durable chat, and the tab set survives an app restart.
- A bot drawer: every Hermes profile is one bot with one canonical chat (titled "Bot Chat"),
  created on first use, resumed afterwards on the profile that owns it.
- Light, dark or system theme.

## How it is put together

Most of the logic lives in a shared Rust crate (`hermes_core`): JSON-RPC framing, the WebSocket
client, cookie auth, the transcript reducer. UniFFI exposes the crate to Kotlin. The Android app
(`android/`, Jetpack Compose) is a thin renderer: it owns tab and drawer state and trusts the
change stream for everything else. Bazel builds the whole graph; Cargo compiles the Rust through
genrules.

## Status

Proof of concept, pre-alpha. Android first; iOS (SwiftUI + TCA) is planned later on the same
untouched core.

## Layout

```
hermes_core/   Rust core: JSON-RPC framing, WebSocket client, auth, transcript reducer (UniFFI)
android/       Jetpack Compose app, packaged as a Bazel android_binary
ios/           SwiftUI + TCA app, later phase, same core
scripts/       build and deploy helpers
```
