use anyhow::Result;
use regex::Regex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(not(feature = "dynamic"))]
use std::collections::hash_map::DefaultHasher;
#[cfg(not(feature = "dynamic"))]
use std::hash::{Hash, Hasher};

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
        bundler.process_content(&content, parent, &mut queue);
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
            bundler.process_content(&content, parent, &mut queue);

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
            .expect("Cannot find parent folder. Have you ran `git init`?")
            .to_path_buf();

        loop {
            let dir = current.join(".git");
            if dir.exists() {
                return Ok(current);
            }

            // I sure do love repeating stupid error messages
            current = current
                .parent()
                .expect("Cannot find parent folder. Have you ran `git init`?")
                .to_path_buf();
        }
    }

    #[cfg(feature = "dynamic")]
    fn generate_id(&self, path: &Path, _: &str) -> String {
        let relative = path.strip_prefix(&self.root).unwrap();
        relative.to_string_lossy().replace('\\', "/")
    }

    #[cfg(not(feature = "dynamic"))]
    fn generate_id(&self, path: &Path, content: &str) -> String {
        let mut hasher = DefaultHasher::new();
        content
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n")
            .hash(&mut hasher);
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .hash(&mut hasher);

        // 16 bits is roughly equal to 65,536 combinations in case anyone was wondering
        // I'm not sure on how high the risk of collision is considering this is not the full hash size tho
        format!("m{:04x}", hasher.finish() & 0xFFFF)
    }

    fn try_queue(&mut self, canonical: PathBuf, queue: &mut VecDeque<PathBuf>) -> bool {
        if self.path_ids.contains_key(&canonical) || self.processed.contains(&canonical) {
            return self.path_ids.contains_key(&canonical);
        }

        if let Ok(file_content) = fs::read_to_string(&canonical) {
            if file_content
                .lines()
                .next()
                .map_or(false, |l| l.starts_with("-- ignore"))
            {
                return false;
            }

            let module_id = self.generate_id(&canonical, &file_content);
            self.path_ids.insert(canonical.clone(), module_id);
            queue.push_back(canonical);
            return true;
        }
        false
    }

    fn process_content(&mut self, content: &str, parent: &Path, queue: &mut VecDeque<PathBuf>) {
        let re = Regex::new(r#"require\s*\(?\s*["']([^"']+)["']\)?"#).unwrap();

        for line in content.lines() {
            if let Some(caps) = re.captures(line) {
                let path = &caps[1];

                if path.starts_with('.') {
                    let resolved = parent.join(path);
                    let resolved_with_ext = if resolved.extension().is_none() {
                        resolved.with_extension("lua")
                    } else {
                        resolved
                    };

                    if let Ok(canonical) = resolved_with_ext.canonicalize() {
                        if self.try_queue(canonical.clone(), queue) {
                            let module_id = self.path_ids.get(&canonical).unwrap();
                            let replaced = re.replace(line, format!("require(\"{}\")", module_id));
                            self.output.push_str(&replaced);
                            self.output.push('\n');
                            continue;
                        }
                    }
                }
            }

            self.output.push_str(line);
            self.output.push('\n');
        }

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
                                self.try_queue(canonical, queue);
                            }
                        } else if path.is_dir() {
                            dirs_to_scan.push_back(path);
                        }
                    }
                }
            }
        }
    }
}
