# Dependency Resolution

When you run `buffrs install`, the resolver builds a complete dependency graph
for your project — including all transitive dependencies — and determines the
concrete version to install for each package.

## Resolution algorithm

For each dependency (direct or transitive), the resolver follows this priority
order:

1. **Lockfile hit** — if `Proto.lock` records a version of the package that
   satisfies *every* requirement gathered for it so far, that version is used
   immediately without contacting the registry. This makes repeated installs
   fast and reproducible. A pin that does not satisfy them all is simply not a
   usable answer, and resolution falls through to the registry.

2. **Registry resolution** — if no locked version qualifies, the resolver
   queries the registry for all available versions of the package, then selects
   the **highest** version that satisfies **all** of the requirements at once.

3. **Download and cache** — the resolved version is downloaded, stored in the
   local cache, and its digest is recorded in the lockfile for future installs.

Transitive dependencies are discovered by reading the `Proto.toml` bundled
inside each downloaded package archive, then resolved using the same steps
above. Because a package's full set of requirements is only known once every
path to it has been walked, choosing a lower version can retract dependencies
that a previously-chosen higher version had pulled in; those become unreachable
and are dropped from the graph.

## Version conflict detection

If the same package is required by more than one path in the dependency tree,
the resolver merges every requirement and picks the highest version satisfying
all of them together. Requirements are never evaluated pairwise or in
encounter order, so a later, tighter requirement can lower an earlier pick
rather than conflicting with it.

The install fails only when the intersection is empty — no published version
satisfies every requirement:

```
no version of leaf-lib satisfies all requirements: [^1.0.0, ^2.0.0];
available versions: [2.0.0, 1.0.0]
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

The resolver always picks the **highest** version satisfying every requirement
when multiple candidates exist. This is deterministic given the same set of available
registry versions. Once a version is recorded in `Proto.lock`, it is used
as-is on all subsequent installs, regardless of newer versions that may have
been published since. Run `buffrs install` after deleting or modifying
`Proto.lock` to re-resolve against the current registry state.
