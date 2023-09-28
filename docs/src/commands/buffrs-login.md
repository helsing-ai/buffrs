# buffrs login

Saves an authentication token in the credentials store.

# Synopsis

`buffrs login --registry <url>`

# Description

This command prompts for an API or Identity token that can be used to authenticate with
Artifactory for downloading and publishing packages.

The token is currently stored in `$HOME/.buffrs/credentials.toml` in the following format:

```toml
[[credentials]]
uri = "https://example.com/artifactory"
token = "<secret>"
```

In the future this may change to use system native keychains.