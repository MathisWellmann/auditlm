{
  description = "Flake for auditlm";

  inputs = {
    nixpks.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
          toolchain.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "miri"
            ];
            targets = ["x86_64-unknown-linux-gnu"];
          });
        buildInputs = with pkgs; [
          rust
          openssl
          pkg-config
        ];
        rust_tools = with pkgs; [
          cargo-nextest
          taplo # Format `.toml` files.
        ];
        nix_tools = with pkgs; [
          alejandra # Nix code formatter.
          deadnix # Nix dead code checker
          statix # Nix static code checker.
        ];
      in
        with pkgs; {
          devShells.default = mkShell {
            buildInputs = buildInputs ++ rust_tools ++ nix_tools;
            RUST_BACKTRACE = "1";
          };
        }
    );
}
