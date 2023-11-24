CREATE TYPE package_type AS ENUM('library', 'api');

CREATE TABLE packages (
    id         SERIAL PRIMARY KEY,
    -- metadata
    name       TEXT NOT NULL,
    type       package_type NOT NULL, 
    -- timestamps
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE package_owners (
    id         SERIAL PRIMARY KEY,
    -- references
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    package_id INTEGER NOT NULL REFERENCES packages(id) ON DELETE RESTRICT,
    created_by INTEGER REFERENCES users(id) ON DELETE RESTRICT,
    -- timestamps
    created_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE package_invites (
    id SERIAL PRIMARY KEY,
    token TEXT NOT NULL UNIQUE,

    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    package_id INTEGER NOT NULL REFERENCES packages(id) ON DELETE RESTRICT,
    created_by INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,

    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
