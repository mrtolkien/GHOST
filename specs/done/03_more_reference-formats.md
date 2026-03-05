# Backlog — Complex Reference Format Ingestion

## Overview

The PoC knowledge system handles markdown text. Real-world references come in richer
formats that need extraction and indexing: PDFs, images, and large structured data
files.

We will use docling as it covers a lot of the spec by accepting pdf, xlsx, docx, even
mp3, ...

## User Story

### Ark Nova

#### Step 1

The user asks a rules question about the board game Ark Nova. The rules are available
online, but only as a PDF.

The GHOST might read a skill, finds the PDF and imports it, then reads a Markdown
version of the content to answer the user's questions.

If the user asks for it again in the future, it can directly re-read the rules or find
them through embeddings by doing "knowledge_search('Ark Nova Rules')" which should
surface either the reference directly, or maybe a note that links to the reference (for
example an #board-game note called Ark Nova could link to the rules in its "sources"
field)

#### Step 2

The user then asks about a specific card: Baboon Rock.

The GHOST might read a skill, then searches for the full list of cards. It finds a
project in Typescript holding all the data:
https://github.com/Ender-Wiggin2019/Next-Ark-Nova-Cards/tree/main/src/data

It downloads the data (with reference_import), puts it in the proper reference topic,
and uses it to respond.

This might be already covered by our existing tools and skills, but it should be part of
an e2e step after the first part.

### Nobel prizes

The user asks about the list of French nobel laureates.

The GHOST discovers the Nobel Prize API and wants to make a query.

It uses our reference import instead of a naked curl so the reference is well organized
and disoverable.

## Design Considerations

- We should likely extend the reference import CLI tool as well as the skill associated
  with it
- Ingestion should produce standard markdown references that the existing knowledge
  system can index and search — the format-specific logic is a preprocessing step, not a
  new knowledge type
- Original files should be preserved alongside extracted content for re-processing when
  extraction improves
