use anyhow::{Context, Result};
use full_moon::ast::{Call, Expression, FunctionArgs, FunctionCall, Prefix, Suffix};
use full_moon::tokenizer::{Token, TokenType};
use full_moon::visitors::Visitor;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Bundler {
    #[cfg(feature = "dynamic")]
    root: PathBuf,
    path_ids: HashMap<PathBuf, String>,
    processed: HashSet<PathBuf>,
    used_names: HashSet<String>,
    pub output: String,
}

impl Bundler {
    pub fn bundle(entry: PathBuf, output: &Path) -> Result<String> {
        #[cfg(not(feature = "dynamic"))]
        let _ = output;
        let mut bundler = Self {
            #[cfg(feature = "dynamic")]
            root: Self::find_root(&entry)?,
            path_ids: HashMap::new(),
            processed: HashSet::new(),
            used_names: HashSet::new(),
            output: String::with_capacity(8192),
        };

        let canonical = entry.canonicalize()?;
        let content = fs::read_to_string(&canonical)?;
        bundler.processed.insert(canonical.clone());

        let mut queue = VecDeque::new();
        let parent = canonical.parent().unwrap_or(Path::new(""));
        bundler.process_content(&content, parent, &mut queue)?;
        #[cfg(feature = "dynamic")]
        bundler.discover_modules(output, &mut queue)?;
        let entrypoint = std::mem::take(&mut bundler.output);
        bundler.output.push('{');

        let mut first = true;
        while let Some(path) = queue.pop_front() {
            if bundler.processed.contains(&path) {
                continue;
            }

            let content = fs::read_to_string(&path)?;
            if content
                .lines()
                .next()
                .map_or(false, |l| l.starts_with("-- ignore"))
            {
                bundler.processed.insert(path);
                continue;
            }

            let module_id = bundler.generate_id(&path, &content);
            bundler.path_ids.insert(path.clone(), module_id.clone());
            bundler.processed.insert(path.clone());

            if !first {
                bundler.output.push(',');
            }
            first = false;

            bundler
                .output
                .push_str(&format!("[\"{}\"]=function()", module_id));

            let parent = path.parent().unwrap_or(Path::new(""));
            bundler.process_content(&content, parent, &mut queue)?;

            bundler.output.push_str("end");
        }

        bundler.output.push_str("}");
        let modules = std::mem::take(&mut bundler.output);
        let table = bundler.fresh_name("__luapack_modules");
        let native = bundler.fresh_name("__luapack_native_require");
        let mut loader = format!("local {native}=require\nlocal {table}\n");
        #[cfg(feature = "lazy")]
        let (cache, state) = {
            let cache = bundler.fresh_name("__luapack_cache");
            let state = bundler.fresh_name("__luapack_state");
            loader.push_str(&format!("local {cache},{state}={{}},{{}}\n"));
            (cache, state)
        };
        loader.push_str(&format!(
            "local require\nrequire=function(c)\n\
             if {table}[c]==nil then return {native}(c) end\n"
        ));
        #[cfg(feature = "lazy")]
        loader.push_str(&format!(
            "if {state}[c]==true then return {cache}[c] end\n\
             if {state}[c]==false then error('circular bundled require: '..c) end\n\
             {state}[c]=false\nlocal ok,result=pcall({table}[c])\n\
             if not ok then {state}[c]=nil;error(result,0) end\n\
             {cache}[c]=result\n{state}[c]=true\nreturn result\n"
        ));
        #[cfg(not(feature = "lazy"))]
        loader.push_str(&format!("return {table}[c]()\n"));
        loader.push_str(&format!("end\n{table}={modules}\n{entrypoint}"));
        Ok(loader)
    }

    fn fresh_name(&mut self, base: &str) -> String {
        let mut candidate = base.to_string();
        let mut suffix = 0;
        while !self.used_names.insert(candidate.clone()) {
            suffix += 1;
            candidate = format!("{base}_{suffix}");
        }
        candidate
    }

    #[cfg(feature = "dynamic")]
    fn find_root(entry: &Path) -> Result<PathBuf> {
        let mut current = entry
            .canonicalize()?
            .parent()
            .ok_or_else(|| {
                anyhow::anyhow!("Cannot find repository root. Have you run `git init`?")
            })?
            .to_path_buf();

        loop {
            let dir = current.join(".git");
            if dir.exists() {
                return Ok(current);
            }

            current = current
                .parent()
                .ok_or_else(|| {
                    anyhow::anyhow!("Cannot find repository root. Have you run `git init`?")
                })?
                .to_path_buf();
        }
    }

