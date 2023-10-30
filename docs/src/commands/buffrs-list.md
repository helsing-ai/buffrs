## buffrs list

Lists all protobuf files (`.proto`) managed by Buffrs to standard out.

### Synopsis

`buffrs list|ls`

### Description

This command lists all protobuf files managed by Buffrs. This way the
output can be fed dynamically into external code generation tools like
`protoc` to do customize the behavior of the generator beyond the capabilities
that Buffrs provides out of the box through [`buffrs
generate`](./buffrs-generate.md).

### Example

Given a project that depends on a `physics` package (that provides two `.proto`
files: `temperature.proto` and `mass.proto`). Once it's dependencies are
installed, the structure of the filesystem would look similar to this:

```
.
├── Proto.toml
└── proto
    ├── some.proto
    └── vendor
        └── physics
            ├── Proto.toml
            ├── temperature.proto
            └── mass.proto
```

Using `buffrs ls` you can feed the installed protocol buffer files of all
package dynamically into another command line tool like `protoc` to generate
code, or run lints:

```bash
protoc --cpp_out ./cpp --include proto $(buffrs ls)
```

---

The raw output of `buffrs ls` would return the following three paths:

```toml
proto/some.proto proto/vendor/physics/temperature.proto proto/vendor/physics/mass.proto
```
