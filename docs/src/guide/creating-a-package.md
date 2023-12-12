## Creating a Package

Creating a new, consumable, Buffrs package can be done in four steps:

1. Initialize a project using [`buffrs init`](../commands/buffrs-init.md)
2. Set metadata, package type, version correctly and declare the
   [dependencies](./dependencies.md) that you want to use.
3. Declare `message`, `enum` and `service` types you want to publish in the
   newly created `proto` folder using `.proto` files.
4. Publish to a registry using [`buffrs publish`](../commands/buffrs-publish.md).

### Example: Publishing `physics`

#### Initialize Your Project

Start by initializing a new Buffrs project using the buffrs init command. This
will set up the basic structure for your package and allow you to manage your
package's metadata and dependencies.

```bash
mkdir physics
cd physics
buffrs init --lib
```

#### Define Package Metadata and Dependencies

In your project folder, locate the `Proto.toml` file. This is where you'll
specify your package's metadata and dependencies. Open this file in a text
editor.

Here's an example `Proto.toml` file:

```toml
# Package metadata
[package]
package = "physics"
version = "1.0.0"
type = "lib"
description = "A library containing physic related types"

# Declare dependencies (none in this case)
[dependencies]
```

#### Define Message, Enum, and Service Types

Inside your project directory, `buffrs init` created a `proto` folder. This is
where you will store your `.proto` files that define your library or api.

An example file structure:

```text
physics
├── Proto.toml
└── proto
    ├── temperature.proto
    ├── mass.proto
    └── vendor
```

Write your Protocol Buffer definitions in these `.proto` files. Here's a simple
example of the `temperature.proto` file that could be in a physics library:

```protobuf
syntax = "proto3";

package physics.temperature;

// Define temperature units
enum TemperatureUnit {
  CELSIUS = 0;
  FAHRENHEIT = 1;
  KELVIN = 2;
}

// Define a message for temperature
message Temperature {
  double value = 1;            // Temperature value
  TemperatureUnit unit = 2;    // Temperature unit (Celsius, Fahrenheit, Kelvin)
}
```

#### Publish Your Package

Once you've set up your Buffrs package and defined your Protocol Buffer
definitions, you can publish it to a registry using the buffrs publish command.
Make sure you're logged in to the registry if required.

```bash
buffrs publish --registry https://your.registry.com --repository tutorial
```

Your package will be uploaded to the registry, and others can now consume it
using Buffrs.

Congratulations! You've successfully published your Buffrs package. Other
developers can now use your Protocol Buffer definitions by adding your package
as a dependency in their Buffrs projects.

That's it! You've created a Buffrs package that others can easily consume.
Remember to keep your package up-to-date and well-documented to make it even
more valuable to the community.
