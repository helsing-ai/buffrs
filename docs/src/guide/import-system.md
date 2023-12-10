## Import System

To reuse already defined messages, protobuf files can be imported from both
dependencies and the managed package itself. Unique identification of the files
is made available through the name of the package as declared in ``Proto.toml``
which is used as root of the imports.

For a dependency ``units``:

```
units
├── Proto.toml
├── weight.proto
└── length.proto
```

and a package named ``physic``:

```
physic
├── Proto.toml
└── proto
    ├── root.proto
    ├── length.proto
    └── calculations
        ├── distance.proto
        └── graph.proto
```

messages can be imported from both packages relative to their root:

```proto
// root.proto
syntax = "proto3";

package physic;

import "physic/length.proto";
import "physic/calculations/distance.proto";
import "units/length.proto";

message Lengths {
    units.Meter meter = 1;
    physic.Parsec parsec = 2;
    physic.calculations.Haversine = 3;
}
```
