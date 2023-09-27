# buffrs add

Adds a new dependency to the package manifest.

# Synopsis

`buffrs add --registry <REGISTRY> <DEPENDENCY>`

# Description

The add command is the recommended way to include a new dependency in the current package. It modifies the local manifest file and will overwrite a pre-existing entry for the same dependency package if it exists.

## Dependency locator format

The dependency should be specified with the repository, name and version according to the following format:

```
<repository>/<package>@<version>
```

The repository name should adhere to lower-kebab case (e.g. `my-buffrs-repo`). The package name has its own set of constraints as detailed in [Package Name Specification](../reference/pkgid-spec.md). The version must adhere to the [Semantic Version convention](https://semver.org/) (e.g. `1.2.3`) -- see [SemVer compatibility](../reference/semver.md) for more information.

## Lockfile interaction

Currently adding a new dependency won't automatically update the lockfile (`Proto.lock`). This is planned to change, but for now make sure to follow up with [`buffrs install`](buffrs-install.md) after adding a new dependency to make sure your lockfile is kept in sync.