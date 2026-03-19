---
title: Onboarding
description: Set up your GHOST with the interactive setup wizard.
---

After installing the GHOST binary, run the onboarding wizard to configure
everything:

## Quick Start

```sh
ghost init
```

The wizard walks you through:

1. **LLM provider** — pick a provider, enter your API key, choose a model
2. **Discord** — create a bot and connect it to your server
3. **Services** — set up embeddings, web search, web fetch, and document
   processing (locally or remotely)

At the end, your GHOST starts and sends you a message on Discord.

## Non-Interactive Mode

For automated deployments, pass all options as flags:

```sh
ghost init \
  --provider openrouter \
  --api-key "$OPENROUTER_API_KEY" \
  --model "anthropic/claude-sonnet-4" \
  --context-window 200000 \
  --discord-token "$DISCORD_BOT_TOKEN" \
  --discord-user "$DISCORD_USER_ID" \
  --embeddings local \
  --search local \
  --crawl local \
  --docling local \
  --start
```

## Re-running

Run `ghost init` again at any time to reconfigure. It detects your
existing configuration and offers to update it — showing a diff of all
changes before applying.

## Need Help During Setup?

Press **h** at any prompt to ask the onboarding assistant for help. It
uses your configured LLM to answer questions about the setup process
(available after the provider step completes).

## Next Steps

- [Services](/getting-started/services/) — how the service stack works
  and how your GHOST manages it
- [Configuration](/getting-started/configuration/) — config.toml and
  .env reference
- [Workspace](/getting-started/workspace/) — what's in your GHOST's
  workspace directory
