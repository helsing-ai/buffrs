## Package Types

Buffrs currently defines three kinds of package: `lib` (library), `api` (API)
and `impl` (implementation). Only the first two can publish packages, but it's
important to understand all three as they interact with each other in unique
ways.

### Libraries

Projects that declare type `lib` in their package manifest publish library
packages. Library packages are the most basic kind of package, as they only
define primitive types and can have no dependencies. Their purpose is to define
a base layer of common types that can be reused by multiple API or
implementation packages. This increases compatibility across services by
providing a common framework for data representation. It also reduces upgrade
issues across evolving projects, as library dependencies tend to change little
over time.

### APIs

Projects that declare type `api` in their package manifest publish API
packages. API packages, like library packages, are intrinsically declarative.
Their distinction is that they are used to define message and services, as
opposed to just types. API packages can depend on library packages, so they
work with the [`install`](../commands/buffrs-install.md) command, but they
don't produce code, so they don't work with the
[`generate`](../commands/buffrs-generate.md) command. Implementation packages
typically depend on API packages and not directly on libraries, though this is
also allowed.

### Implementations

Implementation projects, unlike the other kinds, don't publish packages, so
they cannot be referenced as dependencies in packages. However, they do define
a package type that can depend on both library and API packages. They are final
consumers of reusable protocol buffers and contain the physical implementation
of services, or clients and servers -- so they are the intended target of the
[`generate`](../commands/buffrs-generate.md) command.
