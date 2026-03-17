-- Re-introduce archetype column (dropped in 003). Now 5 types with prompt guidance.
ALTER TABLE note ADD COLUMN archetype TEXT;
CREATE INDEX IF NOT EXISTS idx_reference_source_url ON reference(source_url);
