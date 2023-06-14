<img src="https://github.com/helsing-ai/buffrs/assets/37018485/794f7922-1b7f-4689-870a-7e1f03108ee5" onerror="this.style.display='none'" />

<h1 align="center">buffrs</h1>
<div align="center">
  <h6>An opinionated protobuf package manager</h6>
</div>
<br />
<div align="center">
  <a href="https://crates.io/crates/buffrs">
    <img
      src="https://img.shields.io/crates/v/buffrs.svg?style=flat-square"
      alt="crates.io version"
    />
  </a>
  <a href="https://docs.rs/buffrs">
    <img
      src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs"
    />
  </a>
</div>
<br />

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

To setup a new `buffrs` project you can run:

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
