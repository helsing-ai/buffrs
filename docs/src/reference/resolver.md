# Dependency Resolution

When you run `buffrs install`, the resolver builds a complete dependency graph
for your project — including all transitive dependencies — and determines the
concrete version to install for each package.

## Resolution algorithm

For each dependency (direct or transitive), the resolver follows this priority
order:

1. **Lockfile hit** — if `Proto.lock` already records a version of the package
   that satisfies the requirement, that version is used immediately without
   contacting the registry. This makes repeated installs fast and reproducible.

2. **Registry resolution** — if no matching locked version exists, the resolver
   queries the registry for all available versions of the package, then selects
   the **highest** version that satisfies the requirement.

3. **Download and cache** — the resolved version is downloaded, stored in the
   local cache, and its digest is recorded in the lockfile for future installs.

Transitive dependencies are discovered by reading the `Proto.toml` bundled
inside each downloaded package archive, then resolved recursively using the
same steps above.

## Version conflict detection

If the same package is required by more than one path in the dependency tree,
the resolver checks that the already-resolved version satisfies all
requirements. If it does not, the install fails with a version conflict error:

```
version conflict for leaf-lib: requirement ^2.0.0 is not satisfied by
resolved version 1.5.0 (chosen to satisfy ^1.0.0)
```

To fix a conflict, update the requiring packages so their version requirements
overlap, or introduce a package that bridges the incompatible requirements.

Note that this conflict detection operates **within a single package's
dependency graph**. In a workspace, different members may independently resolve
different versions of the same package — the workspace lockfile records them
separately using a `(name, version)` composite key.

## Workspace resolution

In a workspace, each member package's dependency graph is resolved
independently. The workspace lockfile (`Proto.lock` at the workspace root)
accumulates all resolved packages across all members. Because the workspace
lockfile allows multiple versions of the same package, two members that require
incompatible versions of a shared library can co-exist.

If a subsequent install finds a workspace lockfile, it reuses those locked
versions (subject to satisfying each member's requirements) to avoid redundant
registry queries.

## Topological ordering

After the full graph is built, packages are sorted topologically so that each
dependency is installed before its dependants. This guarantees that vendored
proto sources are available in the correct order during compilation.

## Determinism and the lockfile

The resolver always picks the **highest** satisfying version when multiple
candidates exist. This is deterministic given the same set of available
registry versions. Once a version is recorded in `Proto.lock`, it is used
as-is on all subsequent installs, regardless of newer versions that may have
been published since. Run `buffrs install` after deleting or modifying
`Proto.lock` to re-resolve against the current registry state.
