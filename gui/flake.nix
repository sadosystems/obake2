{
  description = "Obake GUI build environment: the toolchain of rust-toolchain.toml and the system libraries";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
    in {
      # The same libraries as shell-min.nix. GPU libraries (Vulkan loader,
      # libglvnd) are left out: they are loaded at run time and must be the
      # host's, for the host's drivers to load.
      devShells.${system}.default = pkgs.mkShell {
        name = "obake-gui";
        nativeBuildInputs = [ rust ] ++ (with pkgs; [ pkg-config cmake perl protobuf clang mold gcc binutils gnumake git ]);
        buildInputs = with pkgs; [
          curl fontconfig freetype libgit2 openssl sqlite zlib zstd
          alsa-lib glib libva libxkbcommon wayland
          libx11 libxcb libdrm libgbm libxcomposite libxdamage
          libxext libxfixes libxrandr
        ];
      };
    };
}
