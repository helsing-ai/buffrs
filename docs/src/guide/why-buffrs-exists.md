## Why Buffrs Exists

Modern gRPC based software platforms make extensive use of protocol buffers to
define and implement inter-service APIs. While the open-source protocol buffers
ecosystem makes it very easy to get started with a few services and a handful
of messages, recurring difficulties occur with scaling gRPC APIs
beyond immediate repository boundaries for two main reasons:

1. Limited tooling for packaging, publishing, and distributing
   protocol buffer definitions.
2. The lack of widely-adopted best practices for API decomposition and reuse.

To overcome these problems we developed Buffrs - a package manager for protocol
buffers. Using Buffrs, engineers package protocol buffer definitions, publish
them to a shared registry, and integrate them seamlessly into their projects
using versioned API dependencies and batteries-included build tool integration.

For a detailed differentiation against
existing approaches (like [buf], [bazel]
and [git submodules]) and architecture deep
dive take a look at the _[announcement
post.]_

[announcement post.]: https://blog.helsing.ai/buffrs-a-package-manager-for-protocol-buffers-1-2-aaf7c00153d2
[buf]: https://buf.build/
[bazel]: https://bazel.build/
[git submodules]: https://git-scm.com/book/en/v2/Git-Tools-Submodules
