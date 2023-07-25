<!-- markdownlint-disable-next-line -->
<div align="center">

<img src="https://github.com/helsing-ai/buffrs/assets/37018485/76c51445-b5a6-4f4e-a39c-7de7e31a0613" onerror="this.style.display='none'" />
<br/>

# `buffrs`

**An opinionated protobuf package manager**

[![Helsing](https://img.shields.io/badge/helsing-open%20source-black.svg)](http://helsing.ai)
[![Buffrs Crate](https://img.shields.io/crates/v/buffrs.svg)](crates.io/crates/buffrs)
[![Buffrs Book](https://img.shields.io/badge/book-latest-blueviolet.svg)](https://helsing-ai.github.io/buffrs)
[![Buffrs Docs](https://img.shields.io/badge/docs-latest-blue.svg)](https://docs.rs/buffrs)

</div>

## Motivation

Protocol Buffers are agreeably a great way to define fully typed,
language-independent API schemas with strong backward compatibility guarantees.
They offer a neat experience for API consumers through generated bindings. *The
biggest problem associated with Protocol Buffers is their distribution.*

- How do you consume the raw protobuf files of one project reliably in another
  one?
- How do you prevent transitive dependencies?
- How do you publish to a unified registry with package format across
  languages?

One obvious way is to generate code bindings in the repository containing the
Protocol Buffers and publish the generated bindings, but this is associated
with problems such as language lock-in. You need to proactively publish
bindings for any possible language your API consumers may use. Also, in
strongly typed languages like Rust, it is hard to extend the behavior of
generated code in consuming projects due to _the orphan rule_. Summing up: this
approach works somehow but hurts frequently.

This is where `buffrs` comes in: `buffrs` solves this by defining a strict,
package-based distribution mechanism and treats Protocol Buffers as a
first-class citizen.

*This allows you to publish `buffrs` packages to a registry and properly depend
on them in other projects.*

## Roadmap

- [x] Support project manifests and dependency declaration
- [x] Support package distribution via Artifactory
- [ ] Implement `buffrs-registry`, a self-hostable, S3-based registry.
- [ ] Supply tooling around Protocol Buffers, such as bindgen, linting, and
  formatting.

## Installation

You can install the `buffrs` package manager using:

```bash
cargo install buffrs
```

## Quickstart

### Project Setup

To setup a new `buffrs` project you can run:

```bash
buffrs init --api <my-grpc-server-name>
```

> Note: The `--api` flag is only relevant for grpc servers, not for clients!

### Registry Login

Log into your artifactory registry:

```bash
buffrs login --url https://<org>.jfrog.io/artifactory --username your.name@your.org
```

You will be prompted for an artifactory identity token which you can create in
artifactory.

### Managing Dependencies

Add protocol buffers from other projects using a `buffrs` command:

```bash
buffrs add my-proto-repo/my-protos@1.0.0
```

You can also edit the `Proto.toml` manifest and add or remove dependencies
under the `[dependencies]` section.

The manifest file after the above command looks like this:

```toml
[dependencies]
my-protos = { version = "1.0.0", repository = "my-proto-repo" }
```

> Note: Use `buffrs remove <package>` for removing a package from your protos

### Installing Dependencies

Install the `buffrs` manifest

```bash
buffrs install
```

Now you can run your language dependent build tool (e.g. `cargo build`) to
generate local code bindings.

> Note: Use `buffrs uninstall` for cleaning your local proto folder

### Publishing a Package

To package and publish a `buffrs` release to the specified registry and
repository run:

```bash
buffrs publish --repository <artifactory-repository>
```

## Contributing

Pull requests are welcome. For major changes, please open an issue first
to discuss what you would like to change.

Please make sure to update tests as appropriate.
