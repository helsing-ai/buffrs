## buffrs lock print-files

Prints the locked files as JSON to stdout.

### Synopsis

`buffrs lock print-files`

### Description

> Note: This command is designed for consumption through other scripts and
> programs.

Using this command you can retrieve a list of files that buffrs downloads
according to the lockfile. For correct behavior please make sure your
`Proto.lock` is up to date when using this command!

### Example

Given a project that depends on a `physics` package at version `1.0.0` and a
populated `Proto.lock`:

```

```

Running `buffrs lock print-files` will print the following output derived from
the lockfile:

```
[
  {
    "url": "https://your.internal.registry/artifactory/your-repository/physics/physics-1.0.0.tgz",
    "digest": "sha256:61ecdcd949c7b234160dc5aacb4546a21512de4ff8ea85f2fdd7d5fff2bf92b5"
  }
]
```

This way you can programmatically consume this (e.g. in nix, bash, etc) and
download the files if your project while maintaining integrity.
