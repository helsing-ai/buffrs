{ fenix, system }:

fenix.packages.${system}.stable.withComponents [
  "cargo"
  "clippy"
  "rust-analyzer"
  "rust-src"
  "rustc"
  "rustfmt"
]
