{
  description = "Project Vela — Rust development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust toolchain and editor support
              cargo
              clippy
              rust-analyzer
              rustc
              rustfmt

              # Rust quality and dependency tooling
              cargo-audit
              cargo-deny
              cargo-edit
              cargo-llvm-cov
              cargo-nextest

              # Repository, config, and CI quality tooling
              actionlint
              git
              gh
              jq
              just
              nixfmt
              shellcheck
              secretspec
              taplo
              typos

              # Native dependencies expected by the Vela workspace
              pkg-config
              openssl
              sqlite
            ];

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

            shellHook = ''
              # Nix adds the writable dev-shell output to NIX_LDFLAGS. Remove it:
              # linker wrappers split paths containing spaces into invalid arguments.
              project_rpath="-rpath $out/lib"
              export NIX_LDFLAGS="''${NIX_LDFLAGS//$project_rpath/}"
              unset project_rpath

              if [[ $- == *i* ]]; then
                echo "Project Vela development shell"
                echo "Rust $(rustc --version | cut -d' ' -f2) · Cargo $(cargo --version | cut -d' ' -f2)"
                echo "Run 'just --list' once the project recipes are available."
              fi
            '';
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
