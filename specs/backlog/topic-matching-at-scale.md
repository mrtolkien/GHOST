# Topic Matching at Scale

## Problem

The reflection agent's first step is `fd -t d . notes/` to list all existing topic
folders. This works for a small knowledge base but won't scale to thousands of notes
across hundreds of topics.

## Current Behavior

- Agent runs `fd -t d . notes/` to discover folder structure
- Uses `knowledge_search` for individual entity duplicate checks
- Topic selection for new notes is based on visual scan of folder listing

## Future Approach

- Use `knowledge_search` with embedding similarity to find related topics
- Or provide a dedicated `list_topics` tool that returns topics with note counts
- Scoped listing: only show topics relevant to the current transcript's domain

## When to Address

When the notes directory exceeds ~50 topic folders or when reflection starts timing out
on the discovery step.
