{
  description = "virtual environments";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";

  inputs.devshell.url = "github:numtide/devshell";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };

  inputs.naersk.url = "github:nix-community/naersk";
  inputs.fenix = {
    url = "github:nix-community/fenix";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, flake-utils, devshell, fenix, nixpkgs, naersk, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;

          overlays = [ devshell.overlays.default fenix.overlays.default ];
        };

        toolchain = with fenix.packages.${system};
          combine [
            complete.rustc
            complete.cargo
            complete.clippy
            complete.rust-src
            complete.rustc
            complete.rustfmt
            complete.rust-analyzer
            targets.x86_64-pc-windows-gnu.latest.rust-std
            pkgs.cargo-flamegraph
          ];

        naersk' = naersk.lib.${system}.override {
          cargo = toolchain;
          rustc = toolchain;
        };

        package = mode:
          naersk'.buildPackage {
            src = ./.;
            strictDeps = true;

            depsBuildBuild = with pkgs; [
              pkgsCross.mingwW64.stdenv.cc
              pkgsCross.mingwW64.windows.pthreads
            ];

            nativeBuildInputs = with pkgs; [
              # We need Wine to run tests:
              wineWowPackages.stable
            ];

            doCheck = false;

            copyBins = false;
            copyLibs = true;
            release = false;
            mode = mode;

            TARGET_CC = "x86_64-w64-mingw32-gcc";
            TARGET_CXX = "x86_64-w64-mingw32-g++";

            # Tells Cargo that we're building for Windows.
            # (https://doc.rust-lang.org/cargo/reference/config.html#buildtarget)

            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER =
              pkgs.writeScript "wine-wrapper" ''
                #!/usr/bin/env bash

                echo $TARGET_CC
                export WINEPREFIX="$(mktemp -d)"
                export WINEDLLOVERRIDES=mscoree=d
                exec wine64 $@
              '';
          };
      in {
        packages.default = package "build";
        packages.clippy = package "clippy";
        packages.fmt = package "fmt";
      });
}
