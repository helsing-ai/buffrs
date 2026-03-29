## buffrs lock

Commands for working with the Buffrs lockfile (`Proto.lock`).

### Synopsis

`buffrs lock <COMMAND>`

### Description

The `lock` command is a subcommand group for interacting with the Buffrs
lockfile. The lockfile (`Proto.lock`) records the exact resolved versions of
all dependencies and is managed automatically by [`buffrs install`](buffrs-install.md).

### Subcommands

* [`buffrs lock print-files`](buffrs-lock-print-files.md) – Print the locked
  file requirements as JSON to stdout.

### See Also

* [Manifest vs Lockfile](../guide/manifest-vs-lockfile.md)
* [The Lockfile Format](../reference/lockfile.md)
