# buffrs login

Saves an authentication token for use with a registry.

# Synopsis

`buffrs login --registry <url>`

# Description

This command prompts for an API or Identity token that can be used to authenticate with
Artifactory for downloading and publishing packages.

The token is stored in `$HOME/.buffrs/credentials.toml` in the following format:

```toml
[[credentials]]
uri = "https://example.com/artifactory"
token = "<secret>"
```
