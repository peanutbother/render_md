{
  description = "A highly optimized Nix Flake for a Rust project with dependency caching";

  inputs = {
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {self, nixpkgs, flake-utils, rust-overlay, crane}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # `cleanCargoSource` keeps only Rust/Cargo files; render_md_obsidian
        # also compiles in its theme (HTML/CSS/JS) and tests against a
        # fixture vault and golden files.
        src = pkgs.lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (builtins.match ".*/crates/render_md_obsidian/(theme|tests)(/.*)?" path != null);
        };

        commonArgs = {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # `render_md` (the CGI binary, from the `render_md_cgi` crate) and
        # `compile_md` (from the `render_md_compile` crate) are each their
        # own crate in the workspace; build each as its own crane package
        # (`-p` selects the workspace member) rather than one `buildPackage`
        # over the whole workspace, so each output only contains its own
        # binary.
        render_md = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "render_md";
          cargoExtraArgs = "-p render_md_cgi --bin render_md";
        });

        render_md_detailed = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "render_md";
          cargoExtraArgs = "-p render_md_cgi --bin render_md --features detailed-errors";
        });

        compile_md = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "compile_md";
          cargoExtraArgs = "-p render_md_compile --bin compile_md";
        });

        compile_vault = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "compile_vault";
          cargoExtraArgs = "-p render_md_obsidian --bin compile_vault";
        });

        compile_md_detailed = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "compile_md";
          cargoExtraArgs = "-p render_md_compile --bin compile_md --features detailed-errors";
        });
      in
      {
        # `nix build` (packages.default) builds all binaries;
        # `nix build .#render_md` / `.#compile_md` / `.#compile_vault` build just one.
        packages = {
          default = pkgs.symlinkJoin {
            name = "render_md-bins";
            paths = [ render_md compile_md compile_vault ];
          };
          inherit render_md render_md_detailed compile_md compile_md_detailed compile_vault;
        };

        apps = {
          render_md = flake-utils.lib.mkApp {
            drv = render_md;
            name = "render_md";
          };
          render_md_detailed = flake-utils.lib.mkApp {
            drv = render_md_detailed;
            name = "render_md";
          };
          compile_md = flake-utils.lib.mkApp {
            drv = compile_md;
            name = "compile_md";
          };
          compile_vault = flake-utils.lib.mkApp {
            drv = compile_vault;
            name = "compile_vault";
          };
          compile_md_detailed = flake-utils.lib.mkApp {
            drv = compile_md_detailed;
            name = "compile_md";
          };
        };

        # Development environment (`nix develop`)
        devShells.default = craneLib.devShell {
          inputsFrom = [ render_md_detailed compile_md ];

          packages = with pkgs; [
            biome
            compile_md_detailed
            render_md_detailed
            rustToolchain
            tailwindcss_4
          ];

          shellHook = ''
            export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library"
          '';
        };
      }
    );
}
