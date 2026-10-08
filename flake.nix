{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-parts.url = "github:hercules-ci/flake-parts";

    x52 = {
      url = "github:x52dev/nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-parts.follows = "flake-parts";
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.x52.flakeModules.default ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem =
        { config, pkgs, ... }:
        let
          formatTools = [
            config.formatter
            pkgs.actionlint
            pkgs.fd
            pkgs.jq
            pkgs.just
            pkgs.prettier
            pkgs.taplo
            pkgs.zizmor
          ];
        in
        {
          formatter = pkgs.nixfmt;

          devShells.default = pkgs.mkShellNoCC {
            packages = formatTools ++ [
              pkgs.cargo-nextest
              pkgs.cargo-shear
            ];

            shellHook = config.x52.justRust.shellHook;
          };

          devShells.ci = pkgs.mkShellNoCC {
            packages = [
              pkgs.jq
              pkgs.just
            ];

            shellHook = config.x52.justRust.shellHook;
          };

          devShells.ci-format = pkgs.mkShellNoCC {
            packages = formatTools;

            shellHook = config.x52.justRust.shellHook;
          };
        };
    };
}
