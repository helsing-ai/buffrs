# Environment Variables

Buffrs reads the following environment variables at runtime.

## `BUFFRS_HOME`

The home directory for Buffrs data, including the credentials store. Defaults
to `~/.buffrs` if not set.

See [Buffrs Home](../guide/buffrs-home.md) for more information.

## `BUFFRS_CACHE`

Path to the package cache directory. When set, Buffrs uses this directory to
cache downloaded packages so that subsequent installs do not require network
access. This is particularly useful in sandboxed build environments such as
[Nix](https://nixos.org/).

## `BUFFRS_VERBOSE`

Set to `true` to enable verbose (debug-level) logging output. Equivalent to
passing the `-v` / `--verbose` flag on the command line.

## `SSL_CERT_FILE`

Path to a custom root certificate file. When set, Buffrs loads the specified
certificate into the TLS certificate store. This is useful in corporate
environments that use a custom CA.

## `HTTP_PROXY` / `HTTPS_PROXY`

Proxy URLs for HTTP and HTTPS requests respectively. When set, Buffrs routes
outgoing requests through the specified proxy server.
