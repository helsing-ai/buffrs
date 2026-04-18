# Specifying Dependencies

Dependencies are declared in the `[dependencies]` section of `Proto.toml`.
Each entry names the package and provides a version requirement, registry URL,
and repository name.

## Inline table syntax

```toml
[dependencies.my-lib]
version = "^1.0.0"
registry = "https://my-registry.example.com"
repository = "my-repo"
```

The three required fields for a remote dependency are:

| Field | Description |
|-------|-------------|
| `version` | A SemVer requirement — see [SemVer Compatibility](./semver.md) |
| `registry` | Base URL of the Artifactory registry |
| `repository` | Repository name within that registry |

## Adding dependencies via the CLI

The `buffrs add` command writes the manifest entry for you:

```bash
# Caret range (recommended): resolves to the highest 1.x.y
buffrs add --registry https://my-registry.example.com my-repo/my-lib@^1.0.0

# Exact pin: resolves to exactly 1.2.3
buffrs add --registry https://my-registry.example.com my-repo/my-lib@=1.2.3

# Latest: omitting the version resolves to the latest available
buffrs add --registry https://my-registry.example.com my-repo/my-lib
```

After adding a dependency, run `buffrs install` to resolve and download it.

## Version requirements

buffrs supports the full range of SemVer requirement operators:

```toml
version = "^1.0.0"      # >=1.0.0, <2.0.0  (recommended for most deps)
version = "~1.2.0"      # >=1.2.0, <1.3.0  (patch updates only)
version = ">=1.5.0"     # any version at or above 1.5.0
version = "=1.2.3"      # exactly 1.2.3
version = ">=1.0, <2.0" # explicit intersection
```

The resolver queries the registry and selects the **highest** available version
satisfying the requirement. See [SemVer Compatibility](./semver.md) for the
full operator reference.

## Local dependencies

You can depend on a package in a local directory (useful in monorepos or during
development):

```toml
[dependencies.my-lib]
path = "../my-lib"
```

Local dependencies do not have a version requirement — the package at that
path is used as-is. They cannot be mixed with a remote entry for the same
package name.

## The lockfile

Once resolved, the concrete version is recorded in `Proto.lock`. Subsequent
installs use the locked version without re-querying the registry, ensuring
reproducible builds across machines and CI environments.

Commit `Proto.lock` to version control for applications and services. For
libraries intended to be consumed by others, committing the lockfile is
optional — consumers will resolve their own versions.

See [Manifest vs Lockfile](../guide/manifest-vs-lockfile.md) for more detail.
