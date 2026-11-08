use crate::graph::{Edge, Function, Graph, Unresolved};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};
use tree_sitter::{Node, Parser};

#[derive(Clone, Serialize, Deserialize)]
struct Import {
    local: String,
    original: String,
    source: String,
    namespace: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Call {
    owner: String,
    expression: String,
    line: usize,
    direct: bool,
    receiver: Option<String>,
    name: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Unit {
    file: String,
    digest: String,
    nodes: Vec<Function>,
    calls: Vec<Call>,
    imports: Vec<Import>,
    diagnostics: Vec<String>,
    /// Quoted local `#include` paths of C/C++ units, normalized to forward slashes
    /// and relative to the including file. System `<...>` includes are ignored.
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    package: String,
    #[serde(default)]
    struct_fields: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    local_types: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

fn text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}
fn field_text(node: Node, field: &str, source: &str) -> String {
    node.child_by_field_name(field)
        .map(|n| text(n, source).to_string())
        .unwrap_or_default()
}
fn children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "function_definition"
            | "lambda"
            | "lambda_expression"
            | "method_declaration"
            | "func_literal"
    )
}

/// True for the languages handled by the C/C++ resolution branch.
fn is_c_family(language: &str) -> bool {
    matches!(language, "C" | "C++")
}

/// Splits `app::Base::name` into (previous `::` segment, last `::` segment).
fn qualified_parts(value: &str) -> Option<(&str, &str)> {
    let segments = value
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }
    Some((segments[segments.len() - 2], segments[segments.len() - 1]))
}

/// Follows C/C++ declarator wrappers down to the node carrying the declared name.
fn declarator_leaf(mut node: Node) -> Node {
    loop {
        if !matches!(
            node.kind(),
            "function_declarator"
                | "pointer_declarator"
                | "reference_declarator"
                | "parenthesized_declarator"
                | "array_declarator"
                | "init_declarator"
        ) {
            return node;
        }
        match node.child_by_field_name("declarator") {
            Some(inner) => node = inner,
            None => return node,
        }
    }
}

/// Extracts `(name, class)` from a C/C++ declarator.
///
/// `Base::run` -> `("run", Some("Base"))`, `app::Base::create` ->
/// `("create", Some("Base"))`, `Base::~Base` -> `("~Base", Some("Base"))`,
/// `operator==` -> `("operator==", None)`, plain `run` -> `("run", None)`.
fn declarator_parts(node: Node, source: &str) -> Option<(String, Option<String>)> {
    let leaf = declarator_leaf(node);
    match leaf.kind() {
        "identifier" | "field_identifier" | "destructor_name" | "operator_name" => {
            let name = text(leaf, source).trim();
            (!name.is_empty()).then(|| (name.to_string(), None))
        }
        "qualified_identifier" => {
            let (scope, name) = qualified_parts(text(leaf, source))?;
            Some((name.to_string(), Some(scope.to_string())))
        }
        "template_function" => leaf
            .child_by_field_name("name")
            .map(|name| text(name, source).to_string())
            .filter(|name| !name.is_empty())
            .map(|name| (name, None)),
        _ => None,
    }
}

/// Finds the parameter list of a C/C++ function or lambda by walking the
/// declarator chain (`function_definition` does not expose `parameters` directly;
/// lambdas carry them on their `abstract_function_declarator`).
fn function_parameters(node: Node) -> Option<Node> {
    if let Some(parameters) = node
        .child_by_field_name("parameters")
        .or_else(|| node.child_by_field_name("parameter"))
    {
        return Some(parameters);
    }
    let mut current = node;
    while let Some(declarator) = current.child_by_field_name("declarator") {
        if matches!(
            declarator.kind(),
            "function_declarator" | "abstract_function_declarator"
        ) {
            if let Some(parameters) = declarator.child_by_field_name("parameters") {
                return Some(parameters);
            }
        }
        current = declarator;
    }
    None
}

/// Collects quoted local `#include "..."` paths; system `<...>` includes are
/// ignored because they do not point into the scanned tree.
fn collect_includes(node: Node, source: &str, output: &mut Vec<String>) {
    if node.kind() == "preproc_include" {
        if let Some(path) = node.child_by_field_name("path") {
            if path.kind() == "string_literal" {
                let raw = text(path, source);
                if let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                    let normalized = inner.replace('\\', "/");
                    if !normalized.is_empty() {
                        output.push(normalized);
                    }
                }
            }
        }
    }
    for child in children(node) {
        collect_includes(child, source, output);
    }
}

