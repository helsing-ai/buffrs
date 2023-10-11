#!/bin/bash

for file in *.proto; do
    cargo run --features tools --bin parse -- --format json "$file" | jq | tee $(basename "$file" .proto).json
done
