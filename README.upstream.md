# Lua Pack
A Rust-based tool for bundling and minifying Lua scripts into monolithic releases.

## Features
- **Bundling**: Combine multiple Lua modules into a single file
- **Minification**: Reduce file size by removing whitespace and shortening identifiers
- **LZ4 Compression**: Optional compression support (requires external decompressor)
- **Dynamic Requires**: Support for runtime module loading (with the `dynamic` feature)
- **Lua Compatibility**: Supports Lua 5.1-5.4, LuaU, and CfxLua syntax

## Installation
Either `cargo install luapack` or `cargo binstall luapack` depending on which is available. Read the Cargo.toml for compilation options.

## Usage
```bash
luapack [OPTIONS] --output <OUTPUT> --file <FILE>
```

### Options
```
  -p, --prefix <PREFIX>  An optional prefix file that will be added to the output
  -s, --suffix <SUFFIX>  An optional suffix file that will be added to the output
  -o, --output <OUTPUT>  Output file path
  -f, --file <FILE>      Input file path
  -B, --bundle           Bundle modules (entry point resolution)
  -P, --pack             Minify the output
  -l, --lz4              Compress output with LZ4
  -h, --help             Print help
  -V, --version          Print version
```

### Examples
**Default (bundle + pack):**
```bash
luapack -f main.lua -o output.lua
```

**Bundle only:**
```bash
luapack -B -f main.lua -o bundled.lua
```

**Pack only (minify existing file):**
```bash
luapack -P -f script.lua -o minified.lua
```

**Bundle, pack, and compress:**
```bash
luapack -l -f main.lua -o output.lua
```

**With a prefix and suffix:**
```bash
luapack -p prefix.lua -s suffix.lua -l -f main.lua -o output.lua
```

## How It Works
### Bundling
The bundler resolves `require()` statements and combines all dependencies into a single file with a custom module loader. Modules are stored in a table and loaded on-demand.

**Input:**
```lua
-- main.lua
local utils = require('./utils')
utils.hello()
```

```lua
-- utils.lua
return {
    hello = function() print("Hello!") end
}
```

**Output:**
```lua
require = function(c)
  return __M[c]()
end
__M={
  ["utils.lua"]=function()
    return {
      hello = function() print("Hello!") end
    }
  end
}
local utils = require("utils.lua")
utils.hello()
```

### Dynamic Requires
With the `dynamic` feature enabled, all `.lua` files in your project directory are automatically included in the bundle, allowing runtime defined dependencies. If you want to exclude a certain Lua file then add `-- ignore` as its first line. Keep in mind that paths are relative to the parent of the first .git directory it can find.

### Packing (Minification)
Just removes whitespaces (wherever possible) and shortens variable names.

### LZ4 Compression
Compresses output using the lz4 algorithm.

## Limitations
- LZ4 decompressor not included - use `-p` and `-s` to add your own
- Dynamic requires increase bundle size significantly
- Requires Git repository for dynamic feature

## Maintenance
This project is minimally maintained - updates will be made if it breaks with new Lua versions or dependencies.

## License
MPLv2