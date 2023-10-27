## buffrs lint

Lints your protocol buffers for the ([Buffrs Protocol Buffer
Rules](../reference/protocol-buffer-rules.md)

### Synopsis

`buffrs lint`

### Description

This command lints your local package (defined in `proto/*.proto`) for a set of
rules defined in the ([Buffrs Protocol Buffer
Rules](../reference/protocol-buffer-rules.md)). They contain a set of rules
ranging from style to package layout (like filenaming, package declaration
etc.). This enables a common flavor to Buffrs packages which affect users.

One good example why this is required is the enforcement of euqality between
the package declaration in the protocol buffers files (`*.proto`) and the
Buffrs Package ID. This enables to expect that a Buffrs Package `a` declares
the protocol buffer package `a.*` and prevents type colisions / ambiguity.

### Example

Given a Buffrs Package `abc` that contains a protocol buffer file with the
following file (`proto/xyz.proto`):

```proto
syntax = "proto3";

package xyz;
```

Executing `buffrs lint` would return a rule violation:

```toml
PackageName (https://helsing-ai.github.io/buffrs/rules/PackageName)

  × Make sure that the protobuf package name matches the buffer package name.
  ╰─▶   × package name is xyz but should have abc prefix

   ╭─[xyz.proto:1:1]
   ╰────
  help: Make sure the file name matches the package. For example, a package with the name `package.subpackage` should be stored in `proto/package/subpackage.proto`.
```
