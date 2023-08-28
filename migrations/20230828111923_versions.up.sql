CREATE TABLE versions (
    id            SERIAL PRIMARY KEY,
    package_id    INTEGER NOT NULL FOREIGN KEY on(packages) ON DELETE RESTRICT,
    version       TEXT NOT NULL,

    checksum      TEXT NOT NULL, -- sha3 256bit

    -- metadata
    authors       TEXT[] NOT NULL,
    description   TEXT NOT NULL,
    keywords      TEXT[] NOT NULL,
    documentation TEXT,
    homepage      TEXT,
    license       TEXT,
    repository    TEXT,
    -- timestamps
    created_at    TIMESTAMPTZ NOT NULL,
    yanked_at     TIMESTAMPTZ,

    CONSTRAINT unique_version UNIQUE (package_id, version)

);


CREATE TABLE version_dependencies (
    id          SERIAL PRIMARY KEY,
    version_id  INTEGER NOT NULL,
    package_id  INTEGER NOT NULL,
    requirement TEXT NOT NULL,
);

CREATE TABLE categories (
    id          SERIAL PRIMARY KEY,
    label       TEXT NOT NULL UNIQUE,
    slug        TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL
);
