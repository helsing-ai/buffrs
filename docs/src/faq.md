# FAQ

## Why do I get "missing field X" error when running `install`?

This error indicates one of your dependencies has an invalid manifest file. The
error message should indicate which dependency is broken and why:

```
Error:
   0: Failed to process dependency foo-bar@0.0.1
   1: Failed to parse the manifest
   2: TOML parse error at line 1, column 1
   2:   |
   2: 1 | [package]
   2:   | ^
   2: missing field `dependencies`
```

Since the CLI won't allow you to publish a package with an invalid manifest,
you may wonder why some published packages may be broken in this way. The
reason is that all fields used to be optional -- `impl` packages didn't need a
`package` section, and `lib` packages didn't require `dependencies`. When the
lockfile functionality was first introduced, both sections were made mandatory
for all package types, which broke parsing of older manifests.

To fix this you must depend on a newer version of the dependency that publishes
a compliant manifest file.
