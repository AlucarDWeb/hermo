# hermo

hermes mobile app proof of concept

A native phone client for a self-hosted Hermes instance: one chat screen that behaves like the
TUI — streaming text, thinking, tool cards, approvals, clarify prompts, slash commands, session
switching — paired by scanning a QR code shown on the host.

The app is a third renderer over the JSON-RPC WebSocket backend that the Hermes TUI, Desktop and
dashboard already share (`hermes dashboard`, `/api/ws`). It speaks that protocol directly: no
platform adapter, no Python-side changes.

## Status

Proof of concept, pre-alpha. Android first; iOS is planned on the same Rust core.

## Layout

```
hermes_core/   Rust core: JSON-RPC framing, WebSocket client, auth, transcript reducer (UniFFI)
android/       Jetpack Compose app, packaged as a Bazel android_binary
ios/           SwiftUI + TCA app — later phase, same core
scripts/       build and deploy helpers
```

Build graph is Bazel; Cargo compiles the Rust through genrules. The crate and the app code do not
depend on that choice.

## Design notes

Planning and design documents are intentionally kept out of this repository.
