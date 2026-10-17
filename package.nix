{ lib, rustPlatform, lua }:

rustPlatform.buildRustPackage {
  pname = "luapack";
  version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  nativeCheckInputs = [ lua ];

  meta = {
    description = "Lua bundler and minifier";
    homepage = "https://git.0x04.cc/alaestor/fork-luapack";
    license = lib.licenses.mpl20;
    mainProgram = "luapack";
    platforms = lib.platforms.unix;
  };
}
