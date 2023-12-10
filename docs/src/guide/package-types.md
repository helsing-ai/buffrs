## Package Types

Buffrs makes distinctions between two different packages:

```toml
[package]
type = "lib" | "api"
```

This is used in order to fuel composition and type reuse across APIs and thus
enable shared types + wire compatibility.

### `lib` – Libraries

Libraries contain atomic and composite type definitions that describe a domain.
(e.g. `physics`, `auth`, `time` etc.). This pattern is really useful for
scaling systems and maintaining dozens of APIs as they can share types and thus
get the aforementioned benefits of source code and wire compatibility for free.

An example of a proto library named `time` that depends on `google`:

```toml
[package]
name = "time"
type = "lib"
version = "1.0.0"

[dependencies]
google = { version = "=1.0.0", ... }
```

```proto
syntax = "proto3";

package time;

import "google/timestamp.proto";

/// A timestamp wrapper for various formats
message Time {
  oneof format {
    string rfc3339 = 1;
    uint64 unix = 2;
    google.protobuf.Timestamp google = 3;
    ..
  }
}
```

### `api` – APIs

APIs are the next logical building block for real world systems – they define
services and RPCs that your server can implement. You can use the
aforementioned libraries to fuel your development / api definition experience.

A good example of an API could be an imaginary `logging` service that makes use
of the just declared `time.Time`:

```toml
[package]
name = "logging"
type = "api"
version = "1.0.0"

[dependencies]
time = { version = "=1.0.0", ... }
```

```proto
syntax = "proto3";

package logging;

import "time/time.proto";

service Logging {
  rpc critical(LogInput) returns (LogOutput);
  rpc telemetry(LogInput) returns (LogOutput);
  rpc healthiness(HealthInput) returns (HealthOutput);
}

message LogInput { string context = 1; time.Time timestamp = 2; }
message LogOutput { }

message HealthInput { bool db = 1; }
message HealthOutput { }
```
