## buffrs publish

Generates a release and publishes it to the specified registry.

### Synopsis

`buffrs publish [OPTIONS] --registry <REGISTRY> --repository <REPOSITORY>`

### Options

* `--allow-dirty`: allows publishing the package even if the repository has
uncommitted changes.
* `--dry-run`: causes a release bundle to be generated but skips uploading to
  the registry.

### Description

The `publish` command bundles the package's protocol buffer files and manifest
into a gzip-compressed tarball, which is then uploaded to the specified registry
and repository for publication. Once published the artifact will be available
for other packages to be installed as dependencies.

In order for this command to be successful, the registry must be reachable via
the network, and if authorization is required, credentials must have been
previously saved via a [`buffrs login`](buffrs-login.md) invocation.

By default, Buffrs does not allow publishing packages from git repositories in a
dirty state (note: this requires the `git` feature to be enabled). This
behaviour can be overriden by passing the `--allow-dirty` flag.

#### Supported package types

Both library and API packages can be published -- the only exception is
implementation packages, which are deemed to be terminal packages in the
dependency graph. This may change in the future. More details in [Package
Types](../guide/package-types.md).

Library packages cannot have dependencies, so releasing this kind of package may
fail if any are provided in the manifest. API dependencies on library packages
is also forbidden and will cause publication to fail.