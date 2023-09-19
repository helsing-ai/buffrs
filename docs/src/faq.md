# FAQ

## Why doesn't `buffrs add`, `buffrs publish`, or `buffrs login` work anymore?

We recently expanded the capabilities of Buffrs a bit and made it so it can handle being connected to multiple registries.
For this reason, you'll now likely have to add `--registry http://my-registry.jfrog.io/artifactory` to all three.

Note that `buffrs login` had a `--url` flag previously. It was renamed to `--registry` for the sake of consistency.

## Why is my `credentials.toml` file broken?

Because we expanded Buffrs and made it capable of connecting to multiple registries, we had to make some changes to how we store our credentials.

When it only supported a single registry, it looked like this:

```toml
[artifactory]
url = "https://org.jfrog.io/artifactory"
password = "some-token"
```

And now it looks like this, supporting multiple regisitries:

```toml
[[credentials]]
uri = "https://org1.jfrog.io/artifactory"
token = "some-token"

[[credentials]]
uri = "https://org2.jfrog.io/artifactory"
token = "some-other-token"
```

## Why can't I log in with a username?

`buffrs login` no longer supports the `--username` flag, as we no longer use BasicAuth. Instead we set the `X-JFrog-Art-Api` header.