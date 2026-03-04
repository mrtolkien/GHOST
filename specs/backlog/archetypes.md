# Archetypes — removed, potential reintroduction

## What archetypes were

A note classification system with 11 types: person, concept, decision, event, place,
project, organization, procedure, media, quote, topic. Each note could optionally be
tagged with one archetype via the `note_write` tool and CLI.

## Why removed

Models couldn't select archetypes correctly:

- **Misclassification**: Products were labeled "project", companies labeled "concept",
  reviews labeled "media". The archetype field added noise rather than structure.
- **Topic index pollution**: The `topic` archetype encouraged the model to create
  meaningless index notes for every subfolder, cluttering the knowledge base.
- **Decision overhead without value**: The 11-way choice slowed note creation with no
  measurable benefit — tags and wiki links already provide categorization and graph
  structure.

## What a proper reintroduction needs

1. **Fewer archetypes**: 4-5 max, with clear boundaries (e.g. entity, decision,
   procedure, source).
2. **Few-shot examples**: Each archetype needs 2-3 concrete examples in the skill prompt
   showing correct classification.
3. **Validation feedback**: Tool response should warn when classification seems wrong
   (e.g. a "decision" note with no `[[compares>...]]` links).
4. **Post-hoc classification**: Instead of requiring the model to choose at creation,
   classify notes in a background agent pass where the full note content is available.
5. **Measurable benefit**: Only reintroduce if there's a concrete use case (e.g. "show
   me all decisions" query) that tags + links can't serve.