fn supported_extension(extension: &str) -> bool {
    matches!(
        extension,
        "ts" | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "svelte"
            | "go"
            | "c"
            | "h"
            | "cpp"
            | "cc"
            | "cxx"
            | "c++"
            | "hpp"
            | "hh"
            | "hxx"
            | "h++"
    )
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn function_name(node: Node, source: &str) -> Option<String> {
    if let Some(parent) = node
        .parent()
        .filter(|p| matches!(p.kind(), "variable_declarator" | "pair" | "assignment"))
    {
        if let Some(name) = parent
            .child_by_field_name("name")
            .or_else(|| parent.child_by_field_name("key"))
            .or_else(|| parent.child_by_field_name("left"))
            .filter(|n| matches!(n.kind(), "identifier" | "property_identifier"))
        {
            return Some(text(name, source).to_string());
        }
    }
    if let Some(name) = node.child_by_field_name("name") {
        return Some(text(name, source).to_string());
    }
    // C/C++ keep the declared name inside a declarator chain.
    node.child_by_field_name("declarator")
        .and_then(|declarator| declarator_parts(declarator, source))
        .map(|(name, _)| name)
}

fn class_name(node: Node, source: &str) -> Option<String> {
    if node.kind() == "method_declaration" {
        let receiver = node.child_by_field_name("receiver")?;
        let declaration = receiver.named_child(0)?;
        let ty = declaration.child_by_field_name("type")?;
        return Some(text(ty, source).trim_start_matches('*').to_string());
    }
    if node.kind() == "function_definition" {
        // Out-of-line C/C++ methods carry their class in a qualified declarator
        // (`void Base::run() {}` / `void app::Base::run() {}`).
        if let Some(class) = node
            .child_by_field_name("declarator")
            .and_then(|declarator| declarator_parts(declarator, source))
            .and_then(|(_, class)| class)
        {
            return Some(class);
        }
    }
    if !matches!(node.kind(), "method_definition" | "function_definition") {
        return None;
    }
    let mut parent = node.parent();
    while let Some(p) = parent {
        if is_function(p.kind()) {
            break;
        }
        if matches!(
            p.kind(),
            "class_declaration"
                | "class_definition"
                | "class"
                | "class_specifier"
                | "struct_specifier"
                | "union_specifier"
        ) {
            return p
                .child_by_field_name("name")
                .map(|n| text(n, source).into());
        }
        parent = p.parent();
    }
    None
}

fn identifiers(node: Node, source: &str, names: &mut Vec<String>) {
    if node.kind() == "identifier" {
        names.push(text(node, source).into());
    } else {
        for child in children(node) {
            identifiers(child, source, names);
        }
    }
}

fn collect_bindings(node: Node, source: &str, names: &mut Vec<String>, first: bool) {
    if !first && is_function(node.kind()) {
        return;
    }
    if matches!(
        node.kind(),
        "short_var_declaration" | "assignment_statement"
    ) {
        if let Some(left) = node.child_by_field_name("left") {
            identifiers(left, source, names);
        }
    }
    if node.kind() == "variable_declarator" {
        if let Some(name) = node.child_by_field_name("name") {
            // Function-valued declarations are resolved from the symbol table.
            if !node
                .child_by_field_name("value")
                .is_some_and(|n| is_function(n.kind()))
            {
                identifiers(name, source, names);
            }
        }
    }
    if node.kind() == "assignment" {
        if let Some(left) = node.child_by_field_name("left") {
            if !node
                .child_by_field_name("right")
                .is_some_and(|n| is_function(n.kind()))
            {
                identifiers(left, source, names);
            }
        }
    }
    if node.kind() == "declaration" {
        // C/C++: local declarations shadow later calls. Pure function prototypes
        // only declare a callable name, so they must not count as a binding.
        let is_typedef = children(node)
            .iter()
            .any(|n| n.kind() == "storage_class_specifier" && text(*n, source) == "typedef");
        if !is_typedef {
            for declarator in children(node) {
                match declarator.kind() {
                    "init_declarator" => {
                        if let Some(inner) = declarator.child_by_field_name("declarator") {
                            if let Some((name, _)) = declarator_parts(inner, source) {
                                names.push(name);
                            }
                        }
                    }
                    "function_declarator" => {
                        // `int f(int);` is a prototype; `int (*fp)(int);` declares a
                        // function-pointer variable that does shadow calls.
                        let declares_variable = declarator
                            .child_by_field_name("declarator")
                            .is_some_and(|inner| {
                                !matches!(
                                    inner.kind(),
                                    "identifier" | "field_identifier" | "qualified_identifier"
                                )
                            });
                        if declares_variable {
                            if let Some((name, _)) = declarator_parts(declarator, source) {
                                names.push(name);
                            }
                        }
                    }
                    "identifier"
                    | "pointer_declarator"
                    | "array_declarator"
                    | "parenthesized_declarator"
                    | "reference_declarator" => {
                        if let Some((name, _)) = declarator_parts(declarator, source) {
                            names.push(name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for child in children(node) {
        collect_bindings(child, source, names, false);
    }
}

fn role(node: Node, source: &str) -> (String, String) {
    let mut previous = node.prev_named_sibling();
    if previous.is_none() {
        previous = node
            .parent()
            .filter(|p| {
                matches!(
                    p.kind(),
                    "export_statement" | "lexical_declaration" | "decorated_definition"
                )
            })
            .and_then(|p| p.prev_named_sibling());
    }
    if let Some(comment) = previous
        .filter(|n| n.kind() == "comment" && node.start_position().row <= n.end_position().row + 2)
    {
        let value = text(comment, source)
            .trim()
            .trim_start_matches('/')
            .trim_start_matches('*')
            .trim_start_matches('#')
            .trim_end_matches("*/")
            .trim();
        if !value.is_empty() {
            return (
                value
                    .lines()
                    .next()
                    .unwrap_or(value)
                    .chars()
                    .take(140)
                    .collect(),
                "source-comment".into(),
            );
        }
    }
    if let Some(body) = node.child_by_field_name("body") {
        if let Some(first) = body
            .named_child(0)
            .filter(|n| n.kind() == "expression_statement")
        {
            if let Some(string) = first.named_child(0).filter(|n| n.kind() == "string") {
                let value = text(string, source).trim_matches(['\'', '"']).trim();
                if !value.is_empty() {
                    return (
                        value
                            .lines()
                            .next()
                            .unwrap_or(value)
                            .chars()
                            .take(140)
                            .collect(),
                        "source-docstring".into(),
                    );
                }
            }
        }
    }
    (
        "源码未提供职责说明；请结合函数体阅读。".into(),
        "no-description".into(),
    )
}

fn imports(node: Node, source: &str, output: &mut Vec<Import>) {
    match node.kind() {
        "import_statement" if node.child_by_field_name("source").is_some() => {
            let from = field_text(node, "source", source)
                .trim_matches(['\'', '"'])
                .to_string();
            fn js(node: Node, source: &str, from: &str, out: &mut Vec<Import>) {
                match node.kind() {
                    "import_specifier" => {
                        let original = field_text(node, "name", source);
                        let local = node
                            .child_by_field_name("alias")
                            .map(|n| text(n, source).into())
                            .unwrap_or(original.clone());
                        out.push(Import {
                            local,
                            original,
                            source: from.into(),
                            namespace: false,
                        });
                    }
                    "namespace_import" => {
                        if let Some(local) = children(node).last() {
                            out.push(Import {
                                local: text(*local, source).into(),
                                original: "*".into(),
                                source: from.into(),
                                namespace: true,
                            });
                        }
                    }
                    "identifier" => out.push(Import {
                        local: text(node, source).into(),
                        original: "default".into(),
                        source: from.into(),
                        namespace: false,
                    }),
                    _ => {
                        for c in children(node) {
                            js(c, source, from, out);
                        }
                    }
                }
            }
            for child in children(node)
                .into_iter()
                .filter(|n| n.kind() == "import_clause")
            {
                js(child, source, &from, output);
            }
        }
        "import_from_statement" => {
            let module = field_text(node, "module_name", source);
            for child in children(node) {
                if Some(child.id()) == node.child_by_field_name("module_name").map(|n| n.id()) {
                    continue;
                }
                if child.kind() == "aliased_import" {
                    output.push(Import {
                        local: field_text(child, "alias", source),
                        original: field_text(child, "name", source),
                        source: module.clone(),
                        namespace: false,
                    });
                } else if matches!(child.kind(), "dotted_name" | "identifier") {
                    let name = text(child, source).to_string();
                    output.push(Import {
                        local: name.clone(),
                        original: name,
                        source: module.clone(),
                        namespace: false,
                    });
                }
            }
        }
        "import_statement" => {
            for child in children(node) {
                if child.kind() == "aliased_import" {
                    output.push(Import {
                        local: field_text(child, "alias", source),
                        original: "*".into(),
                        source: field_text(child, "name", source),
                        namespace: true,
                    });
                } else if child.kind() == "dotted_name" && !text(child, source).contains('.') {
                    output.push(Import {
                        local: text(child, source).into(),
                        original: "*".into(),
                        source: text(child, source).into(),
                        namespace: true,
                    });
                }
            }
        }
        _ => {}
    }
    for child in children(node) {
        imports(child, source, output);
    }
}

fn visit(
    node: Node,
    source: &str,
    language: &str,
    unit: &mut Unit,
    owner: &str,
    scope: Option<String>,
) {
    let mut active = owner.to_string();
    let mut child_scope = scope;
    if is_function(node.kind()) {
        // Even anonymous callbacks own their calls. Never attribute callback bodies to the enclosing function.
        let name = function_name(node, source).unwrap_or_else(|| {
            format!(
                "<callback@{}:{}>",
                node.start_position().row + 1,
                node.start_position().column + 1
            )
        });
        let line = node.start_position().row + 1;
        let id = format!(
            "{}:{}:{}:{}",
            unit.file,
            line,
            node.start_position().column + 1,
            name
        );
        let class = class_name(node, source);
        let mut bindings = vec![];
        // C/C++ expose parameters through the declarator chain; other languages
        // carry them directly on the function node.
        if let Some(params) = function_parameters(node) {
            identifiers(params, source, &mut bindings);
        }
        if let Some(body) = node.child_by_field_name("body") {
            collect_bindings(body, source, &mut bindings, true);
        }
        let mut ancestor = node.parent();
        let mut exported = language == "Python";
        let mut default_export = false;
        while let Some(parent) = ancestor {
            if is_function(parent.kind()) || parent.kind() == "program" {
                break;
            }
            if parent.kind() == "export_statement" {
                exported = true;
                default_export = text(parent, source)
                    .trim_start()
                    .starts_with("export default");
                break;
            }
            ancestor = parent.parent();
        }
        let (role, role_source) = role(node, source);
        let property_function = node.parent().is_some_and(|p| p.kind() == "pair");
        unit.nodes.push(Function {
            id: id.clone(),
            name,
            file: unit.file.clone(),
            line,
            end_line: node.end_position().row + 1,
            kind: if class.is_some() || node.kind() == "method_definition" {
                "method"
            } else if property_function {
                "property-function"
            } else if node.kind() == "arrow_function" {
                "arrow"
            } else {
                "function"
            }
            .into(),
            role,
            role_source,
            source: text(node, source).chars().take(12000).collect(),
            signature: source[node.start_byte()
                ..node
                    .child_by_field_name("body")
                    .map(|b| b.start_byte())
                    .unwrap_or(node.end_byte())]
                .trim()
                .chars()
                .take(2000)
                .collect(),
            parameters: function_parameters(node)
                .map(|p| text(p, source).chars().take(1200).collect())
                .unwrap_or_default(),
            return_type: node
                .child_by_field_name("return_type")
                .or_else(|| node.child_by_field_name("result"))
                .or_else(|| node.child_by_field_name("type"))
                .map(|p| text(p, source).chars().take(800).collect())
                .unwrap_or_default(),
            language: language.into(),
            symbol_key: String::new(),
            content_hash: String::new(),
            start: node.start_byte(),
            end: node.end_byte(),
            scope: child_scope.clone(),
            class,
            bindings,
            exported,
            default_export,
        });
        active = id.clone();
        child_scope = Some(id);
    }
    if matches!(node.kind(), "call_expression" | "call") {
        if let Some(function) = node.child_by_field_name("function") {
            let direct = function.kind() == "identifier";
            let receiver = function
                .child_by_field_name("object")
                .or_else(|| function.child_by_field_name("operand"))
                // C/C++ field_expression (`this->method()`, `obj.method()`).
                .or_else(|| function.child_by_field_name("argument"))
                .map(|n| text(n, source).to_string())
                // C++ qualified_identifier (`Class::method`, `app::Base::method`).
                .or_else(|| {
                    (function.kind() == "qualified_identifier")
                        .then(|| qualified_parts(text(function, source)))
                        .flatten()
                        .map(|(scope, _)| scope.to_string())
                });
            let name = if direct {
                text(function, source).to_string()
            } else {
                function
                    .child_by_field_name("property")
                    .or_else(|| function.child_by_field_name("attribute"))
                    .or_else(|| function.child_by_field_name("field"))
                    .or_else(|| function.child_by_field_name("name"))
                    .map(|n| text(n, source).to_string())
                    // Nested `app::Base::method` exposes only the last `::` segment
                    // as the callable name.
                    .map(|raw| {
                        qualified_parts(&raw)
                            .map(|(_, name)| name.to_string())
                            .unwrap_or(raw)
                    })
                    .unwrap_or_default()
            };
            unit.calls.push(Call {
                owner: active.clone(),
                expression: text(function, source).chars().take(180).collect(),
                line: node.start_position().row + 1,
                direct,
                receiver,
                name,
            });
        }
    }
    for child in children(node) {
        visit(child, source, language, unit, &active, child_scope.clone());
    }
}

fn collect_go_types(node: Node, source: &str, unit: &mut Unit) {
    if node.kind() == "type_spec" {
        if let (Some(name), Some(ty)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("type"),
        ) {
            if ty.kind() == "struct_type" {
                let mut fields = std::collections::BTreeMap::new();
                for list in children(ty) {
                    for field in children(list)
                        .into_iter()
                        .filter(|n| n.kind() == "field_declaration")
                    {
                        if let Some(ty) = field.child_by_field_name("type") {
                            let value = text(ty, source).trim_start_matches('*').to_string();
                            for name in children(field)
                                .into_iter()
                                .filter(|n| n.kind() == "field_identifier")
                            {
                                fields.insert(text(name, source).to_string(), value.clone());
                            }
                        }
                    }
                }
                unit.struct_fields
                    .insert(text(name, source).to_string(), fields);
            }
        }
    }
    if matches!(
        node.kind(),
        "parameter_declaration" | "var_spec" | "short_var_declaration" | "assignment_statement"
    ) {
        let owner = unit
            .nodes
            .iter()
            .filter(|n| n.start <= node.start_byte() && n.end >= node.end_byte())
            .min_by_key(|n| n.end - n.start)
            .map(|n| n.id.clone());
        if let Some(owner) = owner {
            let value = if let Some(ty) = node.child_by_field_name("type") {
                text(ty, source).trim_start_matches('*').to_string()
            } else {
                let value = node
                    .child_by_field_name("right")
                    .or_else(|| node.child_by_field_name("value"))
                    .and_then(|n| n.named_child(0));
                let value = value.and_then(|n| {
                    if n.kind() == "unary_expression" {
                        n.child_by_field_name("operand")
                    } else {
                        Some(n)
                    }
                });
                value
                    .filter(|n| n.kind() == "composite_literal")
                    .and_then(|n| n.child_by_field_name("type"))
                    .map(|n| text(n, source).to_string())
                    .unwrap_or_default()
            };
            let names = if let Some(left) = node.child_by_field_name("left") {
                children(left)
                    .into_iter()
                    .filter(|n| n.kind() == "identifier")
                    .map(|n| text(n, source).to_string())
                    .collect::<Vec<_>>()
            } else {
                children(node)
                    .into_iter()
                    .filter(|n| n.kind() == "identifier")
                    .map(|n| text(n, source).to_string())
                    .collect()
            };
            for name in names {
                unit.local_types
                    .entry(owner.clone())
                    .or_default()
                    .entry(name)
                    .and_modify(|old| {
                        if *old != value {
                            old.clear()
                        }
                    })
                    .or_insert_with(|| value.clone());
            }
        }
    }
    for child in children(node) {
        collect_go_types(child, source, unit);
    }
}

/// Same-byte-length replacement used by [`mask_abstract_property_names`].
const ABSTRACT_MASK: &[u8; 8] = b"abs_trac";

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

/// Work around a tree-sitter-typescript 0.23.2 grammar gap (the newest grammar
/// available to us; upgrading/forks are not an option on this toolchain):
/// `abstract` is a keyword token in the grammar, but it is missing from the
/// `_reserved_identifier` list in `common/define-grammar.js`, which is the only
/// place that lets keywords appear as identifiers. Consequently a *valid*
/// TypeScript property/method/field named `abstract` in an interface, class or
/// object/type-literal body (`abstract?: string`, `abstract(): void`,
/// `abstract = 1`, ...) tokenizes as the `abstract` keyword, produces a parser
/// ERROR, and is reported as unsupported syntax. `abstract class`, `abstract
/// new` and `abstract readonly` are legal modifier positions and already parse.
///
/// The workaround masks only `abstract` occurrences in property position with a
/// dummy identifier of exactly the same byte length, so the masked text has
/// identical byte offsets. The retry tree may therefore use the *original*
/// source for every name/text/hash/body extraction. Returns `None` when the
/// source contains nothing that needs masking.
fn mask_abstract_property_names(source: &str) -> Option<String> {
    const KEYWORD: &[u8; 8] = b"abstract";
    let bytes = source.as_bytes();
    let mut masked: Option<Vec<u8>> = None;
    let mut index = 0;
    while index + KEYWORD.len() <= bytes.len() {
        if &bytes[index..index + KEYWORD.len()] != KEYWORD
            || (index > 0 && is_identifier_byte(bytes[index - 1]))
        {
            index += 1;
            continue;
        }
        // Inspect the first non-whitespace byte after the word.
        let mut next = index + KEYWORD.len();
        while next < bytes.len() && matches!(bytes[next], b' ' | b'\t' | b'\r' | b'\n') {
            next += 1;
        }
        if next >= bytes.len() || is_identifier_byte(bytes[next]) {
            // Modifier position (`abstract class`, `abstract new`, `abstract
            // readonly`, `abstract <member-name>`, ...): legal in the grammar,
            // never mask it.
            index += 1;
            continue;
        }
        // A property/method/field name — including optional `?`, definite
        // assignment `!`, generic `<`, initializer `=` and members in object
        // literals/type literals — is followed by one of these. Other positions
        // keep the original keyword and therefore the original diagnostic.
        if !matches!(
            bytes[next],
            b'?' | b':' | b'(' | b'=' | b'<' | b'!' | b';' | b','
        ) {
            index += 1;
            continue;
        }
        let buffer = masked.get_or_insert_with(|| bytes.to_vec());
        buffer[index..index + KEYWORD.len()].copy_from_slice(ABSTRACT_MASK);
        index += KEYWORD.len();
    }
    masked.map(|bytes| {
        String::from_utf8(bytes).expect("masking an ASCII word inside UTF-8 keeps the string valid")
    })
}

/// First `ERROR`/missing node in document order as a 1-based `(line, column)`.
fn first_error_position(node: Node) -> Option<(usize, usize)> {
    if node.is_error() || node.is_missing() {
        let point = node.start_position();
        return Some((point.row + 1, point.column + 1));
    }
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            if let Some(position) = first_error_position(child) {
                return Some(position);
            }
        }
    }
    None
}

fn parse(file: &str, source: &str) -> Result<Unit> {
    let full_digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    let extension = Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let (mut language, grammar) = match extension {
        "ts" => (
            "TypeScript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        ),
        "tsx" => ("TSX", tree_sitter_typescript::LANGUAGE_TSX.into()),
        "py" => ("Python", tree_sitter_python::LANGUAGE.into()),
        "go" => ("Go", tree_sitter_go::LANGUAGE.into()),
        "svelte" => (
            "Svelte/TypeScript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        ),
        "c" => ("C", tree_sitter_c::LANGUAGE.into()),
        "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" => {
            ("C++", tree_sitter_cpp::LANGUAGE.into())
        }
        // Headers are ambiguous; try C++ first and fall back to C below.
        "h" => ("C++", tree_sitter_cpp::LANGUAGE.into()),
        _ => ("JavaScript", tree_sitter_javascript::LANGUAGE.into()),
    };
    let embedded;
    let source = if extension == "svelte" {
        let mut bytes = source
            .as_bytes()
            .iter()
            .map(|b| if *b == b'\n' || *b == b'\r' { *b } else { b' ' })
            .collect::<Vec<_>>();
        let scripts = regex::Regex::new(r"(?is)<script\b[^>]*>(.*?)</script\s*>")?;
        for captures in scripts.captures_iter(source) {
            let m = captures.get(1).unwrap();
            bytes[m.start()..m.end()].copy_from_slice(m.as_str().as_bytes());
        }
        embedded = String::from_utf8(bytes)?;
        embedded.as_str()
    } else {
        source
    };
    let mut parser = Parser::new();
    parser.set_language(&grammar)?;
    let mut tree = parser
        .parse(source, None)
        .context("Tree-sitter returned no tree")?;
    // Retry once with `abstract`-as-property-name masked out (see
    // `mask_abstract_property_names` for the upstream grammar gap). For Svelte the
    // mask is applied to the script-extracted source, i.e. after embedding. The
    // masked text has the same byte length, so the retry tree maps onto the
    // original source byte-for-byte and all names/text/bodies/hashes below are
    // still read from the original source.
    let mut diagnostic_tree = None;
    if tree.root_node().has_error() && matches!(extension, "ts" | "tsx" | "svelte") {
        if let Some(masked) = mask_abstract_property_names(source) {
            if let Some(retry) = parser.parse(&masked, None) {
                if retry.root_node().has_error() {
                    // The file is genuinely malformed. Keep the original tree for
                    // extraction (no recovery surprises from the mask), but point
                    // the diagnostic at the retry tree: its first error is the real
                    // syntax error rather than the masked `abstract` false positive.
                    diagnostic_tree = Some(retry);
                } else {
                    tree = retry;
                }
            }
        }
    }
    // `.h` files are ambiguous by convention. C++ is the superset, so keep that
    // tree when it parses; otherwise retry with the C grammar (e.g. K&R function
    // definitions or C-only identifiers) and use the tree that parses cleanly.
    if extension == "h" && tree.root_node().has_error() {
        parser.set_language(&tree_sitter_c::LANGUAGE.into())?;
        if let Some(c_tree) = parser.parse(source, None) {
            if !c_tree.root_node().has_error() {
                tree = c_tree;
                language = "C";
            }
        }
    }
    let mut unit = Unit {
        file: file.into(),
        digest: full_digest,
        nodes: vec![],
        calls: vec![],
        imports: vec![],
        diagnostics: vec![],
        includes: {
            let mut includes = vec![];
            collect_includes(tree.root_node(), source, &mut includes);
            includes.sort();
            includes.dedup();
            includes
        },
        struct_fields: Default::default(),
        local_types: Default::default(),
        package: if extension == "go" {
            children(tree.root_node())
                .into_iter()
                .find(|n| n.kind() == "package_clause")
                .map(|n| {
                    text(n, source)
                        .trim_start_matches("package")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default()
        } else {
            String::new()
        },
    };
    if tree.root_node().has_error() {
        let reported = diagnostic_tree.as_ref().unwrap_or(&tree);
        let (line, column) = first_error_position(reported.root_node()).unwrap_or((1, 1));
        unit.diagnostics.push(format!(
            "{file}:{line}:{column}: 解析器发生语法恢复或遇到未支持的语法；该文件的图可能不完整。"
        ));
    }
    let id = format!("{file}:0:0:<module>");
    let mut bindings = vec![];
    collect_bindings(tree.root_node(), source, &mut bindings, true);
    unit.nodes.push(Function {
        id: id.clone(),
        name: "<module>".into(),
        file: file.into(),
        line: 1,
        end_line: source.lines().count(),
        kind: "module".into(),
        role: "模块顶层执行上下文；不代表运行时入口。".into(),
        role_source: "structural".into(),
        signature: String::new(),
        parameters: String::new(),
        return_type: String::new(),
        source: source.chars().take(12000).collect(),
        language: language.into(),
        symbol_key: String::new(),
        content_hash: String::new(),
        start: 0,
        end: source.len(),
        scope: None,
        class: None,
        bindings,
        exported: false,
        default_export: false,
    });
    imports(tree.root_node(), source, &mut unit.imports);
    visit(tree.root_node(), source, language, &mut unit, &id, None);
    // Reassignment makes a binding uncertain, including an initially function-valued binding.
    fn assignments(node: Node, source: &str, unit: &mut Unit) {
        if matches!(
            node.kind(),
            "assignment_expression" | "augmented_assignment_expression" | "augmented_assignment"
        ) {
            if let Some(left) = node
                .child_by_field_name("left")
                .filter(|n| n.kind() == "identifier")
            {
                let owner = unit
                    .nodes
                    .iter()
                    .filter(|f| f.start <= node.start_byte() && f.end >= node.end_byte())
                    .min_by_key(|f| f.end - f.start)
                    .map(|f| f.id.clone());
                if let Some(owner) = owner {
                    if let Some(f) = unit.nodes.iter_mut().find(|f| f.id == owner) {
                        f.bindings.push(text(left, source).into());
                    }
                }
            }
        }
        for c in children(node) {
            assignments(c, source, unit);
        }
    }
    assignments(tree.root_node(), source, &mut unit);
    if extension == "go" {
        collect_go_types(tree.root_node(), source, &mut unit);
    }
    // Stable identity excludes source coordinates but retains lexical/class context.
    let parents = unit
        .nodes
        .iter()
        .map(|n| (n.id.clone(), (n.name.clone(), n.scope.clone())))
        .collect::<std::collections::HashMap<_, _>>();
    for n in &mut unit.nodes {
        let mut ancestors = Vec::new();
        let mut scope = n.scope.clone();
        while let Some(id) = scope {
            if let Some((name, next)) = parents.get(&id) {
                ancestors.push(name.clone());
                scope = next.clone();
            } else {
                break;
            }
        }
        ancestors.reverse();
        if let Some(class) = &n.class {
            ancestors.push(class.clone());
        }
        ancestors.push(n.name.clone());
        n.symbol_key = format!("{}::{}::{}", n.file, ancestors.join("::"), n.kind);
        n.content_hash = format!(
            "{:x}",
            Sha256::digest(source[n.start..n.end].replace("\r\n", "\n").as_bytes())
        );
    }
    Ok(unit)
}

fn import_file(
    unit: &Unit,
    import: &Import,
    known: &HashSet<String>,
    python: bool,
) -> Option<String> {
    let parent = Path::new(&unit.file).parent().unwrap_or(Path::new(""));
    let base = if python {
        let dots = import.source.chars().take_while(|c| *c == '.').count();
        let tail = import.source[dots..].replace('.', "/");
        if dots > 0 {
            let mut p = parent.to_path_buf();
            for _ in 1..dots {
                p.pop();
            }
            p.join(tail)
        } else {
            PathBuf::from(tail)
        }
    } else {
        if !import.source.starts_with('.') {
            return None;
        }
        parent.join(&import.source)
    };
    let base = normalize(&base).to_string_lossy().replace('\\', "/");
    let mut candidates = vec![base.clone()];
    if python {
        candidates.extend([format!("{base}.py"), format!("{base}/__init__.py")]);
    } else {
        for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "svelte"] {
            candidates.push(format!("{base}.{ext}"));
            candidates.push(format!("{base}/index.{ext}"));
        }
        if base.ends_with(".js") {
            candidates.push(format!("{}.ts", base.trim_end_matches(".js")));
            candidates.push(format!("{}.tsx", base.trim_end_matches(".js")));
        }
    }
    candidates.into_iter().find(|p| known.contains(p))
}

fn shadowed<'a>(name: &str, owner: &'a Function, unit: &'a Unit) -> bool {
    let mut current = Some(owner);
    while let Some(f) = current {
        if f.bindings.iter().any(|b| b == name) {
            return true;
        }
        current = f
            .scope
            .as_ref()
            .and_then(|id| unit.nodes.iter().find(|n| &n.id == id));
    }
    unit.nodes
        .iter()
        .find(|n| n.kind == "module")
        .is_some_and(|f| f.bindings.iter().any(|b| b == name))
}

/// Precomputed C/C++ resolution scopes: for each unit, the indices of the unit
/// itself, units of its quoted local includes, and sibling implementation units
/// of those headers (`service.h` -> same directory `service.c|cc|cpp|cxx|c++`).
fn include_scopes(units: &[Unit]) -> Vec<Vec<usize>> {
    let by_file: HashMap<&str, usize> = units
        .iter()
        .enumerate()
        .map(|(index, unit)| (unit.file.as_str(), index))
        .collect();
    units
        .iter()
        .enumerate()
        .map(|(index, unit)| {
            let mut scope = vec![index];
            let parent = Path::new(&unit.file).parent().unwrap_or(Path::new(""));
            for include in &unit.includes {
                let resolved = normalize(&parent.join(include))
                    .to_string_lossy()
                    .replace('\\', "/");
                let Some(&header_index) = by_file.get(resolved.as_str()) else {
                    continue;
                };
                scope.push(header_index);
                let header = Path::new(&resolved);
                let directory = header.parent().unwrap_or(Path::new(""));
                let Some(stem) = header.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                for extension in ["c", "cc", "cpp", "cxx", "c++"] {
                    let sibling = normalize(&directory.join(format!("{stem}.{extension}")))
                        .to_string_lossy()
                        .replace('\\', "/");
                    if let Some(&sibling_index) = by_file.get(sibling.as_str()) {
                        scope.push(sibling_index);
                    }
                }
            }
            scope.sort_unstable();
            scope.dedup();
            scope
        })
        .collect()
}

fn resolve_c_family_call<'a>(
    call: &Call,
    owner: &'a Function,
    unit: &'a Unit,
    units: &'a [Unit],
    scopes: &[Vec<usize>],
    unit_index: usize,
) -> std::result::Result<(&'a Function, &'static str), &'static str> {
    let scope = scopes.get(unit_index).map(Vec::as_slice).unwrap_or(&[]);
    let scoped_nodes = || scope.iter().flat_map(|&index| units[index].nodes.iter());
    let free = |node: &Function| {
        node.class.is_none()
            && node.scope.is_none()
            && !matches!(node.kind.as_str(), "module" | "method" | "property-function")
    };
    if call.direct {
        if shadowed(&call.name, owner, unit) {
            return Err("shadowed-or-assigned-binding");
        }
        // Same-file free function first, mirroring the lexical rule used for the
        // other languages.
        let same_file = unit
            .nodes
            .iter()
            .filter(|node| free(node) && node.name == call.name)
            .collect::<Vec<_>>();
        match same_file.as_slice() {
            [target] => return Ok((target, "lexical-static")),
            [] => {}
            _ => return Err("ambiguous-local-symbol"),
        }
        // Then a unique definition from the include/sibling implementation scope.
        let included = scoped_nodes()
            .filter(|node| free(node) && node.name == call.name)
            .collect::<Vec<_>>();
        match included.as_slice() {
            [target] => return Ok((target, "c-include-static")),
            [] => {}
            _ => return Err("ambiguous-local-symbol"),
        }
        // Unqualified call to a sibling C++ method from inside a class method.
        if let Some(class) = owner.class.as_deref() {
            let siblings = scoped_nodes()
                .filter(|node| node.class.as_deref() == Some(class) && node.name == call.name)
                .collect::<Vec<_>>();
            return match siblings.as_slice() {
                [target] => Ok((target, "same-class-static-candidate")),
                [] => Err("external-or-unknown-symbol"),
                _ => Err("unknown-or-ambiguous-method"),
            };
        }
        return Err("external-or-unknown-symbol");
    }
    let receiver = call.receiver.as_deref().unwrap_or("");
    if matches!(receiver, "this" | "self" | "cls") {
        let Some(class) = owner.class.as_deref() else {
            return Err("unknown-method-context");
        };
        let methods = scoped_nodes()
            .filter(|node| node.class.as_deref() == Some(class) && node.name == call.name)
            .collect::<Vec<_>>();
        return match methods.as_slice() {
            [target] => Ok((target, "same-class-static-candidate")),
            _ => Err("unknown-or-ambiguous-method"),
        };
    }
    // `Class::method()` resolves only when the receiver names a class/struct/union
    // that has exactly one matching method definition in scope.
    let class_names = scoped_nodes()
        .filter_map(|node| node.class.as_deref())
        .collect::<HashSet<_>>();
    if !receiver.is_empty() && class_names.contains(receiver) {
        let methods = scoped_nodes()
            .filter(|node| node.class.as_deref() == Some(receiver) && node.name == call.name)
            .collect::<Vec<_>>();
        return match methods.as_slice() {
            [target] => Ok((target, "cpp-class-static-candidate")),
            _ => Err("dynamic-or-unknown-receiver"),
        };
    }
    Err("dynamic-or-unknown-receiver")
}

fn resolve_call<'a>(
    call: &Call,
    unit: &'a Unit,
    units: &'a [Unit],
    known: &HashSet<String>,
    scopes: &[Vec<usize>],
    unit_index: usize,
) -> std::result::Result<(&'a Function, &'static str), &'static str> {
    let owner = unit
        .nodes
        .iter()
        .find(|n| n.id == call.owner)
        .ok_or("missing-owner")?;
    if owner.language == "Go" {
        let package_units = units
            .iter()
            .filter(|u| {
                Path::new(&u.file).parent() == Path::new(&unit.file).parent()
                    && u.package == unit.package
            })
            .collect::<Vec<_>>();
        if call.direct {
            if shadowed(&call.name, owner, unit) {
                return Err("shadowed-or-assigned-binding");
            }
            let matches = package_units
                .iter()
                .flat_map(|u| &u.nodes)
                .filter(|n| {
                    n.name == call.name
                        && n.class.is_none()
                        && n.scope.is_none()
                        && n.kind != "module"
                })
                .collect::<Vec<_>>();
            return match matches.as_slice() {
                [n] => Ok((n, "go-package-static")),
                [] => Err("go-external-or-unknown-symbol"),
                _ => Err("go-ambiguous-package-symbol"),
            };
        }
        if let Some(receiver) = &call.receiver {
            let mut segments = receiver.split('.');
            let root = segments.next().unwrap_or("");
            let mut context = Some(owner);
            let mut ty = None;
            while let Some(n) = context {
                if let Some(value) = unit.local_types.get(&n.id).and_then(|m| m.get(root)) {
                    ty = Some(value.clone());
                    break;
                }
                context = n
                    .scope
                    .as_ref()
                    .and_then(|id| unit.nodes.iter().find(|n| &n.id == id));
            }
            if ty.is_none() {
                let globals = package_units
                    .iter()
                    .filter_map(|u| {
                        u.nodes
                            .iter()
                            .find(|n| n.kind == "module")
                            .and_then(|n| u.local_types.get(&n.id))
                            .and_then(|m| m.get(root))
                    })
                    .collect::<Vec<_>>();
                if globals.len() == 1 {
                    ty = Some(globals[0].clone());
                }
            }
            if let Some(mut ty) = ty {
                for field in segments {
                    let fields = package_units
                        .iter()
                        .filter_map(|u| u.struct_fields.get(&ty).and_then(|f| f.get(field)))
                        .collect::<Vec<_>>();
                    if fields.len() != 1 {
                        return Err("go-unknown-struct-field-type");
                    }
                    ty = fields[0].clone();
                }
                let matches = package_units
                    .iter()
                    .flat_map(|u| &u.nodes)
                    .filter(|n| n.name == call.name && n.class.as_deref() == Some(ty.as_str()))
                    .collect::<Vec<_>>();
                return match matches.as_slice() {
                    [n] => Ok((n, "go-typed-receiver-static-candidate")),
                    _ => Err("go-interface-or-unknown-method"),
                };
            }
        }
        if let (Some(class), Some(receiver)) = (&owner.class, &call.receiver) {
            let declaration = owner.source.split('{').next().unwrap_or("");
            let receiver_name = declaration
                .split_once('(')
                .and_then(|(_, s)| s.split_whitespace().next());
            if receiver_name == Some(receiver.as_str()) {
                let matches = package_units
                    .iter()
                    .flat_map(|u| &u.nodes)
                    .filter(|n| n.name == call.name && n.class.as_ref() == Some(class))
                    .collect::<Vec<_>>();
                return match matches.as_slice() {
                    [n] => Ok((n, "go-receiver-static-candidate")),
                    _ => Err("go-unknown-receiver-method"),
                };
            }
        }
        return Err("go-selector-requires-type-resolution");
    }
    if is_c_family(&owner.language) {
        return resolve_c_family_call(call, owner, unit, units, scopes, unit_index);
    }
    if call.direct {
        if shadowed(&call.name, owner, unit) {
            return Err("shadowed-or-assigned-binding");
        }
        // Lexical lookup; never connect unrelated same-name global functions across files.
        let mut level = if owner.kind == "module" {
            None
        } else {
            Some(owner.id.clone())
        };
        loop {
            let found: Vec<_> = unit
                .nodes
                .iter()
                .filter(|n| {
                    !matches!(n.kind.as_str(), "module" | "method" | "property-function")
                        && n.class.is_none()
                        && n.name == call.name
                        && n.scope == level
                })
                .collect();
            match found.as_slice() {
                [f] => return Ok((f, "lexical-static")),
                [] => {}
                _ => return Err("ambiguous-local-symbol"),
            }
            if let Some(id) = level {
                level = unit
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(|n| n.scope.clone());
            } else {
                break;
            }
        }
    } else if let Some(receiver) = &call.receiver {
        if shadowed(receiver, owner, unit) && !matches!(receiver.as_str(), "self" | "cls") {
            return Err("shadowed-receiver");
        }
        if matches!(receiver.as_str(), "this" | "self" | "cls") {
            let class = owner.class.as_ref().ok_or("unknown-method-context")?;
            let found: Vec<_> = unit
                .nodes
                .iter()
                .filter(|n| n.class.as_ref() == Some(class) && n.name == call.name)
                .collect();
            return match found.as_slice() {
                [f] => Ok((f, "same-class-static-candidate")),
                _ => Err("unknown-or-ambiguous-method"),
            };
        }
        // Class-name dispatch is intentionally left unresolved without a type checker.
    }
    let local = if call.direct {
        &call.name
    } else {
        call.receiver.as_ref().ok_or("dynamic-call")?
    };
    let matches: Vec<_> = unit
        .imports
        .iter()
        .filter(|i| &i.local == local && (call.direct != i.namespace))
        .collect();
    let import = match matches.as_slice() {
        [i] => *i,
        [] => {
            return Err(if call.direct {
                "external-or-unknown-symbol"
            } else {
                "dynamic-or-unknown-receiver"
            });
        }
        _ => return Err("ambiguous-import"),
    };
    let file = import_file(unit, import, known, owner.language == "Python")
        .ok_or("external-or-unresolved-module")?;
    let name = if import.namespace {
        &call.name
    } else {
        &import.original
    };
    let found: Vec<_> = units
        .iter()
        .find(|u| u.file == file)
        .into_iter()
        .flat_map(|u| &u.nodes)
        .filter(|n| {
            n.scope.is_none()
                && n.class.is_none()
                && !matches!(n.kind.as_str(), "module" | "method" | "property-function")
                && n.exported
                && (if name == "default" {
                    n.default_export
                } else {
                    !n.default_export && &n.name == name
                })
        })
        .collect();
    match found.as_slice() {
        [f] => Ok((f, "explicit-import-static")),
        [] => Err("missing-export-or-re-export"),
        _ => Err("ambiguous-export"),
    }
}

pub fn scan(root: &Path) -> Result<Graph> {
    scan_cached(root, None)
}

#[derive(Serialize, Deserialize)]
struct Internals {
    scope: Option<String>,
    class: Option<String>,
    bindings: Vec<String>,
    exported: bool,
    default_export: bool,
}
#[derive(Serialize, Deserialize)]
struct CachedUnit {
    unit: Unit,
    internals: Vec<Internals>,
}
#[derive(Serialize, Deserialize, Default)]
struct ParseCache {
    version: String,
    root: String,
    files: std::collections::BTreeMap<String, CachedUnit>,
}
pub fn scan_cached(root: &Path, cache_path: Option<&Path>) -> Result<Graph> {
    let version = format!(
        "{}:{:x}",
        env!("CARGO_PKG_VERSION"),
        Sha256::digest(include_bytes!("analysis.rs"))
    );
    let root_key = root.canonicalize()?.to_string_lossy().to_string();
    let mut cache = cache_path
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<ParseCache>(&b).ok())
        .filter(|c| c.version == version && c.root == root_key)
        .unwrap_or_default();
    let mut paths = vec![];
    let mut diagnostics = vec![];
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some(
                    "node_modules"
                        | "target"
                        | "dist"
                        | "build"
                        | "vendor"
                        | ".git"
                        | ".venv"
                        | "venv"
                        | "__pycache__"
                )
            )
        })
        .build();
    for entry in walker {
        match entry {
            Ok(entry) if entry.file_type().is_some_and(|f| f.is_file()) => {
                let p = entry.into_path();
                if p.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(supported_extension)
                    && !p.to_string_lossy().ends_with(".d.ts")
                {
                    paths.push(p);
                }
            }
            Err(e) => diagnostics.push(format!("无法读取目录项：{e}")),
            _ => {}
        }
    }
    paths.sort();
    let hits = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<_> = paths
        .par_iter()
        .map(|path| {
            let file = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let size = fs::metadata(path)?.len();
            anyhow::ensure!(size <= 2_000_000, "{file}: 文件超过 2 MB，跳过");
            let source = fs::read_to_string(path).with_context(|| format!("Cannot read {file}"))?;
            let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
            if let Some(c) = cache.files.get(&file).filter(|c| c.unit.digest == digest) {
                let mut unit = c.unit.clone();
                for (n, i) in unit.nodes.iter_mut().zip(&c.internals) {
                    n.scope = i.scope.clone();
                    n.class = i.class.clone();
                    n.bindings = i.bindings.clone();
                    n.exported = i.exported;
                    n.default_export = i.default_export;
                }
                hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Ok(unit);
            }
            parse(&file, &source)
        })
        .collect::<Vec<Result<Unit>>>();
    if let Some(path) = cache_path {
        cache.version = version;
        cache.root = root_key;
        cache.files = results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .map(|u| {
                (
                    u.file.clone(),
                    CachedUnit {
                        unit: u.clone(),
                        internals: u
                            .nodes
                            .iter()
                            .map(|n| Internals {
                                scope: n.scope.clone(),
                                class: n.class.clone(),
                                bindings: n.bindings.clone(),
                                exported: n.exported,
                                default_export: n.default_export,
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        crate::write_atomic(path, serde_json::to_vec(&cache)?)?;
    }
    let count = results.iter().filter(|r| r.is_ok()).count();
    let mut graph = assemble(
        &root.file_name().unwrap_or_default().to_string_lossy(),
        results,
        diagnostics,
    )?;
    graph.cache_hits = hits.load(std::sync::atomic::Ordering::Relaxed);
    graph.parsed_files = count - graph.cache_hits;
    Ok(graph)
}

pub fn supported_file(file: &str) -> bool {
    !file.ends_with(".d.ts")
        && Path::new(file)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(supported_extension)
        && !file.split('/').any(|part| {
            part.starts_with('.')
                || matches!(
                    part,
                    "node_modules"
                        | "target"
                        | "dist"
                        | "build"
                        | "vendor"
                        | "venv"
                        | "__pycache__"
                )
        })
}

pub fn scan_sources(
    project: &str,
    sources: &[(String, String)],
    diagnostics: Vec<String>,
) -> Result<Graph> {
    let mut sources = sources.iter().collect::<Vec<_>>();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    let results = sources
        .par_iter()
        .map(|(file, source)| parse(file, source))
        .collect();
    assemble(project, results, diagnostics)
}

fn assemble(
    project: &str,
    results: Vec<Result<Unit>>,
    mut diagnostics: Vec<String>,
) -> Result<Graph> {
    let mut units = vec![];
    for result in results {
        match result {
            Ok(unit) => units.push(unit),
            Err(e) => diagnostics.push(format!("{e:#}")),
        }
    }
    let known: HashSet<_> = units.iter().map(|u| u.file.clone()).collect();
    // C/C++ resolve through quoted includes and their sibling implementations;
    // precompute the per-unit scope indices once for the whole graph.
    let scopes = include_scopes(&units);
    let mut hasher = Sha256::new();
    for unit in &units {
        hasher.update(unit.file.as_bytes());
        hasher.update([0]);
        hasher.update(unit.digest.as_bytes());
        hasher.update([0]);
    }
    let mut graph = Graph {
        schema_version: 2,
        snapshot: format!("{:x}", hasher.finalize()),
        project: project.into(),
        files: units.len(),
        diagnostics,
        ..Graph::default()
    };
    for (unit_index, unit) in units.iter().enumerate() {
        graph.diagnostics.extend(unit.diagnostics.clone());
        graph.nodes.extend(
            unit.nodes
                .iter()
                .filter(|n| n.kind != "module" || unit.calls.iter().any(|c| c.owner == n.id))
                .cloned(),
        );
        for call in &unit.calls {
            match resolve_call(call, unit, &units, &known, &scopes, unit_index) {
                Ok((target, resolution)) => graph.edges.push(Edge {
                    from: call.owner.clone(),
                    to: target.id.clone(),
                    file: unit.file.clone(),
                    line: call.line,
                    resolution: resolution.into(),
                }),
                Err(reason) => graph.unresolved.push(Unresolved {
                    from: call.owner.clone(),
                    expression: call.expression.clone(),
                    file: unit.file.clone(),
                    line: call.line,
                    reason: reason.into(),
                }),
            }
        }
    }
    graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    graph
        .edges
        .sort_by(|a, b| (&a.from, &a.to, a.line).cmp(&(&b.from, &b.to, b.line)));
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn analyze(files: &[(&str, &str)]) -> Graph {
        let units: Vec<_> = files.iter().map(|(p, s)| parse(p, s).unwrap()).collect();
        let known = units.iter().map(|u| u.file.clone()).collect();
        let scopes = include_scopes(&units);
        let mut graph = Graph::default();
        for (unit_index, unit) in units.iter().enumerate() {
            graph.diagnostics.extend(unit.diagnostics.clone());
            graph.nodes.extend(unit.nodes.clone());
            for call in &unit.calls {
                match resolve_call(call, unit, &units, &known, &scopes, unit_index) {
                    Ok((f, r)) => graph.edges.push(Edge {
                        from: call.owner.clone(),
                        to: f.id.clone(),
                        file: unit.file.clone(),
                        line: call.line,
                        resolution: r.into(),
                    }),
                    Err(r) => graph.unresolved.push(Unresolved {
                        from: call.owner.clone(),
                        expression: call.expression.clone(),
                        file: unit.file.clone(),
                        line: call.line,
                        reason: r.into(),
                    }),
                }
            }
        }
        graph
    }
    #[test]
    fn imports_aliases_namespace_and_no_cross_file_guessing() {
        let g = analyze(&[
            ("config.ts", "export function parseConfig() {}"),
            (
                "main.ts",
                "import {parseConfig as parse} from './config'; import * as cfg from './config'; function main(){parse(); cfg.parseConfig(); parseConfig();}",
            ),
        ]);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.unresolved.len(), 1);
        assert_eq!(g.unresolved[0].expression, "parseConfig");
    }
    #[test]
    fn parameter_shadow_and_dynamic_receivers_are_not_edges() {
        let g = analyze(&[(
            "a.js",
            "function run(){} function caller(run){ run(); } function other(obj){obj.run();} function good(){run();}",
        )]);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.unresolved.len(), 2);
    }
    #[test]
    fn nested_function_ownership_and_lexical_scope() {
        let g = analyze(&[(
            "a.ts",
            "function outer(){function inner(){outer();} inner();} function wrong(){inner();}",
        )]);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.unresolved.len(), 1);
        assert!(
            g.path(&g.resolve("outer").unwrap(), &g.resolve("inner").unwrap())
                .is_some()
        );
    }
    #[test]
    fn python_relative_imports_and_docstrings() {
        let g = analyze(&[
            (
                "pkg/config.py",
                "def parse_config():\n    \"\"\"解析配置来源。\"\"\"\n    return {}\n",
            ),
            (
                "pkg/main.py",
                "from .config import parse_config as parse\ndef main():\n    return parse()\n",
            ),
        ]);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(
            g.nodes
                .iter()
                .find(|n| n.name == "parse_config")
                .unwrap()
                .role,
            "解析配置来源。"
        );
    }
    #[test]
    fn methods_and_arrow_functions_are_owned() {
        let g = analyze(&[(
            "a.ts",
            "class Config { parse(){return this.validate();} validate(){} } const run = () => {}; function main(){run();}",
        )]);
        assert_eq!(g.edges.len(), 2);
    }
    #[test]
    fn callback_is_not_a_direct_call_from_enclosing_function() {
        let g = analyze(&[(
            "a.js",
            "function sink(){} function main(){register(() => sink());}",
        )]);
        let main = g.resolve("main").unwrap();
        let sink = g.resolve("sink").unwrap();
        assert_eq!(g.path(&main, &sink), None);
        assert_eq!(g.edges.len(), 1);
    }
    #[test]
    fn duplicate_symbols_fail_closed() {
        let g = analyze(&[(
            "a.js",
            "function run(){} function run(){} function main(){run();}",
        )]);
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.unresolved[0].reason, "ambiguous-local-symbol");
        assert!(g.resolve("run").is_err());
    }
    #[test]
    fn malformed_source_reports_diagnostic() {
        assert!(
            !parse("a.ts", "function broken( {")
                .unwrap()
                .diagnostics
                .is_empty()
        );
    }
    #[test]
    fn object_properties_are_not_lexical_functions() {
        let g = analyze(&[(
            "a.ts",
            "const obj={run:()=>{}, walk(){}}; function main(){run(); walk();}",
        )]);
        assert!(g.edges.is_empty());
        assert_eq!(g.unresolved.len(), 2);
    }
    #[test]
    fn named_function_expressions_resolve_the_assigned_binding() {
        let g = analyze(&[(
            "a.js",
            "const run=function internal(){}; function main(){run();}",
        )]);
        assert_eq!(g.edges.len(), 1);
        assert!(g.resolve("run").is_ok());
    }
    #[test]
    fn default_exports_are_not_named_exports() {
        let g = analyze(&[
            ("config.ts", "export default function parse(){}"),
            (
                "main.ts",
                "import parseDefault from './config'; import {parse} from './config'; function main(){parseDefault();parse();}",
            ),
        ]);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.unresolved.len(), 1);
    }
    #[test]
    fn python_lambda_owns_its_calls() {
        let g = analyze(&[(
            "a.py",
            "def sink():\n    pass\ndef main():\n    callback = lambda: sink()\n    return callback\n",
        )]);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(
            g.path(&g.resolve("main").unwrap(), &g.resolve("sink").unwrap()),
            None
        );
    }
    fn diagnostic_line_col(diagnostic: &str, file: &str) -> (usize, usize) {
        let rest = diagnostic
            .strip_prefix(&format!("{file}:"))
            .unwrap_or_else(|| panic!("unexpected diagnostic: {diagnostic}"));
        let mut parts = rest.splitn(3, ':');
        let line = parts.next().unwrap().parse().unwrap();
        let column = parts.next().unwrap().parse().unwrap();
        assert_eq!(
            parts.next().unwrap(),
            " 解析器发生语法恢复或遇到未支持的语法；该文件的图可能不完整。"
        );
        (line, column)
    }
    #[test]
    fn abstract_named_interface_properties_are_not_a_parse_error() {
        // Regression: tree-sitter-typescript 0.23.2 only tokenizes `abstract` as
        // the keyword, so these valid properties used to produce a false
        // "unsupported syntax" diagnostic and could drop symbols.
        let unit = parse(
            "props.ts",
            "interface Options {\n  abstract?: string;\n  abstract: string;\n  label: string;\n}\nexport function main(options: Options): string {\n  return options.abstract;\n}\n",
        )
        .unwrap();
        assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
        assert!(unit.nodes.iter().any(|n| n.name == "main"));
    }
    #[test]
    fn abstract_named_methods_and_class_fields_keep_their_real_names() {
        let unit = parse(
            "config.ts",
            "class Config {\n  abstract = 1;\n  abstract(): void {}\n}\nfunction main(config: Config) {\n  config.abstract();\n}\n",
        )
        .unwrap();
        assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
        let method = unit
            .nodes
            .iter()
            .find(|n| n.name == "abstract")
            .expect("method named abstract must be extracted");
        assert_eq!(method.kind, "method");
        // Names, signatures and hashes must come from the original source, never
        // from the `abs_trac` parse mask.
        assert!(method.source.contains("abstract"), "{}", method.source);
        assert!(method.signature.contains("abstract("), "{}", method.signature);
        assert_eq!(
            method.content_hash,
            format!("{:x}", Sha256::digest(b"abstract(): void {}"))
        );
        assert!(method.symbol_key.contains("abstract"));
        assert!(unit.nodes.iter().any(|n| n.name == "main"));
        assert_eq!(unit.calls.len(), 1);
        assert_eq!(unit.calls[0].name, "abstract");
    }
    #[test]
    fn multiple_abstract_properties_in_one_file_parse_completely() {
        let unit = parse(
            "registry.ts",
            "interface Props {\n  abstract?: string;\n  abstract: number;\n}\ntype Config = {\n  abstract: boolean;\n  label: string;\n};\nclass Registry {\n  abstract = 1;\n  abstract(value: string): void {}\n}\nexport function read(props: Props): string {\n  return props.abstract ?? \"\";\n}\nexport function build(config: Config): Registry {\n  return new Registry();\n}\n",
        )
        .unwrap();
        assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
        for name in ["read", "build", "abstract"] {
            assert!(
                unit.nodes.iter().any(|n| n.name == name),
                "missing node {name}"
            );
        }
        assert_eq!(
            unit.nodes
                .iter()
                .find(|n| n.name == "abstract")
                .map(|n| n.class.as_deref()),
            Some(Some("Registry"))
        );
    }
    #[test]
    fn abstract_property_in_svelte_embedded_script_parses() {
        // The mask must be applied to the script-extracted source, after embedding.
        let unit = parse(
            "Widget.svelte",
            "<script lang=\"ts\">\ninterface Props {\n  abstract?: string;\n}\nexport function main(props: Props) {\n  return props.abstract;\n}\n</script>\n<h1>{props.abstract}</h1>\n",
        )
        .unwrap();
        assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
        let main = unit
            .nodes
            .iter()
            .find(|n| n.name == "main")
            .expect("main node must be extracted from the embedded script");
        assert_eq!(main.line, 5);
        assert!(main.source.contains("props.abstract"), "{}", main.source);
    }
    #[test]
    fn abstract_class_modifiers_still_parse_alongside_masked_properties() {
        let unit = parse(
            "base.ts",
            "abstract class Base {\n  abstract run(): void;\n  abstract readonly name: string;\n}\nclass Impl extends Base {\n  run(): void {}\n}\ninterface Options {\n  abstract?: string;\n}\nfunction use(impl: Impl, options: Options) {\n  impl.run();\n}\n",
        )
        .unwrap();
        // The masked retry must succeed, which also proves `abstract class` /
        // `abstract readonly` modifiers were not masked (masking them would break
        // the retry and leave the original `abstract?:` error in place).
        assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
        let run = unit
            .nodes
            .iter()
            .filter(|n| n.name == "run")
            .collect::<Vec<_>>();
        assert_eq!(run.len(), 1, "only the concrete method is a function node");
        assert_eq!(run[0].class.as_deref(), Some("Impl"));
        assert!(unit.nodes.iter().any(|n| n.name == "use"));
    }
    #[test]
    fn mask_replaces_only_property_position_abstract() {
        let source = "abstract class Base {\n  abstract run(): void;\n  abstract readonly name: string;\n  abstract?: string;\n}\n";
        let masked = mask_abstract_property_names(source).expect("property position must be masked");
        assert_eq!(masked.len(), source.len(), "offsets must be preserved");
        assert_eq!(masked.matches("abs_trac").count(), 1);
        assert!(masked.contains("abs_trac?: string"));
        assert!(masked.contains("abstract class Base"));
        assert!(masked.contains("abstract run(): void;"));
        assert!(masked.contains("abstract readonly name"));
        // No property position at all -> no pointless retry.
        assert!(
            mask_abstract_property_names("abstract class Base { abstract run(): void; }").is_none()
        );
        assert!(mask_abstract_property_names("const abstractValue = 1;").is_none());
        // Multi-byte neighbors must not be split and the result must stay valid UTF-8.
        let chinese = "// 中文注释\ninterface 配置 {\n  abstract?: string;\n}\n";
        let masked = mask_abstract_property_names(chinese).expect("must mask the property name");
        assert_eq!(masked.len(), chinese.len());
        assert_eq!(masked.matches("abs_trac").count(), 1);
    }
    #[test]
    fn malformed_source_diagnostic_has_one_based_line_and_column() {
        let unit = parse("a.ts", "const a = 1;\nconst broken = ;\n").unwrap();
        assert_eq!(unit.diagnostics.len(), 1, "{:?}", unit.diagnostics);
        let (line, column) = diagnostic_line_col(&unit.diagnostics[0], "a.ts");
        // The first ERROR node is on line 2, at the 1-based column of `=`.
        assert_eq!((line, column), (2, 14), "{}", unit.diagnostics[0]);
    }
    #[test]
    fn masked_retry_never_hides_a_genuinely_malformed_file() {
        let unit = parse(
            "broken.ts",
            "interface Options { abstract?: string }\nfunction broken( {\n",
        )
        .unwrap();
        assert_eq!(unit.diagnostics.len(), 1, "{:?}", unit.diagnostics);
        let (line, column) = diagnostic_line_col(&unit.diagnostics[0], "broken.ts");
        // The masked retry fixes the `abstract` false positive but must still report
        // the real syntax error (line 2), not the masked line 1 property.
        assert_eq!((line, column), (2, 1), "{}", unit.diagnostics[0]);
    }
    #[test]
    fn c_same_file_calls_resolve_and_external_calls_stay_unresolved() {
        let g = analyze(&[(
            "main.c",
            "static int add(int a, int b) { return a + b; }\nint main(void) {\n    add(1, 2);\n    external_call();\n    return 0;\n}\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        assert_eq!(g.edges.len(), 1, "{:?}", g.unresolved);
        assert_eq!(g.edges[0].resolution, "lexical-static");
        assert!(g.edges[0].to.ends_with(":add"));
        assert_eq!(g.unresolved.len(), 1);
        assert_eq!(g.unresolved[0].reason, "external-or-unknown-symbol");
        let add = g.nodes.iter().find(|n| n.name == "add").unwrap();
        assert_eq!(add.language, "C");
        assert_eq!(add.kind, "function");
        assert_eq!(add.parameters, "(int a, int b)");
        assert_eq!(add.return_type, "int");
        assert!(add.class.is_none());
    }
    #[test]
    fn c_quoted_include_and_sibling_definition_resolve_as_include_static() {
        let g = analyze(&[
            ("svc/service.h", "int helper(int value);\n"),
            (
                "svc/service.c",
                "#include \"service.h\"\nint helper(int value) { return value + 1; }\n",
            ),
            (
                "app/main.c",
                "#include \"../svc/service.h\"\nint main(void) { return helper(1); }\n",
            ),
        ]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        assert_eq!(g.edges.len(), 1, "{:?}", g.unresolved);
        assert_eq!(g.edges[0].resolution, "c-include-static");
        assert!(g.edges[0].to.ends_with(":helper"));
        assert!(g.unresolved.is_empty());
        // The header itself is not a symbol; only the sibling implementation is.
        assert_eq!(
            g.nodes
                .iter()
                .filter(|n| n.name == "helper" && n.kind == "function")
                .count(),
            1
        );
    }
    #[test]
    fn c_includes_track_only_quoted_local_paths() {
        let unit = parse(
            "src/main.c",
            "#include <stdio.h>\n#include \"service.h\"\n#include \"..\\shared/util.h\"\n",
        )
        .unwrap();
        assert_eq!(unit.includes, vec!["../shared/util.h", "service.h"]);
    }
    #[test]
    fn c_declarations_and_parameters_shadow_calls() {
        let g = analyze(&[(
            "shadow.c",
            "int helper(void) { return 1; }\nint via_parameter(int helper) { return helper(); }\nint via_local(void) { int helper = 0; return helper(); }\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.unresolved.len(), 2);
        assert!(
            g.unresolved
                .iter()
                .all(|call| call.reason == "shadowed-or-assigned-binding"),
            "{:?}",
            g.unresolved
        );
    }
    #[test]
    fn c_function_prototypes_do_not_shadow_calls() {
        let g = analyze(&[(
            "proto.c",
            "int helper(int value);\nint helper(int value) { return value; }\nint main(void) { return helper(1); }\n",
        )]);
        assert_eq!(g.edges.len(), 1, "{:?}", g.unresolved);
        assert_eq!(g.edges[0].resolution, "lexical-static");
        assert!(g.edges[0].to.ends_with(":helper"));
    }
    #[test]
    fn cpp_inline_methods_and_this_calls_resolve() {
        let g = analyze(&[(
            "base.cpp",
            "class Base {\npublic:\n    void run() { helper(); this->helper(); }\n    void helper() {}\n};\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        assert_eq!(g.edges.len(), 2, "{:?}", g.unresolved);
        assert!(
            g.edges
                .iter()
                .all(|edge| edge.resolution == "same-class-static-candidate")
        );
        let helper = g.resolve("helper").unwrap();
        assert!(g.edges.iter().all(|edge| edge.to == helper));
        let run = g.nodes.iter().find(|n| n.name == "run").unwrap();
        assert_eq!(run.kind, "method");
        assert_eq!(run.class.as_deref(), Some("Base"));
        assert_eq!(run.language, "C++");
        assert_eq!(run.parameters, "()");
    }
    #[test]
    fn cpp_out_of_line_methods_constructors_and_destructors_get_classes() {
        let g = analyze(&[(
            "base.cpp",
            "class Base {\npublic:\n    Base();\n    ~Base();\n    void run();\n    void helper();\n};\nvoid Base::run() { helper(); }\nvoid Base::helper() {}\nBase::Base() {}\nBase::~Base() {}\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        let run = g.nodes.iter().find(|n| n.name == "run").unwrap();
        assert_eq!(run.kind, "method");
        assert_eq!(run.class.as_deref(), Some("Base"));
        let helper = g.nodes.iter().find(|n| n.name == "helper").unwrap();
        assert_eq!(helper.class.as_deref(), Some("Base"));
        let constructor = g.nodes.iter().find(|n| n.name == "Base").unwrap();
        assert_eq!(constructor.kind, "method");
        assert_eq!(constructor.class.as_deref(), Some("Base"));
        let destructor = g.nodes.iter().find(|n| n.name == "~Base").unwrap();
        assert_eq!(destructor.kind, "method");
        assert_eq!(destructor.class.as_deref(), Some("Base"));
        assert_eq!(g.edges.len(), 1, "{:?}", g.unresolved);
        assert_eq!(g.edges[0].resolution, "same-class-static-candidate");
        assert_eq!(g.edges[0].to, helper.id);
    }
    #[test]
    fn cpp_namespace_functions_and_templates_are_extracted() {
        let g = analyze(&[(
            "app.cpp",
            "namespace app {\nint freeFunction(int x) { return x; }\nvoid caller() { freeFunction(1); }\ntemplate <typename T>\nT identity(T value) { return value; }\nvoid useTemplate() { identity(2); }\n}\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        let free = g.nodes.iter().find(|n| n.name == "freeFunction").unwrap();
        assert!(free.class.is_none());
        assert_eq!(free.parameters, "(int x)");
        assert_eq!(free.return_type, "int");
        let template = g.nodes.iter().find(|n| n.name == "identity").unwrap();
        assert_eq!(template.parameters, "(T value)");
        assert_eq!(template.return_type, "T");
        assert_eq!(g.edges.len(), 2, "{:?}", g.unresolved);
        assert!(
            g.edges
                .iter()
                .all(|edge| edge.resolution == "lexical-static")
        );
    }
    #[test]
    fn cpp_class_static_candidate_and_member_calls_fail_closed() {
        let g = analyze(&[(
            "base.cpp",
            "class Base {\npublic:\n    static int create();\n    void run();\n};\nint Base::create() { return 1; }\nvoid Base::run() {}\nvoid caller() {\n    Base::create();\n    Base b;\n    b.run();\n}\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        assert_eq!(g.edges.len(), 1, "{:?}", g.unresolved);
        assert_eq!(g.edges[0].resolution, "cpp-class-static-candidate");
        assert!(g.edges[0].to.ends_with(":create"));
        assert_eq!(g.unresolved.len(), 1);
        assert_eq!(g.unresolved[0].reason, "dynamic-or-unknown-receiver");
    }
    #[test]
    fn cpp_class_static_candidate_resolves_through_included_header() {
        let g = analyze(&[
            (
                "include/base.hpp",
                "class Base {\npublic:\n    static int create() { return 1; }\n};\n",
            ),
            (
                "src/main.cpp",
                "#include \"../include/base.hpp\"\nint main() { return Base::create(); }\n",
            ),
        ]);
        assert_eq!(g.edges.len(), 1, "{:?}", g.unresolved);
        assert_eq!(g.edges[0].resolution, "cpp-class-static-candidate");
        assert!(g.edges[0].to.ends_with(":create"));
    }
    #[test]
    fn cpp_overload_ambiguity_is_unresolved() {
        let g = analyze(&[(
            "over.cpp",
            "class Calc {\npublic:\n    int over(int x);\n    int over(double x);\n};\nint Calc::over(int x) { return x; }\nint Calc::over(double x) { return 1; }\nvoid caller() { Calc::over(1); }\n",
        )]);
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.unresolved.len(), 1);
        assert_eq!(g.unresolved[0].reason, "dynamic-or-unknown-receiver");
    }
    #[test]
    fn cpp_lambdas_become_anonymous_callbacks() {
        let g = analyze(&[(
            "lambda.cpp",
            "void use() {\n    auto fn = [](int x) { return x + 1; };\n    fn(2);\n}\n",
        )]);
        assert_eq!(g.diagnostics.len(), 0, "{:?}", g.diagnostics);
        let callback = g
            .nodes
            .iter()
            .find(|n| n.name.starts_with("<callback@"))
            .expect("lambda must become an anonymous callback node");
        assert_eq!(callback.parameters, "(int x)");
        assert_eq!(callback.language, "C++");
        assert_eq!(g.unresolved.len(), 1);
        assert_eq!(g.unresolved[0].reason, "shadowed-or-assigned-binding");
    }
    #[test]
    fn headers_parse_as_cpp_with_c_fallback() {
        let cpp_header = parse(
            "widget.h",
            "class Base {\npublic:\n    void run() {}\n};\n",
        )
        .unwrap();
        assert!(cpp_header.diagnostics.is_empty(), "{:?}", cpp_header.diagnostics);
        let run = cpp_header
            .nodes
            .iter()
            .find(|n| n.name == "run")
            .unwrap();
        assert_eq!(run.class.as_deref(), Some("Base"));
        assert_eq!(run.language, "C++");
        // K&R definitions are valid C but not C++, so the header retry must switch.
        let c_header = parse("legacy.h", "int knr(a)\n    int a;\n{ return a; }\n").unwrap();
        assert!(c_header.diagnostics.is_empty(), "{:?}", c_header.diagnostics);
        let knr = c_header.nodes.iter().find(|n| n.name == "knr").unwrap();
        assert_eq!(knr.language, "C");
        // A plain C header parses without a diagnostic too.
        let plain = parse("util.h", "#include \"common.h\"\nint helper(int value);\n").unwrap();
        assert!(plain.diagnostics.is_empty(), "{:?}", plain.diagnostics);
        assert_eq!(plain.includes, vec!["common.h"]);
    }
}
