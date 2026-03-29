# The Manifest Format

The Buffrs manifest file is named `Proto.toml` and is placed at the root of a
Buffrs package or workspace. It is written in [TOML](https://toml.io/) format.

## Fields

### `edition`

```toml
edition = "0.12"
```

The `edition` field declares which edition of the Buffrs manifest format is
being used. The edition is tied to the minor version of buffrs that introduced
it, so `"0.12"` means the edition introduced with buffrs `0.12.x`.

Editions are set automatically when you create a new package with
[`buffrs init`](../commands/buffrs-init.md) or [`buffrs new`](../commands/buffrs-new.md)
and are validated on load. If the edition is incompatible with the installed
buffrs version, an error is reported.

See [Editions](editions.md) for more information.

### `[package]`

The `[package]` section declares metadata about the current package. It is
optional when using a [workspace](../guide/workspaces.md)-only manifest.

```toml
[package]
type = "api"
name = "my-api"
version = "1.0.0"
description = "My API package"   # optional
```

| Field | Required | Description |
|-------|----------|-------------|
| `type` | yes | Package type: `"api"`, `"lib"`, or `"impl"` |
| `name` | yes | Package name – must be lowercase ASCII and dashes (see [Package Name Specifications](package-name-spec.md)) |
| `version` | yes | [Semantic version](https://semver.org/) of the package (e.g. `"1.2.3"`) |
| `description` | no | A short human-readable description of the package |

See [Package Types](../guide/package-types.md) for more information.

### `[dependencies]`

The `[dependencies]` section lists the packages that this package depends on.
Each entry maps a package name to a dependency specification.

#### Remote dependency

```toml
[dependencies]
my-package = { registry = "https://your.registry/artifactory", repository = "my-repo", version = "1.2.3" }
```

| Field | Required | Description |
|-------|----------|-------------|
| `registry` | yes | URL of the Artifactory registry |
| `repository` | yes | Name of the repository within the registry |
| `version` | yes | Exact version to install (e.g. `"1.2.3"`) |

#### Local dependency

```toml
[dependencies]
my-lib = { path = "../my-lib" }
```

| Field | Required | Description |
|-------|----------|-------------|
| `path` | yes | Relative path to the local package directory |

See [Local Dependencies](../guide/local-dependencies.md) and
[Specifying Dependencies](specifying-dependencies.md) for more details.

### `[workspace]`

The `[workspace]` section turns the manifest into a workspace root. It is
mutually exclusive with the `[package]` section (a file can be either a
workspace root or a package, not both).

```toml
[workspace]
members = ["pkg1", "pkg2"]
```

| Field | Required | Description |
|-------|----------|-------------|
| `members` | yes | List of relative paths to workspace member package directories |

See [Workspaces](../guide/workspaces.md) for more information.

## Examples

### Minimal implementation manifest

```toml
edition = "0.12"
```

### API package with a remote dependency

```toml
edition = "0.12"

[package]
type = "api"
name = "my-api"
version = "0.1.0"

[dependencies]
common-types = { registry = "https://your.registry/artifactory", repository = "protos", version = "1.0.0" }
```

### Library package

```toml
edition = "0.12"

[package]
type = "lib"
name = "common-types"
version = "1.0.0"
description = "Shared protobuf type definitions"
```

### Workspace manifest

```toml
edition = "0.12"

[workspace]
members = ["api", "lib"]
```
