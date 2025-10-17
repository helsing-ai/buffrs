## Local Dependencies

When working on larger projects or in monorepo projects setups you may find
yourself in the situation to consume a locally defined buffrs project.

Imagine the following project setup:

```
mono
├── build.rs
├── Cargo.toml
├── Proto.toml
├── Proto.toml
├── proto
|   └── mono.proto
└── src
    └── main.rs
```

In this scenario the buffrs project for `mono-api` and the cargo project for
the `mono-server` are setup in the very same directory, which is totally fine
as long as this server does not require other buffrs api packages to be
compiled!

#### Problem

Adding a dependency on other (unrelated api packages to `mono-api`) is
complicated in the above scenario because its not clear to buffrs whether you
are trying to reuse third party api definitions for your api or just wanting to
install protos for compilation.

Hence buffrs will throw you an error if you try to publish an api package with
dependencies on other apis!

```
Error:   × failed to publish `mono-api` to `http://<registry-uri>/<repository>`
  ╰─▶ depending on API packages is not allowed
```

#### Solution

Gladly buffrs offers a builtin solution for this! You can separate the
`mono-api` buffrs package (used to publish your api) from the `mono-server`
buffrs projects (used to install protos for compiling the server).

A monorepo setup here could look like this:

```
mono
├── mono-api
|   ├── Proto.toml
|   └── proto
|       └── mono.proto
└── mono-server
    ├── build.rs
    ├── Cargo.toml
    ├── Proto.toml
    ├── proto
    |   └── vendor
    └── src
        └── main.rs
```

Where `mono/mono-api/Proto.toml` has this content:

```
edition = "0.9"

[package]
type = "api"
name = "mono-api"
version = "0.1.0"
```

And `mono/mono-server/Proto.toml` has this content:

```
edition = "0.9"

[dependencies]
mono-api = { path = "../api" }
third-party-api = { version = "=1.0.0", repository = "some-repo", registry = "http://..." }
```

This enables you to:

- Independently publish `mono-api` using `buffrs publish` / `buffrs package`
- Independently declare dependencies for `mono-server`

#### Publishing with Local Dependencies

Since version 0.12, buffrs automatically handles publishing packages with local dependencies:

```bash
cd mono-server
buffrs publish --registry http://... --repository my-repo
```

Buffrs will:
1. Recursively resolve all local dependencies (and their transitive dependencies)
2. Publish each local dependency in topological order
3. Rewrite local references in the published manifest to point to the registry

For example, if `mono-server` depends on `mono-api` (local), buffrs first publishes `mono-api` to the registry, then publishes `mono-server` with the dependency updated to reference the registry location instead of the local path.

This makes it seamless to develop locally while maintaining publishable packages.
