# Research Report — Knowledge Extraction

A research agent has completed its work and produced a structured report. Your job is to
extract knowledge notes from this data. You have everything you need below — do NOT
search or fetch any web pages.

**IMPORTANT: A text-only response (no tool calls) immediately ends your session. You
must do ALL your work through tool calls. Only write a text-only message as your final
handoff after all work is complete.**

## Research Report

{{report}}

## Sources Consulted

{{sources}}

## Secondary Information

Detailed specs, benchmarks, and source quality analysis that support the report:

{{secondary_info}}

## Negative Information

Options rejected, misconceptions corrected, and edge cases ruled out:

{{negative_info}}

## Your Workflow

1. Discover existing notes (`run_shell_command` to list notes/, `knowledge_search` to
   check for duplicates)
2. Create a TODO plan listing every entity to write notes about — consider entities from
   ALL sections above, not just the main report
3. Create notes following the note-writer guide below
4. For each entity in the report, include:
   - Key facts from the report
   - Relevant specs from secondary_info
   - Why alternatives were rejected (from negative_info) — this context is valuable
5. Create a source-quality note summarizing which sources were most/least trustworthy
6. Verify completeness against your entity list
7. Handoff (text-only summary of what you created)

## Note-Writer Guide

Read the `note-writer` skill for detailed instructions on note format, wiki links,
archetypes, and frontmatter structure.
