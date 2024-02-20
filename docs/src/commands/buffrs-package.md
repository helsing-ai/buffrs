## buffrs package

Generates a release tarball for the package in the current directory.

### Synopsis

`buffrs package`

### Options

* `--dry-run`: prevents buffrs from actually writing the tarball to the filesystem
* `--output-directory`: allows you to specify a directory to output the package
* `--version`: allows you to override the version set in the manifest


### Description

Like the [`publish`](buffrs-publish.md) command, the `package` command bundles
the package's protocol buffer files and manifest into a gzip-compressed
tarball. However, unlike the [`publish`](buffrs-publish.md) command it does not
actually interact with the registry, instead it only writes the release tarball
into the current directory. This is useful for manual distribution and for
safely validating the package setup.
