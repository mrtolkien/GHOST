---
title: Core Tools
description: GHOST's core tool capabilities — shell, file I/O, todo tracking, and agent control.
---

GHOST follows a "skills over tools" philosophy — prefer workflow files and existing
tools over adding new tool APIs. These core tools cover most needs.

## run_shell_command

Execute shell commands on the host system.

| Parameter    | Type    | Required | Description              |
| ------------ | ------- | -------- | ------------------------ |
| `command`    | string  | yes      | Shell command to execute |
| `timeout_ms` | integer | no       | Timeout in milliseconds  |

## read_file

Read file contents with optional pagination.

| Parameter | Type    | Required | Description             |
| --------- | ------- | -------- | ----------------------- |
| `path`    | string  | yes      | File path               |
| `offset`  | integer | no       | Starting line number    |
| `limit`   | integer | no       | Number of lines to read |

Returns contents with line numbers for precise editing.

## write_file

Create or overwrite a file.

| Parameter | Type   | Required | Description   |
| --------- | ------ | -------- | ------------- |
| `path`    | string | yes      | File path     |
| `content` | string | yes      | File contents |

## file_edit

Make targeted string replacements in a file.

| Parameter    | Type   | Required | Description      |
| ------------ | ------ | -------- | ---------------- |
| `path`       | string | yes      | File path        |
| `old_string` | string | yes      | Text to find     |
| `new_string` | string | yes      | Replacement text |

## todo

Session-scoped working memory for tracking multi-step tasks.

| Action         | Description                   |
| -------------- | ----------------------------- |
| `plan`         | Create a TODO list with items |
| `update`       | Update a single item's status |
| `batch_update` | Update multiple items at once |

## agent_control

Spawn and manage autonomous agents.

| Action     | Description                            |
| ---------- | -------------------------------------- |
| `start`    | Spawn a new agent with name and prompt |
| `continue` | Send follow-up instructions            |
| `status`   | Check agent progress                   |
| `stop`     | Terminate an agent                     |

See [Agents](/features/agents/) for details.
