# FAQ


## Breaking Changes

### v?.?.? - UNRELEASED

Buffrs can not be logged into multiple repositories. To facilitate this, a few command line switches have been added (and renamed)

- `buffrs add` and `buffrs publish` have a new, required `--registry` flag, which accepts a URL, for example `http://my.jfrog.io/artifactory`
- `buffrs login` has renamed `--url` to `--registry` for consistency
- The credentials.toml file is different. When it only supported a single registry, it looked like this:

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

- `buffrs login` no longer supports the `--username` flag, as we no longer use BasicAuth. Instead we set the `X-JFrog-Art-Api` header.