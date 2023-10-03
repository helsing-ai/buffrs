## buffrs package

Generates a release tarball for the package in the current directory.

### Synopsis

`buffrs package`

`buffrs package --output-directory <OUTPUT_DIRECTORY>`

`buffrs package --dry-run`

### Description

Like the [`publish`](buffrs-publish.md) command, the `package` command bundles
the package's protocol buffer files and manifest into a gzip-compressed tarball.
However, unlike the [`publish`](buffrs-publish.md) command it does not actually
interact with the registry, instead it only writes the release tarball into the
current directory. This is useful for manual distribution and for safely
validating the package setup.

#### Supported package types

Both library and API packages can be released -- the only exception is
implementation packages, which are deemed to be terminal packages in the
dependency graph. This may change in the future. More details in [Package
Types](../guide/package-types.md).

Library packages cannot have dependencies, so releasing this kind of package may
fail if any are provided in the manifest. API dependencies on library packages
is also forbidden and will cause releases to fail to be generated.