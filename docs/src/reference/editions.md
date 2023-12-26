## Editions

Editions of buffrs mark a specific evolutionary state of the package manager.
The edition system exists to allow for fast development of buffrs while
allowing you to already migrate existing protobufs to buffrs even though it
has not yet reached a stable version.

Editions can be either explicitly stated in the `Proto.toml` or are
automatically inlined once a package is created using buffrs. This ensures that
you dont need to care about them as a user but get the benefits.

> Note: If you release a package with an edition that is incompatible with
> another one (e.g. if `0.7` is incompatible with `0.8`) you will need to
> re-release the package for the new edition (by bumping the version, or
> overriding the existing package) to regain compatibility.

You may see errors like this if you try to consume (or work on) a package of
another edition.

```
Error:   × could not deserialize Proto.toml
  ╰─▶ TOML parse error at line 1, column 1
        |
      1 | edition = "0.7"
        | ^^^^^^^^^^^^^^^
      unsupported manifest edition, supported editions of 0.8.0 are: 0.8
```

### Canary Editions

```toml
edition = "0.7"
```

Canary editions are short-lived editions that are attached to a specific
minor release of buffrs in the `0.x.x` version range. The edition name contains
the minor version it is usable for. E.g. the edition `0.7` is usable /
supported by all `0.7.x` buffrs releases. Compatibility beyond minor releases
is not guaranteed as fundamental breaking changes may be introduced between
editions.
