# Project Layout

To get an understanding of the project layout that buffers uses it is helpful to
start in a clean manner and introspect the outcome.

Lets create a new clean directory initialize for our `physic` library.

```
$ mkdir physic
$ cd physic
$ buffrs init --lib
```

This will initialize the following project structure:

```
physic
├── Proto.toml
└── proto
    └── vendor
```

This will create the `Proto.toml` file which is the manifest file that buffrs
uses. The `proto` directory, which is the source directory for all your protocol
buffer definitions and the `proto/vendor` directory, which contains external
protocol buffers.

**Important:** The vendor directory is managed by buffrs, all manual changes
will be overridden / cam cause not reproducible behavior.
