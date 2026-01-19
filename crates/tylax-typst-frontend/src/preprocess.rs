//! Safe, lightweight Typst preprocessor for IR conversion.
//!
//! This module expands a minimal subset of Typst:
//! - `#let` variables/functions (simple values + content blocks)
//! - `#if` with literal/bool/none comparisons
//! - `#for` loops over literal arrays
//! - basic counters (`counter("x").step()/display()/update()/reset()`)

use std::collections::HashMap;

use typst_syntax::{parse, SyntaxKind, SyntaxNode};
use tylax_ir::Loss;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Text(String),
    Bool(bool),
    Number(f64),
    None,
    Counter(String),
    Array(Vec<Value>),
}

impl Value {
    fn as_text(&self, counters: &HashMap<String, i64>) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => format_number(*n),
            Value::None => String::new(),
            Value::Counter(name) => counters.get(name).copied().unwrap_or(0).to_string(),
            Value::Array(_) => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct ParamDef {
    name: String,
    default: Option<Value>,
}

#[derive(Debug, Clone)]
struct FunctionDef {
    params: Vec<ParamDef>,
    body: String,
}

#[derive(Debug, Default, Clone)]
struct DefDb {
    vars: HashMap<String, Value>,
    funcs: HashMap<String, FunctionDef>,
}

impl DefDb {
    fn define_var(&mut self, name: &str, value: Value) {
        self.vars.insert(name.to_string(), value);
    }

    fn define_func(&mut self, name: &str, def: FunctionDef) {
        self.funcs.insert(name.to_string(), def);
    }

