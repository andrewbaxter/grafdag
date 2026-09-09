# Build with `nix-build package.nix` (or `pkgs.callPackage ./package.nix { }`).
#
# 1. Build the web widget to wasm.
# 2. Stage the static files (wasm-bindgen output + static/).
# 3. Build the server with the staging directory embedded.
{ pkgs ? import <nixpkgs> { } }:
let
  lib = pkgs.lib;
  craneSrc = builtins.fetchTarball {
    url = "https://github.com/ipetkov/crane/archive/refs/tags/v0.24.0.tar.gz";
    sha256 = "06y3dvs9ms4c70r2ia50k77650yrihc09jibqbqrzly23dn42s4m";
  };
  craneLib = import craneSrc { inherit pkgs; };
  # `commonCargoSources` only picks up Rust sources; Tauri also needs its config
  # and the window icon at compile time (`generate_context!`).
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      (craneLib.fileset.commonCargoSources ./.)
      ./crates/grafdag/tauri.conf.json
      ./crates/grafdag/icons
    ];
  };
  commonArgs = {
    inherit src;
    strictDeps = true;
  };

  # Web widget (wasm)
  wasmArgs = commonArgs // {
    pname = "grafdag_web";
    version = "0.1.0";
    cargoExtraArgs = "--locked -p grafdag_web";
    CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
    doCheck = false;
    nativeBuildInputs = [ pkgs.lld ];
  };
  wasmDeps = craneLib.buildDepsOnly wasmArgs;
  wasm = craneLib.buildPackage (wasmArgs // {
    cargoArtifacts = wasmDeps;
    installPhaseCommand = ''
      mkdir -p $out
      cp target/wasm32-unknown-unknown/release/grafdag_web.wasm $out/
    '';
  });

  # Staging directory with everything the server embeds
  staging = pkgs.runCommand "grafdag-static"
    {
      nativeBuildInputs = [ pkgs.wasm-bindgen-cli ];
    } ''
    mkdir -p $out
    wasm-bindgen --target web --no-typescript --out-dir $out ${wasm}/grafdag_web.wasm
    cp -r ${./static}/. $out/
  '';

  # The app binary: local server plus either a desktop window or the browser
  serverArgs = commonArgs // {
    pname = "grafdag";
    version = "0.1.0";
    cargoExtraArgs = "--locked -p grafdag";
    env.GRAFDAG_STATIC_DIR = "${staging}";
    # The desktop window is a Tauri/WRY webview; the browser mode (--browser)
    # doesn't use these, but it's one binary so they're always linked.
    nativeBuildInputs = [ pkgs.pkg-config pkgs.wrapGAppsHook3 ];
    buildInputs = [
      pkgs.webkitgtk_4_1
      pkgs.gtk3
      pkgs.libsoup_3
      pkgs.glib
      pkgs.cairo
      pkgs.pango
      pkgs.gdk-pixbuf
      pkgs.atk
      pkgs.librsvg
      pkgs.openssl
    ];
  };
  serverDeps = craneLib.buildDepsOnly serverArgs;
in
craneLib.buildPackage (serverArgs // {
  cargoArtifacts = serverDeps;
  passthru = {
    inherit wasm staging;
  };
  meta = {
    description = "Interactive DAG visualizer and editor";
    mainProgram = "grafdag";
  };
})
