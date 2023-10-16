CREATE TABLE users (
    id            SERIAL PRIMARY KEY,
    -- metadata
    name          TEXT,
    email         TEXT,
    avatar        TEXT,
    -- static user token here
    handle        TEXT NOT NULL UNIQUE,
    -- timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT (now()),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT (now()),
    deleted_at TIMESTAMPTZ
);

CREATE TABLE user_tokens (
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token         TEXT NOT NULL UNIQUE
);
