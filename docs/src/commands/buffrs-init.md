# buffrs init

Initializes the current directory as a Buffrs project.

# Synopsis

`buffrs init [name]`
`buffrs init --lib [name]`
`buffrs init --api [name]`

# Description

This command prepares the current directory as a Buffrs project, by creating a manifest file (`Proto.toml`) as well as `proto` and `proto/vendor` directories.

By default, if no name is given, the current directory name is used as the package name. Note that there are special constraints on valid package names (see [Package Name Specification](../reference/pkgid-spec.md) for more details).

By default, if no package type is provided, `impl` (implementation) will be used. The meaning of this is described in [Package Types](../guide/package-types.md).