use full_moon::ast::*;
use full_moon::tokenizer::{Token, TokenReference, TokenType};
use full_moon::visitors::VisitorMut;
use std::collections::HashMap;

pub struct Packer {
    counter: usize,
    scopes: Vec<HashMap<String, String>>,
    prev_token: Option<TokenType>,
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
        let new_name = self.to_letter(self.counter);
        self.counter += 1;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), new_name.clone());
        }
        new_name
    }

    fn to_letter(&self, mut n: usize) -> String {
        loop {
            let mut result = String::new();
            let mut num = n + 1;

            while num > 0 {
                num -= 1;
                result.push((b'a' + (num % 26) as u8) as char);
                num /= 26;
            }

            let name = result.chars().rev().collect::<String>();
            if !RESERVED.contains(&name.as_str()) {
                return name;
            }

            n += 1;
        }
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
}

impl Default for Packer {
    fn default() -> Self {
        Self::new()
    }
}

impl VisitorMut for Packer {
    fn visit_token_reference(&mut self, token: TokenReference) -> TokenReference {
        let curr_type = token.token().token_type();

        if matches!(curr_type, TokenType::Whitespace { .. }) {
            return TokenReference::new(vec![], token.token().clone(), vec![]);
        }

        let leading = if let Some(prev) = &self.prev_token {
            if matches!(curr_type, TokenType::StringLiteral { .. }) {
                vec![]
            } else if Self::is_alphanum(prev) && Self::is_alphanum(curr_type) {
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

    fn visit_var(&mut self, var: Var) -> Var {
        match var {
            Var::Name(t) => {
                let name = t.token().to_string();
                let new_name = self.rename(&name);
                Var::Name(self.ident(&new_name))
            }
            _ => var,
        }
    }

    fn visit_prefix(&mut self, prefix: Prefix) -> Prefix {
        match prefix {
            Prefix::Name(t) => {
                let name = t.token().to_string();
                let new_name = self.rename(&name);
                Prefix::Name(self.ident(&new_name))
            }
            _ => prefix,
        }
    }

    fn visit_local_assignment(&mut self, assignment: LocalAssignment) -> LocalAssignment {
        use full_moon::ast::punctuated::Pair;

        let exprs = assignment
            .expressions()
            .pairs()
            .map(|p| match p {
                Pair::Punctuated(e, sep) => {
                    Pair::Punctuated(self.visit_expression(e.clone()), sep.clone())
                }
                Pair::End(e) => Pair::End(self.visit_expression(e.clone())),
            })
            .collect();

        let names = assignment
            .names()
            .pairs()
            .map(|p| match p {
                Pair::Punctuated(n, sep) => {
                    let name = n.token().to_string();
                    let new_name = self.declare(&name);
                    Pair::Punctuated(self.ident(&new_name), sep.clone())
                }
                Pair::End(n) => {
                    let name = n.token().to_string();
                    let new_name = self.declare(&name);
                    Pair::End(self.ident(&new_name))
                }
            })
            .collect();

        assignment.with_names(names).with_expressions(exprs)
    }

    fn visit_function_declaration(&mut self, func: FunctionDeclaration) -> FunctionDeclaration {
        self.push_scope();
        let body = self.visit_function_body(func.body().clone());
        self.pop_scope();
        func.with_body(body)
    }

    fn visit_function_args(&mut self, args: FunctionArgs) -> FunctionArgs {
        use full_moon::ast::punctuated::Pair;

        match &args {
            FunctionArgs::Parentheses { arguments, .. } if arguments.len() == 1 => {
                // Single argument - check if it's a string or table
                if let Some(pair) = arguments.pairs().next() {
                    let expr = match pair {
                        Pair::End(e) => e,
                        Pair::Punctuated(e, _) => e,
                    };

                    match expr {
                        Expression::String(s) => {
                            return FunctionArgs::String(s.clone());
                        }
                        Expression::TableConstructor(t) => {
                            return FunctionArgs::TableConstructor(t.clone());
                        }
                        _ => {}
                    }
                }
                args
            }
            _ => args,
        }
    }

    fn visit_local_function(&mut self, func: LocalFunction) -> LocalFunction {
        let name = func.name().token().to_string();
        let new_name = self.declare(&name);

        self.push_scope();
        let body = self.visit_function_body(func.body().clone());
        self.pop_scope();

        func.with_name(self.ident(&new_name)).with_body(body)
    }

    fn visit_function_body(&mut self, body: FunctionBody) -> FunctionBody {
        use full_moon::ast::punctuated::Pair;

        let params = body
            .parameters()
            .pairs()
            .map(|p| match p {
                Pair::Punctuated(param, sep) => {
                    let new_param = match param {
                        Parameter::Name(t) => {
                            let name = t.token().to_string();
                            let new_name = self.declare(&name);
                            Parameter::Name(self.ident(&new_name))
                        }
                        _ => param.clone(),
                    };
                    Pair::Punctuated(new_param, sep.clone())
                }
                Pair::End(param) => {
                    let new_param = match param {
                        Parameter::Name(t) => {
                            let name = t.token().to_string();
                            let new_name = self.declare(&name);
                            Parameter::Name(self.ident(&new_name))
                        }
                        _ => param.clone(),
                    };
                    Pair::End(new_param)
                }
            })
            .collect();

        let block = self.visit_block(body.block().clone());

        body.with_parameters(params).with_block(block)
    }

    fn visit_numeric_for(&mut self, num_for: NumericFor) -> NumericFor {
        let start = self.visit_expression(num_for.start().clone());
        let end = self.visit_expression(num_for.end().clone());
        let step = num_for
            .step()
            .map(|expr| self.visit_expression(expr.clone()));

        self.push_scope();

        let index_name = num_for.index_variable().token().to_string();
        let new_index = self.declare(&index_name);

        let block = self.visit_block(num_for.block().clone());

        self.pop_scope();

        num_for
            .with_index_variable(self.ident(&new_index))
            .with_start(start)
            .with_end(end)
            .with_step(step)
            .with_block(block)
    }

    fn visit_generic_for(&mut self, gen_for: GenericFor) -> GenericFor {
        use full_moon::ast::punctuated::Pair;

        let exprs = gen_for
            .expressions()
            .pairs()
            .map(|p| match p {
                Pair::Punctuated(e, sep) => {
                    Pair::Punctuated(self.visit_expression(e.clone()), sep.clone())
                }
                Pair::End(e) => Pair::End(self.visit_expression(e.clone())),
            })
            .collect();

        self.push_scope();

        let names = gen_for
            .names()
            .pairs()
            .map(|p| match p {
                Pair::Punctuated(n, sep) => {
                    let name = n.token().to_string();
                    let new_name = self.declare(&name);
                    Pair::Punctuated(self.ident(&new_name), sep.clone())
                }
                Pair::End(n) => {
                    let name = n.token().to_string();
                    let new_name = self.declare(&name);
                    Pair::End(self.ident(&new_name))
                }
            })
            .collect();

        let block = self.visit_block(gen_for.block().clone());

        self.pop_scope();

        gen_for
            .with_names(names)
            .with_expressions(exprs)
            .with_block(block)
    }

    fn visit_do(&mut self, do_block: Do) -> Do {
        self.push_scope();
        let block = self.visit_block(do_block.block().clone());
        self.pop_scope();
        do_block.with_block(block)
    }

    fn visit_if(&mut self, if_stmt: If) -> If {
        let condition = self.visit_expression(if_stmt.condition().clone());

        self.push_scope();
        let block = self.visit_block(if_stmt.block().clone());
        self.pop_scope();

        let else_if = if_stmt.else_if().map(|branches| {
            branches
                .iter()
                .map(|branch| {
                    let cond = self.visit_expression(branch.condition().clone());
                    self.push_scope();
                    let blk = self.visit_block(branch.block().clone());
                    self.pop_scope();
                    branch.clone().with_condition(cond).with_block(blk)
                })
                .collect()
        });

        let else_block = if_stmt.else_block().map(|eb| {
            self.push_scope();
            let blk = self.visit_block(eb.clone());
            self.pop_scope();
            blk
        });

        if_stmt
            .with_condition(condition)
            .with_block(block)
            .with_else_if(else_if)
            .with_else(else_block)
    }

    fn visit_repeat(&mut self, repeat_block: Repeat) -> Repeat {
        self.push_scope();
        let block = self.visit_block(repeat_block.block().clone());
        let until = self.visit_expression(repeat_block.until().clone());
        self.pop_scope();

        repeat_block.with_block(block).with_until(until)
    }

    fn visit_while(&mut self, while_block: While) -> While {
        let condition = self.visit_expression(while_block.condition().clone());

        self.push_scope();
        let block = self.visit_block(while_block.block().clone());
        self.pop_scope();

        while_block.with_condition(condition).with_block(block)
    }
}
