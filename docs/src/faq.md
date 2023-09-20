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

## Why doesn't `buffrs add`, `buffrs publish`, or `buffrs login` work anymore?

We recently expanded the capabilities of Buffrs a bit and made it so it can handle being connected to multiple registries.
For this reason, you'll have to add `--registry http://my-registry.jfrog.io/artifactory` to all three.

Note that `buffrs login` had a `--url` flag previously. It was renamed to `--registry` for the sake of consistency.

## Why is my `credentials.toml` file broken?

Because we expanded Buffrs and made it capable of connecting to multiple registries, we had to make some changes to how we store our credentials.

When it only supported a single registry, it looked like this:

```toml
[artifactory]
url = "https://org.jfrog.io/artifactory"
password = "some-token"
```

And now it looks like this, supporting multiple regisitries:

```toml
[[credentials]]
uri = "https://org1.jfrog.io/artifactory"
token = "some-token"

[[credentials]]
uri = "https://org2.jfrog.io/artifactory"
token = "some-other-token"
```

## Why can't I log in with a username?

`buffrs login` no longer supports the `--username` flag, as we no longer use BasicAuth. Instead we set the `X-JFrog-Art-Api` header.
