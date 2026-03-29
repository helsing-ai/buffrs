# The Lockfile Format

The Buffrs lockfile is named `Proto.lock` and is placed at the root of a
Buffrs package or workspace. It is written in [TOML](https://toml.io/) format
and is managed automatically by [`buffrs install`](../commands/buffrs-install.md).

The lockfile should be committed to version control. It ensures that all
contributors and CI environments install the exact same dependency versions,
including transitive dependencies.

## Structure

```toml
version = 3

[[package]]
name = "my-dep"
version = "1.2.3"
registry = "https://your.registry/artifactory"
repository = "my-repo"
digest = "sha256:abc123..."

[[package]]
name = "transitive-dep"
version = "0.4.1"
registry = "https://your.registry/artifactory"
repository = "my-repo"
digest = "sha256:def456..."
```

### `version`

The lockfile format version. This is managed automatically by buffrs.

### `[[package]]`

Each `[[package]]` entry records one resolved dependency (direct or
transitive). Fields:

| Field | Description |
|-------|-------------|
| `name` | Package name |
| `version` | Exact resolved version |
| `registry` | Registry URL the package was downloaded from |
| `repository` | Repository within the registry |
| `digest` | SHA-256 checksum of the downloaded package archive (prefixed with `sha256:`) |

## Lockfile Interaction

The lockfile is automatically created or updated when running
[`buffrs install`](../commands/buffrs-install.md). You should not need to edit
it manually.

To obtain the list of locked files as JSON (useful for scripted or sandboxed
installations), use [`buffrs lock print-files`](../commands/buffrs-lock-print-files.md).

See [Manifest vs Lockfile](../guide/manifest-vs-lockfile.md) for more
information on the relationship between the manifest and the lockfile.
