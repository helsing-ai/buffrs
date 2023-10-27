## Protocol Buffer Rules

This specification defines rules enforced by Buffrs to prevent package
colisions, and provide uniformity and transparency for package consumers.

Rules with the `00XX` code define the package and filesystem layout, where as
rules with a `01XX` code enforce certain protocol buffer definition rules.

### `0000` – Package Name Prefix / Symmetry

Enforces that the Buffrs Package ID is used as the prefix for all protocol
buffer package declarations.

So given a Buffrs Package with the ID `physics` this enforces that the package
only contains protocol buffer package declarations matching
`physics|physics.*`;

A violation would cause type colisions and ambiguity when trying to resolve a
type.

### `0010` – Sub-Package Declaration

Enforces that subpackages are declared through a sensible folder
structure. Given a Buffrs Package with the ID `physics` the protocol buffer
file that declares `package physics.units;` has to be called
`proto/units.proto`.

Nested subpackages are represented / grouped through folders. So if one wants
to declare `package physics.units.temperature;` the respective file must be
located at `proto/units/temperature.proto`.

### `0020` – Root Package Declaration

Enforces that only one file at a time declares the _root_ package.

Namely: If a Buffrs Package with the ID `physics` is defined, the
`proto/physics.proto` must declare the the same package in the protocol buffer
syntax through `package physics;`.
