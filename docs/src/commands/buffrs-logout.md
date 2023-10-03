## buffrs logout

Removes an authentication token from the credentials store.

### Synopsis

`buffrs logout --registry <url>`

### Description

This command removes a previously saved token from the credentials store by its
associated registry URL. Future invocations of `publish` and `install` that
involve the given registry will then default to unauthenticated mode.

The credentials are currently stored in `$HOME/.buffrs/credentials.toml`.