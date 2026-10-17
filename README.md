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

The recovered 0.1.1 source returns before writing the output file when
`--lz4` is set. Avoid that option until it is fixed. No decompressor is included.

Dynamic discovery requires a Git repository. A Lua file whose first line
starts with `-- ignore` is excluded from bundling.

This recovery preserves upstream behavior. It does not establish that
minification preserves semantics for every Lua program.

---

## Testing

```bash
cargo test --locked
cargo test --locked --all-features
```

The recovered crate contains no dedicated test suite. These commands check
compilation and run any tests Cargo discovers.

---

## License

[MPL-2.0](LICENSE). Original author attribution remains in the recovered source.
