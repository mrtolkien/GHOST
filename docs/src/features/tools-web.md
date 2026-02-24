# Web Tools

Tools for web research. See [Web Research](web.md) for usage details.

## web_search

Search the web via the Brave Search API.

| Parameter     | Type    | Required | Description                                  |
| ------------- | ------- | -------- | -------------------------------------------- |
| `query`       | string  | yes      | Search query                                 |
| `max_results` | integer | no       | Number of results. Default: config value (5) |

Requires `BRAVE_API_KEY` environment variable.

## web_fetch

Fetch and extract readable content from a URL.

| Parameter     | Type    | Required | Description                                    |
| ------------- | ------- | -------- | ---------------------------------------------- |
| `url`         | string  | yes      | Page URL                                       |
| `readability` | boolean | no       | Use Mozilla Readability for article extraction |
| `raw`         | boolean | no       | Return raw HTML                                |