    fn get_var(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    fn get_func(&self, name: &str) -> Option<&FunctionDef> {
        self.funcs.get(name)
    }
}

#[derive(Debug, Default)]
pub struct PreprocessResult {
    pub source: String,
    pub losses: Vec<Loss>,
}

pub fn preprocess_typst(input: &str) -> PreprocessResult {
    if !input.contains('#') {
        return PreprocessResult {
            source: input.to_string(),
            losses: Vec::new(),
        };
    }

    let filtered = strip_imports(input);
    let root = parse(&filtered);
    let mut eval = Evaluator::new();
    let source = eval.expand_node(&root);
    PreprocessResult {
        source,
        losses: eval.losses,
    }
}

fn strip_imports(input: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    let mut depth: i32 = 0;
    for line in input.lines() {
        let trimmed = line.trim_start();
        if !skipping && (trimmed.starts_with("#import") || trimmed.starts_with("#include")) {
            depth = count_paren_delta(trimmed);
            if depth <= 0 {
                skipping = false;
                depth = 0;
            } else {
                skipping = true;
            }
            continue;
        }
        if skipping {
            depth += count_paren_delta(trimmed);
            if depth <= 0 {
                skipping = false;
                depth = 0;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn count_paren_delta(line: &str) -> i32 {
    let mut delta = 0;
    for ch in line.chars() {
        match ch {
            '(' => delta += 1,
            ')' => delta -= 1,
            _ => {}
        }
    }
    delta
}

struct Evaluator {
    db: DefDb,
    scopes: Vec<HashMap<String, Value>>,
    counters: HashMap<String, i64>,
    losses: Vec<Loss>,
    max_depth: usize,
    depth: usize,
}

impl Evaluator {
    fn new() -> Self {
        Self {
            db: DefDb::default(),
            scopes: Vec::new(),
            counters: HashMap::new(),
            losses: Vec::new(),
            max_depth: 32,
            depth: 0,
        }
    }

    fn expand_node(&mut self, node: &SyntaxNode) -> String {
        if self.depth > self.max_depth {
            return node_full_text(node);
        }

        match node.kind() {
            SyntaxKind::LetBinding
            | SyntaxKind::SetRule
            | SyntaxKind::ShowRule
            | SyntaxKind::Import
            | SyntaxKind::ModuleImport
            | SyntaxKind::Include
            | SyntaxKind::ModuleInclude => String::new(),
            SyntaxKind::ContentBlock => self.expand_content_block(node),
            SyntaxKind::Conditional => self.expand_conditional(node),
            SyntaxKind::ForLoop => self.expand_for_loop(node),
            _ => self.expand_children(node),
        }
    }

    fn expand_children(&mut self, node: &SyntaxNode) -> String {
        let children: Vec<_> = node.children().collect();
        if children.is_empty() {
            return node_full_text(node);
        }

        let mut out = String::new();
        let mut i = 0;
        while i < children.len() {
            let child = children[i];
            if child.kind() == SyntaxKind::Hash {
                if let Some(next) = children.get(i + 1) {
                    match next.kind() {
                        SyntaxKind::LetBinding => {
                            self.handle_let_binding(next);
                            i += 2;
                            continue;
                        }
                        SyntaxKind::Conditional => {
                            out.push_str(&self.expand_conditional(next));
                            i += 2;
                            continue;
                        }
                        SyntaxKind::ForLoop => {
                            out.push_str(&self.expand_for_loop(next));
                            i += 2;
                            continue;
                        }
                        SyntaxKind::FuncCall => {
                            if let Some(expanded) = self.expand_func_call(next) {
                                out.push_str(&expanded);
                            } else {
                                out.push('#');
                                out.push_str(&node_full_text(next));
                            }
                            i += 2;
                            continue;
                        }
                        SyntaxKind::Ident => {
                            let name = next.text().to_string();
                            if let Some(value) = self.lookup_value(&name) {
                                out.push_str(&value.as_text(&self.counters));
                            } else {
                                out.push('#');
                                out.push_str(&name);
                            }
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            out.push_str(&self.expand_node(child));
            i += 1;
        }

        out
    }

    fn expand_content_block(&mut self, node: &SyntaxNode) -> String {
        let mut out = String::new();
        for child in node.children() {
            if child.kind() == SyntaxKind::Markup {
                out.push_str(&self.expand_node(&child));
            }
        }
        out
    }

    fn expand_code_block(&mut self, node: &SyntaxNode) -> String {
        let mut out = String::new();
        for child in node.children() {
            match child.kind() {
                SyntaxKind::LeftBrace | SyntaxKind::RightBrace => {}
                _ => out.push_str(&self.expand_node(&child)),
            }
        }
        out
    }

    fn expand_block_body(&mut self, node: &SyntaxNode) -> String {
        match node.kind() {
            SyntaxKind::ContentBlock => self.expand_content_block(node),
            SyntaxKind::CodeBlock => self.expand_code_block(node),
            _ => self.expand_node(node),
        }
    }

    fn expand_conditional(&mut self, node: &SyntaxNode) -> String {
        let condition = self.eval_condition(node);
        match condition {
            Some(true) => self.content_block_at(node, 0),
            Some(false) => self.content_block_at(node, 1),
            None => {
                // Fallback: prefer the first branch to avoid dropping content.
                let first = self.content_block_at(node, 0);
                if !first.is_empty() {
                    first
                } else {
                    self.content_block_at(node, 1)
                }
            }
        }
    }

    fn expand_for_loop(&mut self, node: &SyntaxNode) -> String {
        let (var_names, items, body) = match self.parse_for_loop(node) {
            Some(v) => v,
            None => {
                // Fallback: preserve the body once if we can't evaluate items.
                if let Some(body) = node
                    .children()
                    .find(|c| matches!(c.kind(), SyntaxKind::ContentBlock | SyntaxKind::CodeBlock))
                {
                    return self.expand_block_body(&body);
                }
                self.losses.push(Loss::new(
                    "preprocess-for",
                    "Unsupported for-loop; dropping content",
                ));
                return String::new();
            }
        };

        let mut out = String::new();
        for item in items {
            let mut scope = HashMap::new();
            if var_names.len() == 1 {
                scope.insert(var_names[0].clone(), item);
            } else if let Value::Array(values) = item {
                for (idx, name) in var_names.iter().enumerate() {
                    let value = values.get(idx).cloned().unwrap_or(Value::None);
                    scope.insert(name.clone(), value);
                }
            } else {
                scope.insert(var_names[0].clone(), item);
                for name in var_names.iter().skip(1) {
                    scope.insert(name.clone(), Value::None);
                }
            }
            self.scopes.push(scope);
            self.depth += 1;
            out.push_str(&self.expand_block_body(&body));
            self.depth = self.depth.saturating_sub(1);
            self.scopes.pop();
        }

        out
    }

    fn handle_let_binding(&mut self, node: &SyntaxNode) {
        if let Some(closure) = node.children().find(|c| c.kind() == SyntaxKind::Closure) {
            if let Some(def) = self.parse_closure_def(node, &closure) {
                self.db.define_func(&def.0, def.1);
                return;
            }
        }
        if let Some(def) = self.parse_named_function_def(node) {
            self.db.define_func(&def.0, def.1);
            return;
        }

        if let Some((name, value_node)) = self.parse_let_value(node) {
            if let Some(value) = self.eval_value(&value_node) {
                self.db.define_var(&name, value);
            } else {
                let raw = node_full_text(&value_node);
                self.db.define_var(&name, Value::Text(raw));
            }
            return;
        }

        if let Some((names, value_node)) = self.parse_destructuring_let(node) {
            if let Some(value) = self.eval_value(&value_node) {
                match value {
                    Value::Array(values) => {
                        for (idx, name) in names.iter().enumerate() {
                            let v = values.get(idx).cloned().unwrap_or(Value::None);
                            self.db.define_var(name, v);
                        }
                    }
                    other => {
                        if let Some(first) = names.first() {
                            self.db.define_var(first, other);
                        }
                        for name in names.iter().skip(1) {
                            self.db.define_var(name, Value::None);
                        }
                    }
                }
            } else {
                let raw = node_full_text(&value_node);
                for name in names {
                    self.db.define_var(&name, Value::Text(raw.clone()));
                }
            }
            return;
        }

        if let Some((name, raw_value)) = self.parse_let_value_fallback(node) {
            self.db.define_var(&name, Value::Text(raw_value));
            return;
        }

        self.losses.push(Loss::new(
            "preprocess-let",
            "Unsupported variable definition; skipping",
        ));
    }

    fn expand_func_call(&mut self, node: &SyntaxNode) -> Option<String> {
        if let Some(field_access) = node.children().find(|c| c.kind() == SyntaxKind::FieldAccess) {
            if let Some(result) = self.handle_counter_method(field_access, node) {
                return Some(result);
            }
        }

        let name = self.get_func_name(node)?;
        if name == "counter" {
            // Counter objects don't render by themselves.
            return Some(String::new());
        }

        let def = self.db.get_func(&name)?.clone();
        let (positional, named, body_arg) = self.parse_call_args(node);
        let bindings = self.bind_params(&def, positional, named, body_arg);
        let expanded = self.expand_function_body(&def, &bindings);
        Some(expanded)
    }

    fn handle_counter_method(
        &mut self,
        field_access: &SyntaxNode,
        call_node: &SyntaxNode,
    ) -> Option<String> {
        let method = self.field_access_method(field_access)?;
        let base = self.eval_field_access_base(field_access)?;
        let Value::Counter(name) = base else {
            return None;
        };

        match method.as_str() {
            "step" => {
                let next = self.counters.get(&name).copied().unwrap_or(0) + 1;
                self.counters.insert(name, next);
                Some(String::new())
            }
            "display" => {
                let value = self.counters.get(&name).copied().unwrap_or(0);
                Some(value.to_string())
            }
            "update" => {
                let args = self.parse_args_as_values(call_node);
                if let Some(Value::Text(v)) = args.first() {
                    if let Ok(n) = v.trim().parse::<i64>() {
                        self.counters.insert(name, n);
                    }
                }
                Some(String::new())
            }
            "reset" => {
                self.counters.insert(name, 0);
                Some(String::new())
            }
            _ => None,
        }
    }

    fn parse_for_loop(
        &mut self,
        node: &SyntaxNode,
    ) -> Option<(Vec<String>, Vec<Value>, SyntaxNode)> {
        let mut var_names: Vec<String> = Vec::new();
        let mut items: Option<Vec<Value>> = None;
        let mut body: Option<SyntaxNode> = None;
        let mut saw_in = false;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::Ident if !saw_in && var_names.is_empty() => {
                    var_names.push(child.text().to_string());
                }
                SyntaxKind::Destructuring if !saw_in && var_names.is_empty() => {
                    let names = extract_destructuring_idents(&child);
                    if !names.is_empty() {
                        var_names = names;
                    }
                }
                SyntaxKind::In => {
                    saw_in = true;
                }
                SyntaxKind::Array if saw_in && items.is_none() => {
                    let array = self.parse_array_values(&child)?;
                    items = Some(array);
                }
                SyntaxKind::Ident if saw_in && items.is_none() => {
                    if let Some(Value::Array(values)) = self.lookup_value(child.text().as_ref()) {
                        items = Some(values);
                    }
                }
                SyntaxKind::FuncCall if saw_in && items.is_none() => {
                    if let Some(Value::Array(values)) = self.eval_value(&child) {
                        items = Some(values);
                    }
                }
                SyntaxKind::ContentBlock | SyntaxKind::CodeBlock => {
                    body = Some(child.clone());
                }
                _ => {}
            }
        }

        if var_names.is_empty() {
            return None;
        }

        Some((var_names, items?, body?))
    }

    fn eval_condition(&mut self, node: &SyntaxNode) -> Option<bool> {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::If | SyntaxKind::Else | SyntaxKind::Space | SyntaxKind::ContentBlock => {
                    continue;
                }
                _ => return self.eval_bool_expr(&child),
            }
        }
        None
    }

    fn eval_bool_expr(&mut self, node: &SyntaxNode) -> Option<bool> {
        match node.kind() {
            SyntaxKind::Bool => Some(node.text().trim() == "true"),
            SyntaxKind::Unary => self.eval_unary(node),
            SyntaxKind::Binary => self.eval_binary(node.clone()),
            SyntaxKind::Parenthesized => {
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::LeftParen | SyntaxKind::RightParen | SyntaxKind::Space => {}
                        _ => return self.eval_bool_expr(&child),
                    }
                }
                None
            }
            SyntaxKind::FuncCall => {
                let value = self.eval_value(node)?;
                self.value_truthy(&value)
            }
            SyntaxKind::Ident => match self.lookup_value(node.text().as_ref()) {
                Some(Value::Bool(b)) => Some(b),
                Some(Value::None) => Some(false),
                Some(Value::Number(n)) => Some(n != 0.0),
                Some(Value::Text(s)) => Some(!s.trim().is_empty()),
                Some(Value::Counter(name)) => {
                    let value = self.counters.get(name.as_str()).copied().unwrap_or(0);
                    Some(value != 0)
                }
                Some(Value::Array(values)) => Some(!values.is_empty()),
                None => None,
            },
            _ => None,
        }
    }

    fn value_truthy(&self, value: &Value) -> Option<bool> {
        match value {
            Value::Bool(b) => Some(*b),
            Value::None => Some(false),
            Value::Number(n) => Some(*n != 0.0),
            Value::Text(s) => {
                let trimmed = s.trim();
                if trimmed.eq_ignore_ascii_case("true") {
                    Some(true)
                } else if trimmed.eq_ignore_ascii_case("false") {
                    Some(false)
                } else {
                    Some(!trimmed.is_empty())
                }
            }
            Value::Counter(name) => {
                let value = self.counters.get(name.as_str()).copied().unwrap_or(0);
                Some(value != 0)
            }
            Value::Array(values) => Some(!values.is_empty()),
        }
    }

    fn eval_binary(&mut self, node: SyntaxNode) -> Option<bool> {
        let (left_node, op, right_node) = self.parse_binary_parts(&node)?;
        match op {
            SyntaxKind::And => {
                let left = self.eval_bool_expr(&left_node)?;
                let right = self.eval_bool_expr(&right_node)?;
                Some(left && right)
            }
            SyntaxKind::Or => {
                let left = self.eval_bool_expr(&left_node)?;
                let right = self.eval_bool_expr(&right_node)?;
                Some(left || right)
            }
            SyntaxKind::EqEq | SyntaxKind::ExclEq | SyntaxKind::Lt | SyntaxKind::LtEq
            | SyntaxKind::Gt | SyntaxKind::GtEq => {
                let left = self.eval_value(&left_node)?;
                let right = self.eval_value(&right_node)?;
                let ordering = self.compare_values(&left, &right)?;
                match op {
                    SyntaxKind::EqEq => Some(ordering == 0),
                    SyntaxKind::ExclEq => Some(ordering != 0),
                    SyntaxKind::Lt => Some(ordering < 0),
                    SyntaxKind::LtEq => Some(ordering <= 0),
                    SyntaxKind::Gt => Some(ordering > 0),
                    SyntaxKind::GtEq => Some(ordering >= 0),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn compare_values(&self, left: &Value, right: &Value) -> Option<i8> {
        match (left, right) {
            (Value::None, Value::None) => Some(0),
            (Value::Bool(a), Value::Bool(b)) => {
                if a == b {
                    Some(0)
                } else if !*a && *b {
                    Some(-1)
                } else {
                    Some(1)
                }
            }
            (Value::Text(a), Value::Text(b)) => Some(ordering_to_i8(a.cmp(b))),
            (Value::Number(a), Value::Number(b)) => Some(compare_numbers(*a, *b)),
            (Value::Number(a), Value::Text(b)) => {
                let b = b.parse::<f64>().ok()?;
                Some(compare_numbers(*a, b))
            }
            (Value::Text(a), Value::Number(b)) => {
                let a = a.parse::<f64>().ok()?;
                Some(compare_numbers(a, *b))
            }
            _ => None,
        }
    }

    fn parse_let_value(&self, node: &SyntaxNode) -> Option<(String, SyntaxNode)> {
        let mut name: Option<String> = None;
        let mut value: Option<SyntaxNode> = None;
        let mut seen_eq = false;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Ident if name.is_none() => name = Some(child.text().to_string()),
                SyntaxKind::Math | SyntaxKind::Equation if name.is_none() && !seen_eq => {
                    name = Some(child.text().to_string());
                }
                SyntaxKind::Eq => seen_eq = true,
                SyntaxKind::Space => {}
                _ => {
                    if !seen_eq
                        && name.is_none()
                        && !matches!(
                            child.kind(),
                            SyntaxKind::Params
                                | SyntaxKind::Closure
                                | SyntaxKind::Destructuring
                                | SyntaxKind::Let
                        )
                    {
                        name = Some(child.text().to_string());
                        continue;
                    }
                    if seen_eq {
                        value = Some(child.clone());
                        break;
                    }
                }
            }
        }
        Some((name?, value?))
    }

    fn parse_destructuring_let(&self, node: &SyntaxNode) -> Option<(Vec<String>, SyntaxNode)> {
        let mut names: Option<Vec<String>> = None;
        let mut value: Option<SyntaxNode> = None;
        let mut seen_eq = false;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Destructuring if names.is_none() => {
                    let extracted = extract_destructuring_idents(&child);
                    if !extracted.is_empty() {
                        names = Some(extracted);
                    }
                }
                SyntaxKind::Eq => seen_eq = true,
                SyntaxKind::Space => {}
                _ => {
                    if seen_eq {
                        value = Some(child.clone());
                        break;
                    }
                }
            }
        }
        Some((names?, value?))
    }

    fn parse_closure_def(
        &mut self,
        let_node: &SyntaxNode,
        closure: &SyntaxNode,
    ) -> Option<(String, FunctionDef)> {
        let name = closure
            .children()
            .find(|c| c.kind() == SyntaxKind::Ident)
            .or_else(|| let_node.children().find(|c| c.kind() == SyntaxKind::Ident))?
            .text()
            .to_string();
        let params_node = closure.children().find(|c| c.kind() == SyntaxKind::Params);
        let params = params_node
            .map(|p| self.parse_params(&p))
            .unwrap_or_default();
        let body_node = self.find_closure_body(closure)?;
        let body = match body_node.kind() {
            SyntaxKind::ContentBlock => self.expand_content_block(&body_node),
            _ => node_full_text(&body_node),
        };
        Some((name, FunctionDef { params, body }))
    }

    fn find_closure_body(&self, node: &SyntaxNode) -> Option<SyntaxNode> {
        if let Some(body_node) = node.children().find(|c| c.kind() == SyntaxKind::ContentBlock) {
            return Some(body_node.clone());
        }
        if let Some(code_node) = node.children().find(|c| c.kind() == SyntaxKind::CodeBlock) {
            return Some(code_node.clone());
        }
        if let Some(body_node) = node.children().find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::Raw
                    | SyntaxKind::Math
                    | SyntaxKind::Equation
                    | SyntaxKind::FuncCall
                    | SyntaxKind::Str
                    | SyntaxKind::Ident
                    | SyntaxKind::Parenthesized
                    | SyntaxKind::Binary
                    | SyntaxKind::Unary
            )
        }) {
            return Some(body_node.clone());
        }
        node.children()
            .find(|c| {
            !matches!(
                c.kind(),
                SyntaxKind::Params | SyntaxKind::Arrow | SyntaxKind::Space
            )
        })
            .cloned()
    }

