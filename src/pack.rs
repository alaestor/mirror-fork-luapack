use full_moon::ast::*;
use full_moon::node::Node;
use full_moon::tokenizer::{Token, TokenReference, TokenType};
use full_moon::visitors::{VisitMut, VisitorMut};
use std::collections::{HashMap, HashSet};

pub struct Packer {
    counter: usize,
    scopes: Vec<HashMap<String, String>>,
    prev_token: Option<TokenType>,
    used_names: HashSet<String>,
    replacements: HashMap<usize, String>,
}

const RESERVED: [&str; 21] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

impl Packer {
    pub fn new() -> Self {
        Self {
            counter: 0,
            scopes: vec![HashMap::new()],
            prev_token: None,
            used_names: HashSet::new(),
            replacements: HashMap::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn rename(&mut self, name: &str) -> String {
        for scope in self.scopes.iter().rev() {
            if let Some(renamed) = scope.get(name) {
                return renamed.clone();
            }
        }
        name.to_string()
    }

    fn declare(&mut self, name: &str) -> String {
        let new_name = self.allocate_name();
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), new_name.clone());
        }
        new_name
    }

    fn allocate_name(&mut self) -> String {
        loop {
            let candidate = self.to_letter(self.counter);
            self.counter += 1;
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn to_letter(&self, mut n: usize) -> String {
        let index = n;
        // Map the allocation index onto the alphabetic sequence with keywords
        // removed. Skipping only the current keyword would duplicate its successor.
        loop {
            let skipped = RESERVED
                .iter()
                .filter(|word| {
                    let ordinal = word.bytes().fold(0usize, |value, byte| {
                        value * 26 + usize::from(byte - b'a') + 1
                    }) - 1;
                    ordinal <= n
                })
                .count();
            let adjusted = index + skipped;
            if adjusted == n {
                break;
            }
            n = adjusted;
        }
        let mut result = String::new();
        let mut num = n + 1;
        while num > 0 {
            num -= 1;
            result.push((b'a' + (num % 26) as u8) as char);
            num /= 26;
        }
        result.chars().rev().collect()
    }

    fn ident(&self, name: &str) -> TokenReference {
        TokenReference::new(
            vec![],
            Token::new(TokenType::Identifier {
                identifier: name.into(),
            }),
            vec![],
        )
    }

    fn is_alphanum(t: &TokenType) -> bool {
        matches!(
            t,
            TokenType::Identifier { .. }
                | TokenType::Number { .. }
                | TokenType::StringLiteral { .. }
        ) || matches!(t, TokenType::Symbol { symbol } if {
            let s = symbol.to_string();
            s.chars().next().map_or(false, |c| c.is_alphabetic())
        })
    }

    fn needs_space(previous: &TokenType, current: &TokenType) -> bool {
        if Self::is_alphanum(previous) && Self::is_alphanum(current) {
            return true;
        }
        match (previous, current) {
            (TokenType::Symbol { symbol }, TokenType::Number { .. }) => {
                symbol.to_string().ends_with('.')
            }
            (TokenType::Number { .. }, TokenType::Symbol { symbol }) => {
                symbol.to_string().starts_with('.')
            }
            (TokenType::Symbol { symbol }, TokenType::StringLiteral { .. }) => {
                symbol.to_string() == "["
            }
            (TokenType::Symbol { symbol: left }, TokenType::Symbol { symbol: right }) => {
                // Keep operators and long-bracket delimiters from merging.
                // This also prevents Lua and CfxLua comment openers.
                let left = left.to_string();
                let right = right.to_string();
                left.ends_with(|c| "-+*/%^<>=~:.&|?[".contains(c))
                    && right.starts_with(|c| "-+*/%^<>=~:.&|?[".contains(c))
            }
            _ => false,
        }
    }
}

impl Default for Packer {
    fn default() -> Self {
        Self::new()
    }
}

impl VisitorMut for Packer {
    fn visit_ast(&mut self, ast: Ast) -> Ast {
        // Reserve every source identifier before allocating any replacement.
        // This includes globals used only after the first local declaration.
        self.used_names
            .extend(
                ast.nodes()
                    .tokens()
                    .filter_map(|token| match token.token().token_type() {
                        TokenType::Identifier { identifier } => Some(identifier.to_string()),
                        _ => None,
                    }),
            );
        full_moon::visitors::Visitor::visit_ast(&mut BindingResolver::new(self), &ast);
        let nodes = ast.nodes().clone().visit_mut(self);
        let eof = self.visit_eof(ast.eof().clone());
        ast.with_nodes(nodes).with_eof(eof)
    }

    fn visit_token_reference(&mut self, token: TokenReference) -> TokenReference {
        let token = if let Some(name) = self
            .replacements
            .get(&token.token().start_position().bytes())
        {
            self.ident(name)
        } else {
            token
        };
        let curr_type = token.token().token_type();

        if matches!(curr_type, TokenType::Whitespace { .. }) {
            return TokenReference::new(vec![], token.token().clone(), vec![]);
        }

        let leading = if let Some(prev) = &self.prev_token {
            if Self::needs_space(prev, curr_type) {
                vec![Token::new(TokenType::Whitespace {
                    characters: " ".into(),
                })]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        self.prev_token = Some(curr_type.clone());
        TokenReference::new(leading, token.token().clone(), vec![])
    }

    fn visit_function_args(&mut self, args: FunctionArgs) -> FunctionArgs {
        if let FunctionArgs::Parentheses { arguments, .. } = &args {
            if arguments.len() == 1 {
                match arguments.iter().next() {
                    Some(Expression::String(string)) => {
                        return FunctionArgs::String(string.clone());
                    }
                    Some(Expression::TableConstructor(table)) => {
                        return FunctionArgs::TableConstructor(table.clone());
                    }
                    _ => {}
                }
            }
        }
        args
    }
}

// Resolve bindings on the original tree before changing tokens or whitespace.
// Each hook relies on full_moon's automatic traversal exactly once.
struct BindingResolver<'a> {
    packer: &'a mut Packer,
    assignments: Vec<Vec<(String, String)>>,
    loop_bindings: HashMap<usize, Vec<TokenReference>>,
    method_bodies: HashSet<usize>,
    repeat_depths: Vec<usize>,
}

impl<'a> BindingResolver<'a> {
    fn new(packer: &'a mut Packer) -> Self {
        Self {
            packer,
            assignments: Vec::new(),
            loop_bindings: HashMap::new(),
            method_bodies: HashSet::new(),
            repeat_depths: Vec::new(),
        }
    }

    fn record(&mut self, token: &TokenReference, replacement: String) {
        self.packer
            .replacements
            .insert(token.token().start_position().bytes(), replacement);
    }

    fn declare(&mut self, token: &TokenReference) {
        let name = self.packer.declare(&token.token().to_string());
        self.record(token, name);
    }

    fn reference(&mut self, token: &TokenReference) {
        let name = self.packer.rename(&token.token().to_string());
        self.record(token, name);
    }

    fn bind_loop(&mut self, block: &Block, names: Vec<TokenReference>) {
        if let Some(position) = block.start_position() {
            self.loop_bindings.insert(position.bytes(), names);
        }
    }
}

impl full_moon::visitors::Visitor for BindingResolver<'_> {
    fn visit_block(&mut self, block: &Block) {
        self.packer.push_scope();
        if let Some(names) = block
            .start_position()
            .and_then(|position| self.loop_bindings.remove(&position.bytes()))
        {
            for name in names {
                self.declare(&name);
            }
        }
    }

    fn visit_block_end(&mut self, _: &Block) {
        // A repeat block's locals remain visible in its until condition.
        if self.repeat_depths.last().map(|depth| depth + 1) != Some(self.packer.scopes.len()) {
            self.packer.pop_scope();
        }
    }

    fn visit_local_assignment(&mut self, assignment: &LocalAssignment) {
        let mut bindings = Vec::new();
        for token in assignment.names() {
            let name = token.token().to_string();
            let replacement = self.packer.allocate_name();
            self.record(token, replacement.clone());
            bindings.push((name, replacement));
        }
        self.assignments.push(bindings);
    }

    fn visit_local_assignment_end(&mut self, _: &LocalAssignment) {
        // The new names are not visible while evaluating their initializers.
        let bindings = self.assignments.pop().expect("local assignment scope");
        self.packer
            .scopes
            .last_mut()
            .expect("block scope")
            .extend(bindings);
    }

    fn visit_local_function(&mut self, function: &LocalFunction) {
        self.declare(function.name());
    }

    fn visit_function_declaration(&mut self, function: &FunctionDeclaration) {
        if let Some(receiver) = function.name().names().iter().next() {
            self.reference(receiver);
        }
        if function.name().method_name().is_some() {
            if let Some(position) = function.body().start_position() {
                self.method_bodies.insert(position.bytes());
            }
        }
    }

    fn visit_function_body(&mut self, body: &FunctionBody) {
        self.packer.push_scope();
        if body
            .start_position()
            .is_some_and(|position| self.method_bodies.remove(&position.bytes()))
        {
            self.packer
                .scopes
                .last_mut()
                .unwrap()
                .insert("self".into(), "self".into());
        }
        for parameter in body.parameters() {
            if let Parameter::Name(token) = parameter {
                self.declare(token);
            }
        }
    }

    fn visit_function_body_end(&mut self, _: &FunctionBody) {
        self.packer.pop_scope();
    }

    fn visit_numeric_for(&mut self, loop_: &NumericFor) {
        self.bind_loop(loop_.block(), vec![loop_.index_variable().clone()]);
    }

    fn visit_generic_for(&mut self, loop_: &GenericFor) {
        self.bind_loop(loop_.block(), loop_.names().iter().cloned().collect());
    }

    fn visit_repeat(&mut self, _: &Repeat) {
        self.packer.push_scope();
        self.repeat_depths.push(self.packer.scopes.len());
    }

    fn visit_repeat_end(&mut self, _: &Repeat) {
        self.packer.pop_scope();
        self.packer.pop_scope();
        self.repeat_depths.pop();
    }

    fn visit_var(&mut self, var: &Var) {
        if let Var::Name(token) = var {
            self.reference(token);
        }
    }

    fn visit_prefix(&mut self, prefix: &Prefix) {
        if let Prefix::Name(token) = prefix {
            self.reference(token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn generated_names_are_unique_and_not_reserved() {
        let packer = Packer::new();
        let mut seen = HashSet::new();
        for index in 0..20_000 {
            let name = packer.to_letter(index);
            assert!(!RESERVED.contains(&name.as_str()), "{name}");
            assert!(seen.insert(name.clone()), "duplicate at {index}: {name}");
        }
    }

    #[test]
    fn bindings_resolve_in_innermost_scope() {
        let mut packer = Packer::new();
        let outer = packer.declare("value");
        packer.push_scope();
        let inner = packer.declare("value");
        assert_ne!(inner, outer);
        assert_eq!(packer.rename("value"), inner);
        packer.pop_scope();
        assert_eq!(packer.rename("value"), outer);
        assert_eq!(packer.rename("external"), "external");
    }
}
