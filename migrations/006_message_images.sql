-- Add images column to message table for storing image content blocks.
-- JSON array of {path, mime_type, filename} objects.
ALTER TABLE message ADD COLUMN images TEXT;
