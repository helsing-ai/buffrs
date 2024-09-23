## buffrs init

Initializes a Buffrs project in a new folder created in the current directory.

### Synopsis

`buffrs new <NAME>`

`buffrs new --lib <NAME>`

`buffrs new --api <NAME>`

### Description

This command creates a new Buffrs project with the provided name by creating a 
manifest file (`Proto.toml`) as well as `proto` and `proto/vendor` directories
in a new directory created at the current location.

By default, if no package type is provided, `impl` (implementation) will be
used. The meaning of this is described in [Package
Types](../guide/package-types.md).
