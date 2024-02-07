{ fenix, system }:

fenix.packages.${system}.stable.withComponents [
  "cargo"
  "clippy"
  "llvm-tools"
  "rust-analyzer"
  "rust-src"
  "rustc"
  "rustfmt"
]
