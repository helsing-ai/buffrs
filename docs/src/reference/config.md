# Configuration

## Authentication

Buffrs uses a local credential storage for authenticating with registries. The [`login`](../commands/buffrs-login.md) command can be used to add new credentials to the storage. Once saved, credentials are automatically used for authenticating with the registry they are associated with. Registries are identified by their URL.

Note that credentials are optional, if they are missing for a given registry URL, no authentication is attempted.

## TLS configuration

Buffrs will automatically pick up the `SSL_CERT_FILE` environment variable if it's been set, and attempt to use the native subsystem to parse and load the specified root certificate into the certificate store. No additional configuration is needed to apply custom root certificates.

## Proxy support

Buffrs will automatically pick up on `HTTP_PROXY` and `HTTPS_PROXY` environment variables if they've been set, and use the specified proxy URLs for the associated remote requests. No additional configuration is needed.