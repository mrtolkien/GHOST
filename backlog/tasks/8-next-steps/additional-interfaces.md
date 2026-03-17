# Backlog — Additional Interfaces

## Overview

Add communication interfaces beyond Discord.

## Planned Interfaces

### WebSocket API

- Generic WebSocket interface for custom clients
- JSON message protocol
- Enables building custom UIs (web, mobile, desktop)
- Will be accessed 100% through Tailscale + will need some security

### ?Telegram Bot?

- Alternative to Discord for users who prefer Telegram
- Similar message model (channels, DMs, groups)
- Rich markdown support

### ?Slack Bot?

- For workplace/team use
- Thread-based conversations map well to sessions
- Slash command integration

### ?Matrix?

- Open-source, self-hosted alternative
- E2EE support for privacy-sensitive use

### CLI Chat

- `ghost chat` — Interactive CLI chat mode?

### ?Other Ideas?

If there's a way to check what the most popular interfaces are for OpenClaw, we'll start
with it.

## Architecture Note

All interfaces are thin transport layers. They:

1. Receive messages from their platform
2. Validate the sender
3. Pass to `SessionChat::chat()`
4. Return the response to the platform

They do NOT manage tools, build chat history, or talk to providers.
