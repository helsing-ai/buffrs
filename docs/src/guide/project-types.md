## Project Types

Buffrs currently defines three kinds of project: `lib` (Library), `api` (API) and `impl` (Implementation). Only the first two can publish packages, but it's important to understand all three as they interact with each other in unique ways.

### Library projects

Library projects are the most basic kind of project, as they only define primitive types and can have no dependencies. Their purpose is to define a base layer of common types that can be reused by multiple API or implementation projects. This increases compatibility across services by providing a common framework for data representation. It also reduces upgrade issues across evolving projects, as library dependencies tend to change little over time. Library projects publish `lib` packages.

### API projects

API projects, like library projects, are intrinsically declarative. Their distinction is that they are used to define message and services, as opposed to just types. API projects can depend on library packages, so they work with the [`install`](../commands/buffrs-install.md) command, but they don't produce code, so they don't work with the [`generate`](../commands/buffrs-generate.md) command. Implementation projects typically depend on API projets and not directly libraries, though this is allowed. API projects publish `api` packages.

### Implementation projects

Implementation projects, unlike the other kinds, don't produce packages, so they cannot be added as dependencies of other projects. However, they can depend on both `lib` and `api` packges. They are final consumers of reusable protocol buffers and contain the physical implementation of services, or clients and servers -- so they are the intended target of the [`generate`](../commands/buffrs-generate.md) command.