    fn parse_named_function_def(&mut self, node: &SyntaxNode) -> Option<(String, FunctionDef)> {
        let name = node
            .children()
            .find(|c| c.kind() == SyntaxKind::Ident)?
            .text()
            .to_string();
        let params_node = node.children().find(|c| c.kind() == SyntaxKind::Params)?;
        let params = self.parse_params(&params_node);
        let mut seen_eq = false;
        let mut body_node: Option<SyntaxNode> = None;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Eq => seen_eq = true,
                SyntaxKind::Space => {}
                _ => {
                    if seen_eq {
                        body_node = Some(child.clone());
                        break;
                    }
                }
            }
        }
        let body_node = body_node?;
        let body = node_full_text(&body_node);
        Some((name, FunctionDef { params, body }))
    }

    fn parse_let_value_fallback(&self, node: &SyntaxNode) -> Option<(String, String)> {
        let text = node_full_text(node);
        let text = text.trim_start();
        let text = text.strip_prefix("let")?.trim_start();
        let mut parts = text.splitn(2, '=');
        let name = parts.next()?.trim();
        let value = parts.next()?.trim();
        if name.is_empty() || value.is_empty() {
            return None;
        }
        Some((name.to_string(), value.to_string()))
    }

    fn parse_params(&mut self, node: &SyntaxNode) -> Vec<ParamDef> {
        let mut params = Vec::new();
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Ident => {
                    params.push(ParamDef {
                        name: child.text().to_string(),
                        default: None,
                    });
                }
                SyntaxKind::Named => {
                    let mut name: Option<String> = None;
                    let mut value: Option<Value> = None;
                    for part in child.children() {
                        match part.kind() {
                            SyntaxKind::Ident if name.is_none() => {
                                name = Some(part.text().to_string())
                            }
                            SyntaxKind::None
                            | SyntaxKind::Bool
                            | SyntaxKind::Int
                            | SyntaxKind::Float
                            | SyntaxKind::Str
                            | SyntaxKind::Ident => value = self.eval_value(&part),
                            _ => {}
                        }
                    }
                    if let Some(name) = name {
                        params.push(ParamDef { name, default: value });
                    }
                }
                _ => {}
            }
        }
        params
    }

    fn extract_codeblock_body(&self, node: &SyntaxNode, params: &[ParamDef]) -> String {
        let mut out: Vec<String> = Vec::new();
        let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();

        fn walk(node: &SyntaxNode, params: &[&str], out: &mut Vec<String>) {
            for child in node.children() {
                if child.kind() == SyntaxKind::Ident {
                    let name = child.text().to_string();
                    if params.iter().any(|p| *p == name) {
                        out.push(format!("#{}", name));
                    }
                }
                walk(&child, params, out);
            }
        }

        walk(node, &param_names, &mut out);

        if out.is_empty() {
            String::new()
        } else {
            out.join(" ")
        }
    }

    fn parse_call_args(
        &mut self,
        node: &SyntaxNode,
    ) -> (Vec<Value>, HashMap<String, Value>, Option<Value>) {
        let mut positional = Vec::new();
        let mut named = HashMap::new();
        let mut body_arg: Option<Value> = None;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::Args => {
                    for arg in child.children() {
                        match arg.kind() {
                            SyntaxKind::LeftParen
                            | SyntaxKind::RightParen
                            | SyntaxKind::Comma
                            | SyntaxKind::Space => {}
                            SyntaxKind::Named => {
                                if let Some((name, value)) = self.parse_named_arg(&arg) {
                                    named.insert(name, value);
                                }
                            }
                            _ => {
                                if let Some(value) = self.eval_value(&arg) {
                                    positional.push(value);
                                } else {
                                    positional.push(Value::Text(arg.text().to_string()));
                                }
                            }
                        }
                    }
                }
                SyntaxKind::ContentBlock => {
                    let content = self.expand_content_block(&child);
                    body_arg = Some(Value::Text(content));
                }
                _ => {}
            }
        }

        (positional, named, body_arg)
    }

    fn parse_named_arg(&mut self, node: &SyntaxNode) -> Option<(String, Value)> {
        let mut name: Option<String> = None;
        let mut value: Option<Value> = None;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Ident if name.is_none() => name = Some(child.text().to_string()),
                SyntaxKind::None
                | SyntaxKind::Bool
                | SyntaxKind::Int
                | SyntaxKind::Float
                | SyntaxKind::Str
                | SyntaxKind::ContentBlock => value = self.eval_value(&child),
                SyntaxKind::Ident => value = self.eval_value(&child),
                _ => {}
            }
        }
        Some((name?, value?))
    }

    fn bind_params(
        &mut self,
        def: &FunctionDef,
        positional: Vec<Value>,
        mut named: HashMap<String, Value>,
        body_arg: Option<Value>,
    ) -> HashMap<String, Value> {
        let mut bindings = HashMap::new();
        let mut positional_iter = positional.into_iter();

        if let Some(body) = body_arg {
            if def.params.iter().any(|p| p.name == "body") {
                bindings.insert("body".to_string(), body);
            }
        }

        for param in &def.params {
            if bindings.contains_key(&param.name) {
                continue;
            }
            if let Some(value) = named.remove(&param.name) {
                bindings.insert(param.name.clone(), value);
                continue;
            }
            if let Some(value) = positional_iter.next() {
                bindings.insert(param.name.clone(), value);
                continue;
            }
            if let Some(default) = &param.default {
                bindings.insert(param.name.clone(), default.clone());
            } else {
                bindings.insert(param.name.clone(), Value::None);
            }
        }

        bindings
    }

    fn expand_function_body(&mut self, def: &FunctionDef, bindings: &HashMap<String, Value>) -> String {
        let mut scope = HashMap::new();
        for (k, v) in bindings {
            scope.insert(k.clone(), v.clone());
        }
        self.scopes.push(scope);
        self.depth += 1;
        let result = self.expand(&def.body);
        self.depth = self.depth.saturating_sub(1);
        self.scopes.pop();
        result
    }

    fn expand(&mut self, source: &str) -> String {
        let root = parse(source);
        self.expand_node(&root)
    }

    fn lookup_value(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        self.db.get_var(name).cloned()
    }

    fn get_func_name(&self, node: &SyntaxNode) -> Option<String> {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Ident => return Some(child.text().to_string()),
                SyntaxKind::FieldAccess => {
                    return self.field_access_method(&child);
                }
                _ => {}
            }
        }
        None
    }

    fn field_access_method(&self, node: &SyntaxNode) -> Option<String> {
        let mut idents = Vec::new();
        for child in node.children() {
            if child.kind() == SyntaxKind::Ident {
                idents.push(child.text().to_string());
            }
        }
        idents.last().cloned()
    }

    fn eval_field_access_base(&mut self, node: &SyntaxNode) -> Option<Value> {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::FuncCall => return self.eval_func_call_value(&child),
                SyntaxKind::FieldAccess => {
                    if let Some(value) = self.eval_field_access_base(&child) {
                        return Some(value);
                    }
                }
                SyntaxKind::Ident => {
                    return self.lookup_value(child.text().as_ref());
                }
                _ => {}
            }
        }
        None
    }

    fn eval_func_call_value(&mut self, node: &SyntaxNode) -> Option<Value> {
        let name = self.get_func_name(node)?;
        if name == "range" {
            return self.eval_range(node);
        }
        if name == "counter" {
            let args = self.parse_args_as_values(node);
            let first = args.first()?;
            let key = match first {
                Value::Text(s) => s.trim_matches('"').to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => format_number(*n),
                Value::None => "none".to_string(),
                Value::Counter(s) => s.clone(),
                Value::Array(_) => return None,
            };
            return Some(Value::Counter(key));
        }

        let def = self.db.get_func(&name)?.clone();
        let (positional, named, body_arg) = self.parse_call_args(node);
        let bindings = self.bind_params(&def, positional, named, body_arg);
        Some(Value::Text(self.expand_function_body(&def, &bindings)))
    }

    fn parse_args_as_values(&mut self, node: &SyntaxNode) -> Vec<Value> {
        let mut args = Vec::new();
        for child in node.children() {
            if child.kind() == SyntaxKind::Args {
                for arg in child.children() {
                    match arg.kind() {
                        SyntaxKind::LeftParen
                        | SyntaxKind::RightParen
                        | SyntaxKind::Comma
                        | SyntaxKind::Space => {}
                        SyntaxKind::Named => {}
                        _ => {
                            if let Some(value) = self.eval_value(&arg) {
                                args.push(value);
                            } else {
                                args.push(Value::Text(arg.text().to_string()));
                            }
                        }
                    }
                }
            }
        }
        args
    }

    fn parse_array_values(&mut self, node: &SyntaxNode) -> Option<Vec<Value>> {
        let mut values = Vec::new();
        for child in node.children() {
            match child.kind() {
                SyntaxKind::LeftParen
                | SyntaxKind::RightParen
                | SyntaxKind::Comma
                | SyntaxKind::Space => {}
                _ => {
                    let value = self.eval_value(&child)?;
                    values.push(value);
                }
            }
        }
        Some(values)
    }

    fn eval_value(&mut self, node: &SyntaxNode) -> Option<Value> {
        match node.kind() {
            SyntaxKind::Bool => Some(Value::Bool(node.text().trim() == "true")),
            SyntaxKind::Int | SyntaxKind::Float => {
                let text = node.text().to_string();
                let number = text.parse::<f64>().ok()?;
                Some(Value::Number(number))
            }
            SyntaxKind::Str => Some(Value::Text(
                node.text().trim_matches('"').to_string(),
            )),
            SyntaxKind::None => Some(Value::None),
            SyntaxKind::Ident => self.lookup_value(node.text().as_ref()),
            SyntaxKind::ContentBlock => Some(Value::Text(self.expand_content_block(node))),
            SyntaxKind::CodeBlock
            | SyntaxKind::Code
            | SyntaxKind::Raw
            | SyntaxKind::Math
            | SyntaxKind::Equation => Some(Value::Text(node_full_text(node))),
            SyntaxKind::Array => Some(Value::Array(self.parse_array_values(node)?)),
            SyntaxKind::FuncCall => self.eval_func_call_value(node),
            SyntaxKind::Binary => {
                let value = self.eval_binary(node.clone())?;
                Some(Value::Bool(value))
            }
            SyntaxKind::Unary => {
                let value = self.eval_unary(node)?;
                Some(Value::Bool(value))
            }
            _ => None,
        }
    }

    fn content_block_at(&mut self, node: &SyntaxNode, index: usize) -> String {
        let blocks: Vec<_> = node
            .children()
            .filter(|c| matches!(c.kind(), SyntaxKind::ContentBlock | SyntaxKind::CodeBlock))
            .collect();
        if let Some(block) = blocks.get(index) {
            self.expand_block_body(block)
        } else {
            String::new()
        }
    }

    fn eval_unary(&mut self, node: &SyntaxNode) -> Option<bool> {
        let mut found_not = false;
        let mut operand: Option<SyntaxNode> = None;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::Not => found_not = true,
                SyntaxKind::Space => {}
                _ => {
                    operand = Some(child.clone());
                }
            }
        }
        let value = self.eval_bool_expr(&operand?)?;
        if found_not {
            Some(!value)
        } else {
            Some(value)
        }
    }

    fn parse_binary_parts(
        &self,
        node: &SyntaxNode,
    ) -> Option<(SyntaxNode, SyntaxKind, SyntaxNode)> {
        let mut left: Option<SyntaxNode> = None;
        let mut right: Option<SyntaxNode> = None;
        let mut op: Option<SyntaxKind> = None;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::Space => {}
                SyntaxKind::EqEq
                | SyntaxKind::ExclEq
                | SyntaxKind::Lt
                | SyntaxKind::LtEq
                | SyntaxKind::Gt
                | SyntaxKind::GtEq
                | SyntaxKind::And
                | SyntaxKind::Or => op = Some(child.kind()),
                _ => {
                    if left.is_none() {
                        left = Some(child.clone());
                    } else if right.is_none() {
                        right = Some(child.clone());
                    }
                }
            }
        }

        Some((left?, op?, right?))
    }

    fn eval_range(&mut self, node: &SyntaxNode) -> Option<Value> {
        let (positional, named, _) = self.parse_call_args(node);
        let mut start: Option<i64> = None;
        let mut end: Option<i64> = None;
        let mut step: i64 = 1;

        if let Some(value) = named.get("start") {
            start = value_to_i64(value);
        }
        if let Some(value) = named.get("end") {
            end = value_to_i64(value);
        }
        if let Some(value) = named.get("step") {
            if let Some(v) = value_to_i64(value) {
                step = v;
            }
        }

        let mut pos_iter = positional.into_iter();
        if start.is_none() {
            if let Some(value) = pos_iter.next() {
                start = value_to_i64(&value);
            }
        }
        if end.is_none() {
            if let Some(value) = pos_iter.next() {
                end = value_to_i64(&value);
            }
        }
        if step == 1 {
            if let Some(value) = pos_iter.next() {
                if let Some(v) = value_to_i64(&value) {
                    step = v;
                }
            }
        }

        if start.is_none() && end.is_some() {
            start = Some(0);
        }

        let Some(mut start) = start else {
            self.losses.push(Loss::new(
                "preprocess-range",
                "range() missing numeric start",
            ));
            return None;
        };
        let end = match end {
            Some(val) => val,
            None => {
                // Support range(end) => start at 0
                let end = start;
                start = 0;
                end
            }
        };
        if step == 0 {
            self.losses.push(Loss::new(
                "preprocess-range",
                "range() step cannot be 0",
            ));
            return None;
        }

        let mut values = Vec::new();
        if step > 0 {
            while start < end {
                values.push(Value::Number(start as f64));
                start += step;
            }
        } else {
            while start > end {
                values.push(Value::Number(start as f64));
                start += step;
            }
        }
        Some(Value::Array(values))
    }
}

