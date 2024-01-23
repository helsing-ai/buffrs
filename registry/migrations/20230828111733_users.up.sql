-- table of users.
CREATE TABLE users (
    id          SERIAL PRIMARY KEY,
    handle      TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT (now()),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT (now()),
    -- when users are deleted, we keep the row, but set the deleted_at field.
    deleted_at  TIMESTAMPTZ
);

CREATE TYPE scope as ENUM('publish', 'yank', 'update');

CREATE TABLE tokens (
    hash       TEXT PRIMARY KEY,
    "user"      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scopes     scope[] NOT NULL DEFAULT array[]::scope[],
    created_at TIMESTAMPTZ NOT NULL DEFAULT (now()),
    expires_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

-- view showing only active users
CREATE VIEW users_active AS
    SELECT *
    FROM users
    WHERE deleted_at IS NULL;

-- view showing only active tokens
CREATE VIEW tokens_active AS
    SELECT
        tokens.*,
        users.handle
    FROM users_active users
    JOIN tokens on tokens.user = users.id
    WHERE tokens.expires_at > now()
    AND tokens.deleted_at IS NULL;
