{
  description = "agent-isle - general containment environment for AI agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system: nixpkgs.legacyPackages.${system});

      # Convert agent attribute set to compile-time env vars.
      # Example: { opencode = pkgs.opencode; } → { OPENCODE_PATH = "/nix/store/.../bin/opencode"; }
      agentEnvVars = agents:
        nixpkgs.lib.mapAttrs' (name: pkg:
          nixpkgs.lib.nameValuePair
            (nixpkgs.lib.toUpper name + "_PATH")
            "${pkg}/bin/${name}"
        ) agents;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
          mkAgentIsle = { agents ? {}, maskedAgents ? [] }: pkgs.rustPlatform.buildRustPackage {
            pname = "agent-isle";
            version = "0.1.0";
            src = ./.;
            cargoHash = "sha256-fG8IhHGKz9lyW03WHq+Y2ZyyTlMi+hJ4sPaPk0Z0zv0=";
            env = {
              BWRAP_PATH = "${pkgs.bubblewrap}/bin/bwrap";
              BETTERLEAKS_PATH = "${pkgs.betterleaks}/bin/betterleaks";
            } // agentEnvVars agents;
            postInstall = nixpkgs.lib.concatMapStringsSep "\n" (agent:
              "ln -s $out/bin/agent-isle $out/bin/${agent}"
            ) maskedAgents;
          };
          agent-isle = mkAgentIsle {};
        in
        {
          inherit agent-isle mkAgentIsle;
          default = agent-isle;
        }
      );

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgsFor.${system};
        in
        {
          default = pkgs.mkShell {
            name = "agent-isle";
            packages = with pkgs; [
              rustc
              cargo
              clippy
              rustfmt
              cargo-llvm-cov
              llvmPackages.llvm
              betterleaks
              bubblewrap
              pandoc
              cacert
            ];
            env = {
              SSL_CERT_FILE = "$NIX_SSL_CERT_FILE";
              LLVM_COV = "${pkgs.llvmPackages.llvm}/bin/llvm-cov";
              LLVM_PROFDATA = "${pkgs.llvmPackages.llvm}/bin/llvm-profdata";
            };
            shellHook = ''
              git config core.hooksPath githooks 2>/dev/null || true
              alias build-docs='./scripts/build-docs.sh'
              alias build-ai-dev-env='nix print-dev-env > ./scripts/ai-dev-env.sh'
            '';
          };
        }
      );
    };
}
