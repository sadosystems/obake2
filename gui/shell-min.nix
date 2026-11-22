# Minimal obake dev shell for building the vendored Zed (gui/).
# Zed's own flake devshell pre-builds a docs tool through Nix (slow, crashed the
# daemon here), so this provides only the system libraries plus a fast linker.
# Rust itself comes from rustup (rust-toolchain.toml).
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  name = "obake-gui";
  nativeBuildInputs = with pkgs; [ pkg-config cmake perl protobuf clang mold ];
  buildInputs = with pkgs; [
    curl fontconfig freetype libgit2 openssl sqlite zlib zstd
    alsa-lib glib libva libxkbcommon wayland vulkan-loader libglvnd
    libx11 libxcb libdrm libgbm libxcomposite libxdamage
    libxext libxfixes libxrandr
  ];
  LD_LIBRARY_PATH = with pkgs; lib.makeLibraryPath [ vulkan-loader libglvnd wayland libxkbcommon libx11 libxcb ];
}
