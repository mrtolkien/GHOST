---
title: Web Research
description: Web search and page fetching capabilities for GHOST research tasks.
---

GHOST can search the web and fetch page content for research tasks.

## Web Search

GHOST supports two search backends:

- **[SearXNG](https://docs.searxng.org/)** (default) — self-hosted
  meta-search engine. Aggregates results from multiple engines and returns
  rich metadata (source engines, relevance scores, ranking positions).
- **[Brave Search](https://brave.com/search/api/)** — API-based fallback.
  Requires a subscription key.

### Configuring SearXNG

Point GHOST at your SearXNG instance via config or environment variable:

```toml title="~/.config/ghost/config.toml"
[web.search]
provider = "searxng"
url = "http://localhost:8888"
```

Or set the `SEARXNG_URL` environment variable — GHOST will pick it up
automatically without any TOML config.

### Configuring Brave (fallback)

If you don't run a SearXNG instance, set a Brave API key instead:

```toml title="~/.config/ghost/config.toml"
[web.search]
provider = "brave"
```

:::caution
Requires the `BRAVE_API_KEY` environment variable.
:::

### CLI

```bash
ghost web search "rust error handling best practices"
ghost web search "latest AI news" -n 10
```

## Web Fetch

Extracts readable content from web pages. Supports two modes:

- **Readability** — article extraction (Mozilla Readability algorithm)
- **Raw** — full HTML output

```bash
ghost web fetch https://example.com
ghost web fetch https://example.com --readability
ghost web fetch https://example.com --raw
```

:::tip
Optionally uses [Crawl4AI](https://github.com/unclecode/crawl4ai) as a
backend when standard web fetching doesn't yield good results.
:::

```toml title="~/.config/ghost/config.toml"
[web]
crawl4ai_url = "http://localhost:11235"
```

## Caching

All web fetches are automatically cached to `$WORKSPACE/.web-cache/`. The reflection
agent triages those artifacts when it runs, creating sources linked to the fetched data.

## How GHOST Uses Web Tools

During conversation, your GHOST proactively:

1. Searches the web when current information is needed
2. Fetches full pages to read beyond search snippets
3. Caches results for later reference
4. Cites sources in responses

The **deep-research** agent takes this further — it performs iterative multi-source
research, fetching multiple full pages before drawing conclusions.
