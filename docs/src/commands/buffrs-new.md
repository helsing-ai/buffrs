## buffrs new

Initializes a Buffrs project in a new folder created in the current directory.

### Synopsis

`buffrs new --lib <NAME>`

`buffrs new --api <NAME>`

### Description

This command creates a new Buffrs project with the provided name by creating a
manifest file (`Proto.toml`) as well as `proto` and `proto/vendor` directories
in a new directory created at the current location.

A package type (`--lib` or `--api`) must be provided. The meaning of each type
is described in [Package Types](../guide/package-types.md).

Unlike [`buffrs init`](buffrs-init.md) which initializes the current directory,
this command creates a new subdirectory named after the package.