    #[cfg(feature = "dynamic")]
    fn generate_id(&self, path: &Path, _: &str) -> String {
        let relative = path.strip_prefix(&self.root).unwrap();
        relative.to_string_lossy().replace('\\', "/")
    }

    #[cfg(not(feature = "dynamic"))]
    fn generate_id(&self, path: &Path, _: &str) -> String {
        self.path_ids
            .get(path)
            .cloned()
            .unwrap_or_else(|| format!("m{}", self.path_ids.len()))
    }

    fn try_queue(&mut self, canonical: PathBuf, queue: &mut VecDeque<PathBuf>) -> Result<bool> {
        if self.path_ids.contains_key(&canonical) || self.processed.contains(&canonical) {
            return Ok(self.path_ids.contains_key(&canonical));
        }

        let file_content = fs::read_to_string(&canonical)
            .with_context(|| format!("Cannot read local dependency {}", canonical.display()))?;
        if file_content
            .lines()
            .next()
            .map_or(false, |l| l.starts_with("-- ignore"))
        {
            return Ok(false);
        }

        let module_id = self.generate_id(&canonical, &file_content);
        self.path_ids.insert(canonical.clone(), module_id);
        queue.push_back(canonical);
        Ok(true)
    }

    fn process_content(
        &mut self,
        content: &str,
        parent: &Path,
        queue: &mut VecDeque<PathBuf>,
    ) -> Result<()> {
        let ast = full_moon::parse(content)
            .map_err(|errors| anyhow::anyhow!("Parse error: {errors:?}"))?;
        let mut requires = StaticRequires::default();
        requires.visit_ast(&ast);
        self.used_names.extend(requires.identifiers);
        requires.calls.sort_by_key(|(start, _, _)| *start);
        let mut copied = 0;
        for (start, end, path) in requires.calls {
            if path.starts_with('.') {
                let resolved = parent.join(&path);
                let resolved_with_ext = if resolved.extension().is_none() {
                    resolved.with_extension("lua")
                } else {
                    resolved
                };

                let canonical = resolved_with_ext.canonicalize().with_context(|| {
                    format!(
                        "Cannot resolve local dependency {path:?} from {}",
                        parent.display()
                    )
                })?;
                if self.try_queue(canonical.clone(), queue)? {
                    let module_id = self.path_ids.get(&canonical).unwrap();
                    self.output.push_str(&content[copied..start]);
                    self.output.push_str(&format!("\"{}\"", module_id));
                    copied = end;
                }
            }
        }
        self.output.push_str(&content[copied..]);
        self.output.push('\n');

        Ok(())
    }

    #[cfg(feature = "dynamic")]
    fn discover_modules(&mut self, output: &Path, queue: &mut VecDeque<PathBuf>) -> Result<()> {
        // Only an existing output can be discovered. Canonicalization also excludes
        // outputs reached through a symlink in their parent path.
        let output = output.canonicalize().ok();
        let mut dirs = VecDeque::from([self.root.clone()]);
        while let Some(dir) = dirs.pop_front() {
            let mut entries = fs::read_dir(&dir)
                .with_context(|| format!("Cannot scan {}", dir.display()))?
                .collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let kind = entry.file_type()?;
                // Do not follow directory cycles or discover files outside the root.
                if kind.is_symlink() || entry.file_name() == ".git" {
                    continue;
                }
                let path = entry.path();
                if kind.is_dir() {
                    dirs.push_back(path);
                } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "lua") {
                    let canonical = path.canonicalize()?;
                    if output.as_ref() != Some(&canonical) {
                        self.try_queue(canonical, queue)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct StaticRequires {
    calls: Vec<(usize, usize, String)>,
    identifiers: HashSet<String>,
}

impl Visitor for StaticRequires {
    fn visit_identifier(&mut self, token: &Token) {
        self.identifiers.insert(token.to_string());
    }

    fn visit_function_call(&mut self, call: &FunctionCall) {
        if !matches!(call.prefix(), Prefix::Name(name) if name.token().to_string() == "require") {
            return;
        }
        let Some(Suffix::Call(Call::AnonymousCall(arguments))) = call.suffixes().next() else {
            return;
        };
        let string = match arguments {
            FunctionArgs::String(string) => string,
            FunctionArgs::Parentheses { arguments, .. } => {
                let Some(Expression::String(string)) = arguments.iter().next() else {
                    return;
                };
                string
            }
            _ => return,
        };
        if let TokenType::StringLiteral { literal, .. } = string.token().token_type() {
            // Escaped names need Lua string decoding, not raw-text path matching.
            if !literal.contains('\\') {
                self.calls.push((
                    string.token().start_position().bytes(),
                    string.token().end_position().bytes(),
                    literal.to_string(),
                ));
            }
        }
    }
}
