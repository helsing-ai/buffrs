{ rust-bin }:

rust-bin.stable.latest.default.override {
  extensions = [ "rust-src" "rust-analyzer" ];
  targets = [ "arm-unknown-linux-gnueabihf" ];
}

