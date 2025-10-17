## buffrs install

Downloads and installs dependencies specified in the manifest.

### Synopsis

`buffrs install`

### Description

This command manages Buffrs local set of installed dependency packages. It is
meant to be run from the root of the Buffrs project, where the `Proto.toml`
manifest file can be found. Currently, only API and implementation packages can have
dependencies, so this command is only useful for those package types.

The installation process will respect the requirements stated in the manifest
file -- specifically, the version, registry and repository provided for each
dependency. Each dependency may specify its own dependencies, via its manifest
file, which will also be downloaded and its contents unpacked flatly to the
local filesystem, under the shared `proto/vendor` path prefix (see [Project
Layout](../guide/project-layout.md) for more information). Only one version of
each package can be installed, so if there is a conflicting requirement,
installation will fail.

Since version 0.12, `buffrs install` supports [local dependencies](../guide/local-dependencies.md)
and [workspaces](../guide/workspaces.md):
- **Local dependencies**: Dependencies specified with `path = "..."` are recursively resolved and installed
- **Workspaces**: When run from a workspace root, installs dependencies for all workspace members

Once installation has completed, the resolved packages versions will be frozen
and captured in a `Proto.lock` file, which ensures that future installations
(local or performed in another machine) will install the exact same dependency
versions. This file is managed automatically and should be kept under version
control, so that others can reproduce your local installation.

#### Lockfile

The install command manages the Buffrs lockfile (`Proto.lock`) automatically. If
one doesn't exist when the command is invoked, one is created after the
installation has completed.

If dependencies have been added or removed since the last invocation, the
lockfile will be modified accordingly. If the manifest requirements conflict
with the lockfile (i.e. the manifest requests a different version than the one
that was locked), installation will fail.

Versions are locked upon first installation, and will persist until the lockfile
is regenerated with `buffrs lock`, dependencies are explicitly upgraded via
`buffrs update` (or a manual edit of the manifest) or they have been removed.
Once removed, if dependencies are added back again, a different version may be
automatically selected and locked.

##### Transitive dependencies

Transitive dependencies are also managed by the current project's lockfile. Even
if dependencies provide their own lockfile, those won't be used.
