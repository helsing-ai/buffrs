# FAQ

## Why doesn't `buffrs add`, `buffrs publish`, or `buffrs login` work anymore?

We recently expanded the capabilities of Buffrs a bit and made it so it can
handle being connected to multiple registries. For this reason, you'll have to
add `--registry http://my-registry.jfrog.io/artifactory` to all three.

Note that `buffrs login` had a `--url` flag previously. It was renamed to
`--registry` for the sake of consistency.

## Why is my `credentials.toml` file broken?

Because we expanded Buffrs and made it capable of connecting to multiple
registries, we had to make some changes to how we store our credentials.

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

`buffrs login` no longer supports the `--username` flag, as we no longer use
BasicAuth. Instead we set the `Authorization` header which enables support for
identity tokens, jwt, and encoded basic auth tokens at the same time.

## How do I use a custom root certificate?

Just set the `SSL_CERT_FILE` environment variable to point to your custom
certificate and you're good to go. More details in
[Configuration](reference/config.md).

## How do I use an HTTP proxy?

Just set either `HTTP_PROXY` or `HTTPS_PROXY` as environment variables
(depending on what kind of proxy, or proxies, you have) and Buffers will
direct all of its requests of the matching protocol to the specified proxy
URL. More details in [Configuration](reference/config.md).
