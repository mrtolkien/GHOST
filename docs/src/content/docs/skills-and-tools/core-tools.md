---
title: Core Tools
description:
  GHOST's core tool capabilities — shell, file I/O, and todo tracking.
---

Shell access, file operations, and task tracking.

## `run_shell_command`

Execute shell commands on the host system.

| Parameter    | Type    | Required | Description                                                            |
| ------------ | ------- | -------- | ---------------------------------------------------------------------- |
| `command`    | string  | yes      | Shell command to execute                                               |
| `timeout_ms` | integer | no       | Timeout in milliseconds (default 30000). Ignored when `background=true` |
| `directory`  | string  | no       | Working directory (relative to workspace)                              |
| `background` | boolean | no       | Run with no timeout; result delivered as a system message              |

When `background` is true, the tool returns immediately and the command runs
detached. On completion, the output is posted as a `[shell-command completed]`
system message into the session. GHOST sees this message on the next
conversation turn.

## `read_file`

Read file contents with optional pagination.

| Parameter | Type    | Required | Description             |
| --------- | ------- | -------- | ----------------------- |
| `path`    | string  | yes      | File path               |
| `offset`  | integer | no       | Starting line number    |
| `limit`   | integer | no       | Number of lines to read |

Returns contents with line numbers for precise editing.

## `write_file`

Create or overwrite a file.

| Parameter | Type   | Required | Description   |
| --------- | ------ | -------- | ------------- |
| `path`    | string | yes      | File path     |
| `content` | string | yes      | File contents |

## `file_edit`

Make targeted string replacements in a file.

| Parameter    | Type   | Required | Description      |
| ------------ | ------ | -------- | ---------------- |
| `path`       | string | yes      | File path        |
| `old_string` | string | yes      | Text to find     |
| `new_string` | string | yes      | Replacement text |

## `todo`

Session-scoped working memory for tracking multi-step tasks.

| Action         | Description                   |
| -------------- | ----------------------------- |
| `plan`         | Create a TODO list with items |
| `update`       | Update a single item's status |
| `batch_update` | Update multiple items at once |

