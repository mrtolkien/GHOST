CREATE TABLE agent_state (
    agent_slug TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (agent_slug, key)
);
