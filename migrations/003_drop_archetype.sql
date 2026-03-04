-- Remove the archetype column from notes.
-- Models couldn't select archetypes correctly — added decision overhead without value.
ALTER TABLE note DROP COLUMN archetype;
