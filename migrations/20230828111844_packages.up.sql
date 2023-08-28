CREATE TYPE package_type AS ENUM('library', 'api');

CREATE TABLE packages (
    id         SERIAL PRIMARY KEY,
    -- metadata
    name       TEXT NOT NULL,
    type       package_type NOT NULL, 
    -- timestamps
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
);

CREATE TABLE package_owners (
    id         SERIAL PRIMARY KEY,
    -- references
    user_id    INTEGER NOT NULL FOREIGN KEY ON(users) ON DELETE RESTRICT,
    package_id INTEGER NOT NULL FOREIGN KEY ON(packages) ON DELETE RESTRICT,
    created_by INTEGER FOREIGN KEY ON(users) ON DELETE RESTRICT,
    -- timestamps
    created_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);
