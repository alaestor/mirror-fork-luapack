use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "luapack-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        // Dynamic discovery needs a repository boundary, not Git history.
        fs::create_dir(path.join(".git")).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_luapack"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn build(&self, mode: &[&str]) -> Vec<u8> {
        let mut args = vec!["--file", "main.lua", "--output", "out.lua"];
        args.extend_from_slice(mode);
        let result = self.run(&args);
        assert!(result.status.success(), "{result:?}");
        fs::read(self.0.join("out.lua")).expect("successful invocation must write output")
    }

    fn execute(&self, file: &str) -> Output {
        Command::new("lua")
            .current_dir(&self.0)
            .arg(file)
            .output()
            .expect("Lua must be installed; use nix develop")
    }

    fn equivalent(&self, mode: &[&str]) {
        let original = self.execute("main.lua");
        assert!(original.status.success(), "invalid fixture: {original:?}");
        self.build(mode);
        let bundled = self.execute("out.lua");
        assert!(
            bundled.status.success(),
            "{bundled:?}\n{}",
            fs::read_to_string(self.0.join("out.lua")).unwrap()
        );
        assert_eq!(bundled.stdout, original.stdout);
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn help_and_version() {
    let p = Project::new();
    let help = p.run(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--output"));
    let version = p.run(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn missing_arguments_and_unknown_options_fail() {
    let p = Project::new();
    for args in [
        vec![],
        vec!["--file", "main.lua"],
        vec!["--output", "out.lua"],
        vec!["--unknown"],
    ] {
        let result = p.run(&args);
        assert!(!result.status.success(), "{args:?}");
        assert!(!result.stderr.is_empty());
        assert!(!p.0.join("out.lua").exists());
    }
}

#[test]
fn io_errors_fail_without_creating_output() {
    let p = Project::new();
    p.write("main.lua", "print(42)");
    for args in [
        vec!["-f", "missing.lua", "-o", "out.lua"],
        vec!["-P", "-f", "main.lua", "-o", "out.lua", "-p", "missing.lua"],
        vec!["-P", "-f", "main.lua", "-o", "out.lua", "-s", "missing.lua"],
        vec!["-P", "-f", "main.lua", "-o", "missing/out.lua"],
    ] {
        let result = p.run(&args);
        assert!(!result.status.success(), "{args:?}");
        assert!(!result.stderr.is_empty());
        assert!(!p.0.join("out.lua").exists());
    }
}

#[test]
fn parse_errors_do_not_overwrite_output() {
    let p = Project::new();
    p.write("main.lua", "local = ???");
    p.write("out.lua", "keep me");
    let result = p.run(&["-P", "-f", "main.lua", "-o", "out.lua"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Parse error"));
    assert_eq!(fs::read_to_string(p.0.join("out.lua")).unwrap(), "keep me");
}

#[test]
fn prefix_and_suffix_are_verbatim() {
    let p = Project::new();
    p.write("main.lua", "print('body')");
    p.write("prefix", "-- prefix\n");
    p.write("suffix", "\n-- suffix");
    let output = p.build(&["-P", "-p", "prefix", "-s", "suffix"]);
    assert!(output.starts_with(b"-- prefix\n"));
    assert!(output.ends_with(b"\n-- suffix"));
    p.equivalent(&["-P", "-p", "prefix", "-s", "suffix"]);
}

#[test]
fn empty_input_is_valid() {
    let p = Project::new();
    p.write("main.lua", "");
    for mode in [&["-P"][..], &["-B"][..], &[][..]] {
        p.equivalent(mode);
    }
}

macro_rules! semantic_tests {
    ($($name:ident: $source:expr,)*) => { $(
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("main.lua", $source);
            p.equivalent(&["-P"]);
        }
    )* };
}

semantic_tests! {
    pack_literals: "print(nil, true, false, 42, 'hello', [[long string]])",
    pack_removes_comments: "-- comment\nlocal descriptive = 42 -- trailing\nprint(descriptive)",
    pack_local_initializers: "local value = 7\ndo local value = value + 1; print(value) end\nprint(value)",
    pack_parallel_assignment: "local left, right = 1, 2; left, right = right, left; print(left, right)",
    pack_shadowing: "local value = 1; do local value = 2; print(value) end; print(value)",
    pack_global_collision: "a = 9; local descriptive = 2; print(a, descriptive)",
    pack_function_parameters: "local function sum(left, right) return left + right end; print(sum(2, 3))",
    pack_anonymous_function_scope: "local value = 7; local fn = function(value) return value + 1 end; print(fn(2), value)",
    pack_recursive_function: "local function factorial(n) if n == 0 then return 1 end return n * factorial(n - 1) end; print(factorial(5))",
    pack_closure: "local value = 7; local function make() return function() return value end end; print(make()())",
    pack_numeric_for: "local total = 0; for index = 1, 5 do total = total + index end; print(total)",
    pack_generic_for: "local total = 0; for index, value in ipairs({2, 4, 6}) do total = total + index * value end; print(total)",
    pack_while: "local value = 0; while value < 3 do value = value + 1 end; print(value)",
    pack_repeat_scope: "repeat local value = 3; print(value) until value == 3",
    pack_branches: "local value = 2; if value == 1 then local label = 'one'; print(label) elseif value == 2 then local label = 'two'; print(label) else local label = 'other'; print(label) end",
    pack_fields_and_methods: "local object = {value = 3}; function object:add(amount) return self.value + amount end; print(object:add(4), object.value)",
    pack_unary_minus: "local value = 3; print(value - -2)",
    pack_concat: "local value = 12; print(value .. 'x', 1 .. 2)",
    pack_varargs: "local function values(...) return ... end; print(values(1, 2, 3))",
    pack_call_sugar: "local function emit(value) print(type(value)) end; emit('text'); emit({1, 2})",
}

#[test]
fn pack_shortens_source_and_keeps_valid_syntax() {
    let p = Project::new();
    let source =
        "-- removable comment\nlocal descriptive_variable = 42\nprint(descriptive_variable)\n";
    p.write("main.lua", source);
    let output = p.build(&["-P"]);
    assert!(output.len() < source.len());
    assert!(!String::from_utf8_lossy(&output).contains("removable comment"));
    p.equivalent(&["-P"]);
}

#[test]
fn bundle_nested_relative_modules() {
    let p = Project::new();
    p.write(
        "main.lua",
        "local module = require('./lib/module'); print(module)",
    );
    p.write("lib/module.lua", "return require('../value') + 1");
    p.write("value.lua", "return 41");
    p.equivalent(&["-B"]);
}

#[test]
fn default_and_explicit_combined_modes() {
    let p = Project::new();
    p.write(
        "main.lua",
        "local module = require('./value'); print(module)",
    );
    p.write("value.lua", "return 42");
    p.equivalent(&[]);
    p.equivalent(&["-B", "-P"]);
}

#[test]
fn bundle_multiple_requires_on_one_line() {
    let p = Project::new();
    p.write(
        "main.lua",
        "local one, two = require('./one'), require('./two'); print(one, two)",
    );
    p.write("one.lua", "return 1");
    p.write("two.lua", "return 2");
    p.equivalent(&["-B"]);
}

#[test]
fn bundle_does_not_rewrite_strings_or_comments() {
    let p = Project::new();
    p.write(
        "main.lua",
        "print([[require('./value')]])\n-- require('./value')\nprint('done')",
    );
    p.write("value.lua", "return 42");
    p.equivalent(&["-B"]);
}

#[test]
fn bundle_external_requires_keep_native_loader() {
    let p = Project::new();
    p.write("main.lua", "print(type(require('math')))");
    p.equivalent(&["-B"]);
}

#[test]
fn bundle_duplicate_imports_respect_cache_feature() {
    let p = Project::new();
    p.write(
        "main.lua",
        "require('./value')\nrequire('./value')\nprint(count)",
    );
    p.write("value.lua", "count = (count or 0) + 1\nreturn 42");
    p.build(&["-B"]);
    let result = p.execute("out.lua");
    assert!(result.status.success(), "{result:?}");
    let expected = if cfg!(feature = "lazy") {
        b"1\n"
    } else {
        b"2\n"
    };
    assert_eq!(result.stdout, expected);
}

#[test]
fn bundle_ignored_modules_are_not_embedded() {
    let p = Project::new();
    p.write("main.lua", "print('ok')");
    p.write("ignored.lua", "-- ignore\nthis is not valid Lua: SENTINEL");
    let output = p.build(&["-B"]);
    assert!(!String::from_utf8_lossy(&output).contains("SENTINEL"));
    p.equivalent(&["-B"]);
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_writes_a_size_prefixed_compressed_block() {
    let p = Project::new();
    p.write("main.lua", "print('compress me')");
    let plain = p.build(&["-P"]);
    fs::remove_file(p.0.join("out.lua")).unwrap();
    let compressed = p.build(&["-P", "--lz4"]);
    assert_eq!(
        lz4_flex::decompress_size_prepended(&compressed).unwrap(),
        plain
    );
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_preserves_prefix_and_suffix_outside_compressed_payload() {
    let p = Project::new();
    p.write("main.lua", "print('compress me')");
    p.write("prefix", "PREFIX");
    p.write("suffix", "SUFFIX");
    let plain = p.build(&["-P"]);
    fs::remove_file(p.0.join("out.lua")).unwrap();
    let output = p.build(&["-P", "--lz4", "-p", "prefix", "-s", "suffix"]);
    assert!(output.starts_with(b"PREFIX"));
    assert!(output.ends_with(b"SUFFIX"));
    assert_eq!(
        lz4_flex::decompress_size_prepended(&output[6..output.len() - 6]).unwrap(),
        plain
    );
}

#[cfg(not(feature = "lz4"))]
#[test]
fn lz4_option_is_unavailable_without_feature() {
    let p = Project::new();
    assert!(!p.run(&["--lz4"]).status.success());
    assert!(!String::from_utf8_lossy(&p.run(&["--help"]).stdout).contains("--lz4"));
}

#[cfg(feature = "dynamic")]
#[test]
fn dynamic_require_discovers_unreferenced_modules() {
    let p = Project::new();
    p.write("main.lua", "local name = 'value.lua'; print(require(name))");
    p.write("value.lua", "return 42");
    p.build(&["-B"]);
    let result = p.execute("out.lua");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"42\n");
}

#[cfg(feature = "dynamic")]
#[test]
fn dynamic_requires_repository_with_actionable_error() {
    let p = Project::new();
    fs::remove_dir(p.0.join(".git")).unwrap();
    p.write("main.lua", "print(42)");
    let result = p.run(&["-B", "-f", "main.lua", "-o", "out.lua"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("git init"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
}

#[cfg(feature = "lazy")]
#[test]
fn lazy_caches_function_valued_modules_without_calling_them() {
    let p = Project::new();
    p.write("main.lua", "local first = require('./value')\nlocal second = require('./value')\nprint(type(first), type(second), first == second)");
    p.write("value.lua", "return function() return 42 end");
    p.equivalent(&["-B"]);
}

#[test]
fn invalid_utf8_is_rejected() {
    let p = Project::new();
    fs::write(p.0.join("main.lua"), [0xff, 0xfe]).unwrap();
    let result = p.run(&["-P", "-f", "main.lua", "-o", "out.lua"]);
    assert!(!result.status.success());
    assert!(!p.0.join("out.lua").exists());
}

#[test]
fn extension_syntax_can_be_packed() {
    let p = Project::new();
    // These dialects are parsed, but cannot be executed by standard Lua.
    for source in [
        "local value: number = 42\nprint(value)",
        "local hash = `example`\nprint(hash)",
    ] {
        p.write("main.lua", source);
        let output = p.build(&["-P"]);
        full_moon::parse(std::str::from_utf8(&output).unwrap())
            .expect("packed dialect syntax must parse");
    }
}

#[test]
fn bundle_explicit_extension_and_string_call_syntax() {
    let p = Project::new();
    p.write(
        "main.lua",
        "local value = require './value.lua'\nprint(value)",
    );
    p.write("value.lua", "return 42");
    // Lua's native loader appends .lua itself, so only execute the bundle.
    p.build(&["-B"]);
    let result = p.execute("out.lua");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"42\n");
}

#[test]
fn bundle_distinguishes_same_filename_and_first_twenty_lines() {
    let p = Project::new();
    p.write(
        "main.lua",
        "local one = require('./one/value')\nlocal two = require('./two/value')\nprint(one, two)",
    );
    let common = "-- shared line\n".repeat(20);
    p.write("one/value.lua", &format!("{common}return 1"));
    p.write("two/value.lua", &format!("{common}return 2"));
    p.equivalent(&["-B"]);
}

#[test]
fn bundle_reports_missing_relative_modules() {
    let p = Project::new();
    p.write("main.lua", "require('./missing')");
    let result = p.run(&["-B", "-f", "main.lua", "-o", "out.lua"]);
    assert!(
        !result.status.success(),
        "missing relative dependency silently accepted"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("missing"));
}

#[cfg(feature = "dynamic")]
#[test]
fn dynamic_discovery_uses_repository_root() {
    let p = Project::new();
    p.write(
        "src/main.lua",
        "local name = 'value.lua'; print(require(name))",
    );
    p.write("value.lua", "return 42");
    let result = p.run(&["-B", "-f", "src/main.lua", "-o", "out.lua"]);
    assert!(result.status.success(), "{result:?}");
    let output = p.execute("out.lua");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"42\n");
}
