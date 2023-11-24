## First Steps with Buffrs

This section gives you a quick intro to the `buffrs` command line
interface. Let us take a look at its ability to declare protocol buffer
dependencies and to publish new packages to
the registry.

To initialize a new project with Buffrs, use [`buffrs init`](../commands/buffrs-init.md):

```bash
$ mkdir web-server && cd web-server
$ buffrs init --api
```

> Note: By omitting the `--api` flag (or `--lib` flag respectively) you
> instruct Buffrs to not declare a local package and setup the project to be a
> consumer-only (e.g. a server implementation).



```bash
$ tree .
.
├── Proto.toml
└── proto
    └── vendor

2 directories, 1 file
```

This is all we need to get started. Now let’s check out the newly created `Proto.toml`:

```toml
[package]
name = "web-server"
version = "0.1.0"
type = "api"

[dependencies]
```

This is called a Buffrs Manifest, and it contains all of the metadata that
Buffrs needs to know about your package to install dependencies and distribute
your protocol buffers as a package.

Let us define a dependency of the webserver on a hypothetical library
called `user` in the `datatypes` repository.

This is done by invoking [`buffrs add`](../commands/buffrs-add.md):

```bash
$ buffrs add --registry https://your.registry.com datatypes/user@=0.1.0
```

The result is a dependency in the `Proto.toml`:

```toml
[dependencies.user]
version = "=0.1.0"
repository = "datatypes"
registry = "https://your.registry.com/"
```

### Going further

For more details on using Buffrs, check out the [Buffrs Guide](../guide/index.md)
