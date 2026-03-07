# Backlog — Terminal UI (TUI)

## Overview

A rich terminal interface for chatting with the GHOST, managing knowledge, viewing job
logs, and configuring settings. Built with `ratatui`.

## Why Deferred

- The PoC focuses on CLI commands and Discord
- TUI is a significant UI effort that doesn't add core functionality
- CLI commands cover all features (the TUI would be a nicer interface to the same
  operations)

## Planned Features

- Chat interface with markdown rendering
- Knowledge browser (search, view, edit notes)
- Job log viewer with filtering
- Config editor
- Session manager
- Real-time job status indicators

## Architecture

The TUI would connect to the daemon via WebSocket (requires the WebSocket API from
`additional-interfaces.md`). This keeps the TUI as a thin client.

## Dependencies

- `ratatui` + `crossterm`
- WebSocket client (tokio-tungstenite)