fn node_full_text(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    if !text.is_empty() {
        return text;
    }
    node.clone().into_text().to_string()
}

fn extract_destructuring_idents(node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    let mut stack = vec![node.clone()];
    while let Some(current) = stack.pop() {
        for child in current.children() {
            match child.kind() {
                SyntaxKind::Ident => {
                    let name = child.text().to_string();
                    if name != "_" {
                        names.push(name);
                    }
                }
                SyntaxKind::Destructuring => stack.push(child.clone()),
                _ => {}
            }
        }
    }
    names
}

fn format_number(n: f64) -> String {
    if (n.fract()).abs() < 1e-9 {
        format!("{}", n.round() as i64)
    } else {
        let mut s = n.to_string();
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    }
}

fn compare_numbers(a: f64, b: f64) -> i8 {
    if (a - b).abs() < 1e-9 {
        0
    } else if a < b {
        -1
    } else {
        1
    }
}

fn ordering_to_i8(ordering: std::cmp::Ordering) -> i8 {
    match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => Some(*n as i64),
        Value::Text(s) => s.trim().parse::<i64>().ok(),
        Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::preprocess_typst;

    fn norm(s: &str) -> String {
        s.trim().replace("\r\n", "\n")
    }

    #[test]
    fn expands_let_and_function() {
        let input = "#let foo = [Hello]\n#let bar(x) = [Hi #x]\n\n#foo\n\n#bar(1)\n";
        let result = preprocess_typst(input);
        assert_eq!(norm(&result.source), "Hello\n\nHi 1");
    }

    #[test]
    fn expands_if_and_for() {
        let input = "#if true [A] else [B]\n#for x in (1,2) [#x ]\n";
        let result = preprocess_typst(input);
        assert_eq!(norm(&result.source), "A\n1 2");
    }

    #[test]
    fn expands_counter_methods() {
        let input = "#let c = counter(\"t\")\n#c.step()\n#c.display()";
        let result = preprocess_typst(input);
        assert_eq!(norm(&result.source), "1");
    }

    #[test]
    fn expands_logic_and_range() {
        let input =
            "#if true and not false [yes] #if \"a\" == \"a\" [eq]\n#let nums = range(1,4)\n#for x in nums [#x ]";
        let result = preprocess_typst(input);
        assert_eq!(norm(&result.source), "yes eq\n1 2 3");
    }
}
