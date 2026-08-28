# SemVer Compatibility

buffrs uses [Semantic Versioning](https://semver.org/) for all packages.
Version requirements in `Proto.toml` follow the same syntax as
[Cargo](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html).

## Version requirement syntax

A version requirement is placed in the `version` field of a dependency:

```toml
[dependencies.my-lib]
version = "^1.2.0"
registry = "https://my-registry.example.com"
repository = "my-repo"
```

The following operators are supported:

### Caret (`^`) — default for ranges

Allows minor and patch updates within the same major version.
This is the recommended operator for most dependencies.

| Requirement | Resolves versions |
|-------------|-------------------|
| `^1.2.3`    | `>=1.2.3, <2.0.0` |
| `^1.2`      | `>=1.2.0, <2.0.0` |
| `^1`        | `>=1.0.0, <2.0.0` |
| `^0.2.3`    | `>=0.2.3, <0.3.0` |
| `^0.0.3`    | `>=0.0.3, <0.0.4` |

Note that `0.x` versions are treated as unstable: `^0.2` only allows `0.2.x`,
not `0.3.x`, since breaking changes are expected in pre-1.0 packages.

### Tilde (`~`) — patch-level updates only

Allows patch updates within the same minor version.

| Requirement | Resolves versions |
|-------------|-------------------|
| `~1.2.3`    | `>=1.2.3, <1.3.0` |
| `~1.2`      | `>=1.2.0, <1.3.0` |
| `~1`        | `>=1.0.0, <2.0.0` |

### Exact (`=`) — pin to a specific version

Resolves to exactly the stated version, with no flexibility.

```toml
version = "=1.2.3"
```

Use exact pins when you need bit-for-bit reproducibility in the manifest
itself, or when you are distributing a library whose consumers should be
in full control of the version.

### Comparison operators

For more control, the standard comparison operators are available:

| Requirement     | Meaning                          |
|-----------------|----------------------------------|
| `>=1.2.0`       | Any version at or above 1.2.0    |
| `>1.2.0`        | Any version strictly above 1.2.0 |
| `<2.0.0`        | Any version strictly below 2.0.0 |
| `<=2.0.0`       | Any version at or below 2.0.0    |
| `>=1.0.0, <2.0.0` | Intersection (multiple constraints) |

## How the resolver picks a version

When a requirement matches more than one available version, buffrs always
selects the **highest** satisfying version. The resolved concrete version is
written to `Proto.lock` to ensure reproducible installs — re-running
`buffrs install` will use the locked version rather than querying the registry
again.

See [Dependency Resolution](./resolver.md) for a full description of the
resolution algorithm.

## Choosing between pinning and ranges

| Situation | Recommended style |
|-----------|-------------------|
| Public library — let consumers decide | `^1.0.0` |
| Internal service — stable dependency set | `^1.0.0` or `~1.2.0` |
| Security patch must be applied exactly | `=1.2.5` |
| Compatibility ceiling known | `>=1.0.0, <3.0.0` |
