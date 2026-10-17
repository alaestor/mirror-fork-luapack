<div align="center">

# Lua Pack

**A Lua bundler and minifier written in Rust**

[![Rust](https://img.shields.io/badge/Rust-edition%202024-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MPL--2.0-blue)](LICENSE)

</div>

---

## Overview

luapack combines Lua modules into a single file and minifies source by removing
whitespace and shortening identifiers. Its parser has Luau and CfxLua extensions
enabled.

This is a recovery fork of
[the-unnamed-goose/luapack](https://github.com/the-unnamed-goose/luapack)
by The Unnamed Goose. That repository is no longer available. The published
crate also names
[repetitive-contrarian/luapack](https://github.com/repetitive-contrarian/luapack),
which is unavailable too.

The source was recovered from the published **0.1.1** crate, the same version
used by our Nix package. Git history was not recovered. See
[RECOVERY.md](RECOVERY.md) for provenance and
[README.upstream.md](README.upstream.md) for the original documentation.

---

## Features

| Feature | Description |
| --- | --- |
| Bundling | Combines modules referenced by `require()` into a single Lua file |
| Minification | Removes whitespace and shortens identifiers |
| Lua extensions | Parses Luau and CfxLua syntax |
| Prefix and suffix | Adds files before and after the generated output |
| Dynamic discovery | Includes Lua files for runtime requires, enabled at compile time |
| Module caching | Caches module return values with the optional `lazy` feature |

---

## Installation

Use a Rust toolchain that supports edition 2024.

```bash
cargo install --path . --locked
```

---

## Quick start

Bundle and minify an entry point and its modules:

```bash
luapack --file main.lua --output dist.lua
```

Bundle without minifying:

```bash
luapack --bundle --file main.lua --output bundled.lua
```

Minify a file without bundling:

```bash
luapack --pack --file script.lua --output minified.lua
```

---

## CLI reference

```text
Usage: luapack [OPTIONS] --output <OUTPUT> --file <FILE>

  -p, --prefix <PREFIX>  Prefix file to prepend
  -s, --suffix <SUFFIX>  Suffix file to append
  -o, --output <OUTPUT>  Output file
  -f, --file <FILE>      Input file
  -l, --lz4              Compress with LZ4, if compiled with the lz4 feature
  -B, --bundle           Bundle modules
  -P, --pack             Minify source
  -h, --help             Print help
  -V, --version          Print version
```

Without `--bundle` or `--pack`, both operations run.

---

## Build options

| Cargo feature | Default | Behavior |
| --- | --- | --- |
| `lz4` | Enabled | Exposes the `--lz4` option. See the limitation below. |
| `dynamic` | Disabled | Includes Lua files discovered under a Git project root for runtime requires. |
| `lazy` | Disabled | Caches module return values. This can change behavior for modules expected to run repeatedly. |

For example:

```bash
cargo install --path . --locked --features dynamic,lazy
```

---

## Limitations

`--lz4` writes a size-prefixed LZ4 block. No decompressor is included. Prefix
and suffix files remain verbatim outside that block, so they can provide a
decompressor wrapper.

Dynamic discovery requires a Git repository and scans from its root once.
It skips `.git`, symlinks, and the selected output file. A Lua file whose first
line starts with `-- ignore` is excluded from bundling.

Without `lazy`, bundled modules execute on every require. With `lazy`, each
module's first successful return value is cached, including functions, nil,
and false. Circular bundled imports raise an error with `lazy` enabled.

Static import rewriting supports unescaped string arguments to `require`.
Escaped paths and locally shadowed `require` bindings remain limitations.
Regression coverage does not establish semantic equivalence for every Lua
program or extension dialect.

---

## Testing

```bash
nix develop
cargo test --locked --no-fail-fast
cargo test --locked --no-default-features --no-fail-fast
cargo test --locked --all-features --no-fail-fast
```

The CLI suite requires `lua` on `PATH`, supplied by the development shell.
It checks errors and file handling, executes original and generated Lua to
compare behavior, and exercises the optional Cargo features. Run all eight
combinations of `lz4`, `dynamic`, and `lazy` when changing bundling behavior;
an all-features run does not cover the non-dynamic module ID implementation.

---

## License

[MPL-2.0](LICENSE). Original author attribution remains in the recovered source.
