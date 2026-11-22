# Minimal obake dev shell for building the vendored Obake (gui/).
# Obake's own flake devshell pre-builds a docs tool through Nix (slow, crashed the
# daemon here), so this provides only the system libraries plus a fast linker.
# Rust itself comes from rustup (rust-toolchain.toml).
# GPU libraries (Vulkan loader, libglvnd) are deliberately NOT provided: they are
# loaded at runtime and must be the host's, or the host GPU drivers won't load.
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  name = "obake-gui";
  nativeBuildInputs = with pkgs; [ pkg-config cmake perl protobuf clang mold ];
  buildInputs = with pkgs; [
    curl fontconfig freetype libgit2 openssl sqlite zlib zstd
    alsa-lib glib libva libxkbcommon wayland
    libx11 libxcb libdrm libgbm libxcomposite libxdamage
    libxext libxfixes libxrandr
  ];
}
