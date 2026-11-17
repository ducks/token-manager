{ pkgs ? import (builtins.fetchTarball {
  url = "https://github.com/NixOS/nixpkgs/archive/9fbb54b33e91ee4ca368e35a78e0613c720600b3.tar.gz";
  sha256 = "sha256-cV5xEJJK3BvhU8rEd4mC9UsmDi5qscv/kzGPhBRC5WA=";
}) {} }:
pkgs.mkShell {
  packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.gnumake pkgs.git pkgs.python3 pkgs.libsecret ];
}
