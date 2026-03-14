ALTER TABLE note ADD COLUMN file_hash TEXT;
ALTER TABLE reference ADD COLUMN file_hash TEXT;
ALTER TABLE diary ADD COLUMN file_hash TEXT;
ALTER TABLE script ADD COLUMN file_hash TEXT;
