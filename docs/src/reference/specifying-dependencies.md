# Specifying Dependencies

Dependencies are declared in the `[dependencies]` section of the `Proto.toml`
manifest. Each entry maps a dependency package name to a dependency
specification object.

## Remote Dependencies

Remote dependencies are downloaded from an Artifactory registry during
[`buffrs install`](../commands/buffrs-install.md).

```toml
[dependencies]
my-package = { registry = "https://your.registry/artifactory", repository = "my-repo", version = "1.2.3" }
```

The `version` field must be an exact semantic version (e.g. `"1.2.3"`).
Version ranges or operators (`^`, `~`, `<`, `>`) are not currently supported.

Use [`buffrs add`](../commands/buffrs-add.md) to add a remote dependency from
the command line:

```
buffrs add --registry https://your.registry/artifactory my-repo/my-package@1.2.3
```

## Local Dependencies

Local dependencies are resolved from the local filesystem relative to the
manifest. They are useful for multi-package repositories where packages depend
on each other without going through a remote registry.

```toml
[dependencies]
my-lib = { path = "../my-lib" }
```

The `path` field is a relative path from the manifest file to the dependency's
root directory (the directory containing the dependency's `Proto.toml`).

See [Local Dependencies](../guide/local-dependencies.md) for more information.

## Lockfile

After adding or modifying dependencies in the manifest, run
[`buffrs install`](../commands/buffrs-install.md) to resolve and lock them.
The lockfile (`Proto.lock`) records the exact resolved versions and checksums
and should be committed to version control.

See [Manifest vs Lockfile](../guide/manifest-vs-lockfile.md) for more
information.

