-- Script knowledge type: executable artifacts the GHOST writes and reuses.

CREATE TABLE script (
    id         TEXT PRIMARY KEY NOT NULL,
    path       TEXT NOT NULL UNIQUE,  -- relative to workspace: scripts/finance/spending.py
    content    TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE script_fts USING fts5(
    path,
    content,
    content=script,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

CREATE TRIGGER script_fts_ai AFTER INSERT ON script BEGIN
    INSERT INTO script_fts(rowid, path, content)
    VALUES (new.rowid, new.path, new.content);
END;

CREATE TRIGGER script_fts_ad AFTER DELETE ON script BEGIN
    INSERT INTO script_fts(script_fts, rowid, path, content)
    VALUES ('delete', old.rowid, old.path, old.content);
END;

CREATE TRIGGER script_fts_au AFTER UPDATE ON script BEGIN
    INSERT INTO script_fts(script_fts, rowid, path, content)
    VALUES ('delete', old.rowid, old.path, old.content);
    INSERT INTO script_fts(rowid, path, content)
    VALUES (new.rowid, new.path, new.content);
END;
