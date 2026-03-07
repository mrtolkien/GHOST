---
name: sending-attachments
description:
  Use when you need to send an image, generated file, CSV, or any
  attachment to the OPERATOR.
---

# Sending Attachments

Send files to the OPERATOR. Session and channel are detected automatically
from environment — no IDs needed.

## Commands

**Image:**

    ghost send-image $WORKSPACE/tmp/my-image.png

**Image with caption:**

    ghost send-image $WORKSPACE/tmp/my-image.png --caption "Here's the chart"

**Any file:**

    ghost attach $WORKSPACE/tmp/data.csv

## When NOT to use

- Small data that fits in a message — just write it inline
- Code snippets — use markdown code blocks instead
