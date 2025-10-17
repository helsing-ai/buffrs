## buffrs uninstall

Deletes all installed dependencies from the local filesystem.

### Synopsis

`buffrs uninstall`

### Description

This command does the reverse operation from the
[`install`](buffrs-uninstall.md) command, and will clear out the `proto/vendors`
directory, thus removing all installed dependencies from the local filesystem. This
is generally safe to do as the `vendors` directory is managed by Buffrs and
shouldn't contain any custom proto files. Subsequently invoking the install
command should restore the exact same files, assuming the lockfile hasn't been
regenerated.

Since version 0.12, when run from a [workspace](../guide/workspaces.md) root,
this command will clear vendor directories for all workspace members.
