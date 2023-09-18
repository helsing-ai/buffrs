## Installation

### Install Buffrs

The easiest way to get `buffrs` is to install the current stable release from
[crates.io] using:

```bash
cargo install buffrs
```

As of right now you are required to authenticate yourself against your private
artifactory instance (which will be replaced by the Buffrs Registry in Q4
2023).

Logging in to your instance is done using the following `buffrs` command:

```bash
buffrs login --url https://<organization>.jfrog.io/artifactory
```

You will be prompted for an artifactory identity token which you can create
within the artifactory user interface or programmatically through terraform.

### Build and Install Buffrs from Source

As alternative installation method you can clone the [Buffrs Repository] and
install it locally using Cargo (`cargo install --path .`).

[crates.io]: https://crates.io
[Buffrs Repository]: https://github.com/helsing-ai/buffrs
