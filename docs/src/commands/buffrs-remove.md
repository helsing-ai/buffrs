## buffrs remove

Removes an existing dependency from the package manifest.

### Synopsis

`buffrs remove <PACKAGE>`

### Description

The remove command is the recommended way to remove a dependency (identified by
package name) from the manifest. It modifies the manifest and will produce an
error if the specified package cannot be found. It implements the opposite
operation from the [`add`](buffrs-add.md) command.

#### Lockfile Interaction

Currently removing a dependency won't automatically update the lockfile
(`Proto.lock`). This is planned to change, but for now make sure to follow up
with [`buffrs install`](buffrs-install.md) after adding a new dependency to
make sure your lockfile is kept in sync.
