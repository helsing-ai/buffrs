## The Framework

To understand the distribution and decomposition framework that Buffrs provides
it is useful to understand which properties of API management systems are
desirable and why. Key aspects include:

### Versioning

A versioning scheme for APIs — similar to versioned library dependencies —
explicitly encodes compatibility properties. This allows developers to make
either backwards-compatible or breaking changes and encode the compatibility
guarantees in the API version: a minor version upgrade “just works”, while
a new major version API may require manual migration/adaption in the consuming
server/client implementations.

### Source Compatibility

Given versioned protocol buffer APIs with explicit compatibility guarantees, it
is desirable to have a system in which wire-format compatible APIs are also
source-code compatible. This means that engineers can update minor patches
automatically and that APIs that build upon the same protocol buffer types can
be used with the same generated code types. This is especially important in
strict languages like Rust (due to, e.g., the orphan rule).

### Composition

Enabling engineers to reuse and combine code in order to build new
systems from existing building blocks is a key feature. A common composition
scheme with protocol buffers is to use a set of base messages and data types
across many APIs.

### Discoverability

Before engineers can reuse and compose protocol buffers, they
need to be able to discover and understand what APIs exist and how to use them.
Discoverability is a significant accelerator for engineering productivity and
helps developers stay abreast of the evolution of APIs and architecture.

###

---

### Protocol Buffers as a First Class Citizen

Buffrs decided to take a unique approach compared to other protocol buffer
management systems (or systems capable of distributing protocol buffers) like
[buf] and [bazel]. Buffrs is essentially a package manager for protocol
buffers. Protocol buffers are treated as a _first class citizen_
within Buffrs – which means that they are seen as distributable units called
packages.

Complex software projects frequently turn out to depend on different versions
of the same APIs over time and individual components in those systems may have
diverging compatibility guarantees. Internal services may break their API
backwards compatibility, way more frequent than external gateways that serve
millions of users.

The three fundamental ideas of Buffrs to enable a stable management approach
for scenarios like the above are:

#### Buffrs Packages

A closed and meaningful unit of protocol buffers which enables either
productivity through shared types (e.g., Google's `google.protobuf.Timestamp`)
or describes a domain / API (e.g., `service GeoLocator`).

#### Buffrs Registry

A central registry for managing and hosting packages, documentation, enabling
engineers to search and find and to implement access control.

#### Local Code Generation

The last block is local code generation. This enables projects to freely
diverge in their actual implementations by choosing a code generation
implementation which fits their specific needs while still maintaining complete
wire compatibility. This prevents language lock-ins and makes any version of
any package usable for any language that exists today or might exist in the
future (given it has a protobuf code generator).

[buf]: https://buf.build/
[bazel]: https://bazel.build/
