# Workspaces

Since version 0.12, buffrs supports workspaces for managing multiple related packages in a single repository.

## What is a Workspace?

A workspace is a collection of buffrs packages that share a common root directory. This is useful when maintaining multiple API versions, splitting functionality across packages, or managing shared libraries alongside consuming packages.

## Creating a Workspace

To create a workspace, add a `[workspace]` section to your root `Proto.toml`:

```toml
edition = "0.12"

[workspace]
members = [
  "packages/common",
  "packages/api-one",
  "packages/api-two",
]
```

Each member is a separate buffrs package with its own `Proto.toml` manifest. The workspace root `Proto.toml` should **not** contain a `[package]` section - it only defines the workspace structure.

## Directory Structure

A typical workspace structure:

```
my-workspace/
├── Proto.toml              # Workspace root manifest
└── packages/
    ├── common/
    │   ├── Proto.toml      # Package manifest
    │   └── proto/
    │       └── common.proto
    ├── api-one/
    │   ├── Proto.toml
    │   └── proto/
    │       └── api.proto
    └── api-two/
        ├── Proto.toml
        └── proto/
            └── api.proto
```

## Workspace Commands

### Installing Dependencies

From the workspace root:

```bash
buffrs install
```

This installs dependencies for all workspace members, handling inter-workspace dependencies automatically.

### Publishing

From the workspace root:

```bash
buffrs publish --registry http://... --repository my-repo
```

Buffrs automatically:
- Resolves dependencies across all workspace members
- Publishes packages in topological order (dependencies first)
- Handles local dependencies within the workspace
- Ensures each package is published only once, even if multiple members depend on it

### Uninstalling

From the workspace root:

```bash
buffrs uninstall
```

Clears vendor directories for all workspace members.

## Inter-workspace Dependencies

Workspace members can depend on each other using local path references:

```toml
# packages/api-two/Proto.toml
edition = "0.12"

[package]
type = "api"
name = "my-api-two"
version = "2.0.0"

[dependencies]
"my-common" = { path = "../common" }
```

When publishing the workspace, buffrs automatically publishes `my-common` first, then publishes `my-api-two` with the dependency reference updated to point to the registry.

## Package-only Commands

Some commands are designed for package-level operations and cannot run from a workspace root:

- `buffrs add`
- `buffrs remove`
- `buffrs package`
- `buffrs lint`
- `buffrs list`

These commands will provide a clear error message directing you to run them from a package directory within the workspace.

## Benefits

Workspaces provide:

- **Single-command publishing**: No need to manually navigate directories or worry about dependency order
- **Automatic deduplication**: Shared dependencies are published once
- **Development efficiency**: Develop and test changes across multiple packages simultaneously
- **Monorepo support**: Natural fit for monorepo setups with multiple API packages
