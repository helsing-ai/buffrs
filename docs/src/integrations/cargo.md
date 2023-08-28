# Integrating Buffrs with Cargo

To integrate Buffrs into your Cargo workflow, the `buffrs` crate on crates.io
is available. It contains types and functionality to interact with buffrs
programmatically (as opposed to the cli).

To enable your project to interact with buffrs programmatically you need to add
the `buffrs` crate to your `[build-dependencies]` section:

```toml
# ..

[build-dependencies]
buffrs = "<latest>"
```

This tells Cargo to make the `buffrs` crate available within your build scripts
(contained in `build.rs`) and enables us to instruct Cargo to build the Rust
language bindings when your project is compiled via `buffrs::build` an out of
the box build script which utilizes tonic and prost.

`build.rs`:

```rust
fn main() {
    buffrs::build(buffrs::Language::Rust).unwrap();
}
```

Invoking `buffrs::build` will:

1. Download all missing dependencies (enabling your project to just work with
   `cargo run`)
2. Compile locally defined Buffrs packages (if present)
3. Compile all dependencies specified in your `Proto.toml` (if present)
4. Output the language bindings into `proto/build/rust`

## Using the generated bindings

To use the generated rust code within your application code, you can either use
the `buffrs::include!` macro, or use the std version and manually locate the
buffrs module.

```rust
// Using buffrs
mod proto { buffrs::include!(); }

// Using std
mod proto {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proto/build/rust/mod.rs",
    ))
}
```
