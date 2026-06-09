{
  description = "pleme-actions-shared — shared toolkit for pleme-io GitHub Actions";

  inputs = {
    nixpkgs.follows = "substrate/nixpkgs";
    crate2nix = {
      url = "github:nix-community/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    substrate = {
      url = "github:pleme-io/substrate";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crate2nix,
    flake-utils,
    substrate,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        rustLibrary = import "${substrate}/lib/rust-library.nix" {
          inherit system nixpkgs;
          nixLib = substrate;
          inherit crate2nix;
        };
        lib = rustLibrary {
          name = "pleme-actions-shared";
          src = ./.;
        };
      in {
        packages.default = lib.package;
        checks.tests = lib.tests;
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc cargo cargo-edit clippy rustfmt
            rust-analyzer
          ];
        };
      });
}
