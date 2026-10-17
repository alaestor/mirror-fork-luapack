use anyhow::{Context, Result};
use full_moon::ast::{Call, Expression, FunctionArgs, FunctionCall, Prefix, Suffix};
use full_moon::tokenizer::TokenType;
use full_moon::visitors::Visitor;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Bundler {
    #[cfg(feature = "dynamic")]
    root: PathBuf,
    path_ids: HashMap<PathBuf, String>,
    processed: HashSet<PathBuf>,
    pub output: String,
}

impl Bundler {
    pub fn bundle(entry: PathBuf) -> Result<String> {
        let mut bundler = Self {
            #[cfg(feature = "dynamic")]
            root: Self::find_root(&entry)?,
            path_ids: HashMap::new(),
            processed: HashSet::new(),
            output: String::with_capacity(8192),
        };

        // The packer has an issue with the module name in this function declaration ... for some reason.
        // I'm just going to replace it with c and call it day. That's because of another bug in the way variables are declared.
        // The compression improvements aren't big enough for me to fix them so I won't.
        bundler.output.push_str("require = function(c)\n");
        #[cfg(feature = "lazy")]
        {
            bundler
                .output
                .push_str("if type(__M[c]) == 'function' then\n");
            bundler.output.push_str("local result=__M[c]()\n");
            bundler.output.push_str("__M[c] = result\n");
            bundler.output.push_str("return result\n");
            bundler.output.push_str("end\n");
            bundler.output.push_str("return __M[c]\n");
        }
        #[cfg(not(feature = "lazy"))]
        bundler.output.push_str("return __M[c]()\n");
        bundler.output.push_str("end\n");
        bundler.output.push_str("__M={");

        let canonical = entry.canonicalize()?;
        let content = fs::read_to_string(&canonical)?;
        // I really don't want to modify process_content just for this change soo
        bundler.processed.insert(canonical.clone());

        let mut queue = VecDeque::new();
        let restore = bundler.output.clone();
        bundler.output.clear();

        let parent = canonical.parent().unwrap_or(Path::new(""));
        bundler.process_content(&content, parent, &mut queue)?;
        // content and parent are both dropped by this point, thus their names can now be reused in the function namespace

        // Also a process_content hack
        let entrypoint = bundler.output.clone();
        bundler.output = restore;

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

        // Don't fix what ain't broken
        bundler.output.push_str("}");
        bundler.output.push_str(&entrypoint);

        Ok(bundler.output)
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

        #[cfg(feature = "dynamic")]
        {
            let mut dirs_to_scan = VecDeque::new();
            dirs_to_scan.push_back(parent.to_path_buf());

            while let Some(current_dir) = dirs_to_scan.pop_front() {
                if let Ok(entries) = fs::read_dir(&current_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().map_or(false, |e| e == "lua") {
                            if let Ok(canonical) = path.canonicalize() {
                                self.try_queue(canonical, queue)?;
                            }
                        } else if path.is_dir() {
                            dirs_to_scan.push_back(path);
                        }
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
}

impl Visitor for StaticRequires {
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
