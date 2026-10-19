---
title: "Initial Stack Decision"
type: operational
status: living
tags: [operational, current-stack]
---

# Initial Stack Decision

## Current proposal

- Language: Rust
- RPC: gRPC over HTTP/2
- Schema: Protocol Buffers
- CLI: clap
- TUI: ratatui (Phase H)

These are implementation choices for the first prototype, not irreversible architectural contracts.

Current upstream documentation confirms tonic 0.14.x provides gRPC over HTTP/2 for Rust, prost 0.14.x provides Protocol Buffers code generation/types, clap 4.6.x is the current documented major line, and Ratatui 0.30.x is the current documented release line at the time of this specification.
