CREATE TABLE coding_sessions (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    channel_id  TEXT,
    working_dir TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active',
    started_at  TEXT NOT NULL,
    ended_at    TEXT
);

CREATE INDEX idx_coding_sessions_channel ON coding_sessions(channel_id)
    WHERE status = 'active';
CREATE INDEX idx_coding_sessions_status ON coding_sessions(status);
