---
title: Onboarding
description: What ghost init does and how to use it.
---

`ghost init` is an interactive wizard that configures everything your GHOST
needs to run. It replaces manual config file editing with guided prompts.

## What It Does

The wizard runs in 5 phases:

1. **Environment detection** — checks for Nix, container runtime
   (podman/docker), existing services, available memory
2. **LLM provider** — pick a provider (OpenRouter, Anthropic, Kimi, or
   ChatGPT OAuth), enter credentials, choose a model, and validate the
   connection with a real API call
3. **Discord** — step-by-step guide to create a bot, enter the token and
   your user ID, validated against the Discord API
4. **Services** — for each service (embeddings, web search, web fetch,
   document processing): choose local install, container, remote URL, or
   skip. Installs nix packages and generates a compose file
5. **Write + launch** — shows a config diff, writes `config.toml` + `.env`,
   bootstraps the workspace, installs systemd/launchd units, starts
   services, runs health checks, and sends the first Discord message

## Non-Interactive Mode

Every prompt has a corresponding CLI flag. Pass all flags to skip prompts
entirely — useful for automated deployments and testing:

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

Partial flags work too — the wizard prompts for anything not provided.

## Re-running

Run `ghost init` again at any time. It detects your existing configuration,
offers to update or start fresh, and pre-fills prompts with current values.
A diff of all changes is shown before writing.

## Help During Setup

After the provider step validates successfully, type **h** at any text
prompt to chat with an onboarding assistant. It uses your configured LLM to
answer questions about services, tradeoffs, and configuration — then
returns you to the wizard.
