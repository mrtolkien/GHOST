---
title: Web Tools
description: Tools for web research — search and page fetching.
---

Tools for web research. See [Web Research](/web-research/overview/) for usage details.

## `web_search`

Search the web using the configured search provider. SearXNG is the default;
Brave Search is available as a fallback. See
[configuration](/web-research/overview/#web-search) for setup.

| Parameter     | Type    | Required | Description                                  |
| ------------- | ------- | -------- | -------------------------------------------- |
| `query`       | string  | yes      | Search query                                 |
| `max_results` | integer | no       | Number of results. Default: config value (5) |

When using SearXNG, results include extra metadata: source engines, ranking
positions, and an aggregated relevance score.

## `web_fetch`

Fetch and extract readable content from a URL.

| Parameter     | Type    | Required | Description                                    |
| ------------- | ------- | -------- | ---------------------------------------------- |
| `url`         | string  | yes      | Page URL                                       |
| `readability` | boolean | no       | Use Mozilla Readability for article extraction |
| `raw`         | boolean | no       | Return raw HTML                                |
