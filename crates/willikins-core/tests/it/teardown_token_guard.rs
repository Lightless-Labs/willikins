//! Acceptance test 2, the `TeardownToken` half (milestone 3m task R2,
//! trust boundary 2 of
//! `docs/plans/2026-10-05-milestone-3m-safe-rollback.md`): every non-test
//! call site of `TeardownToken::new` in the workspace is inside
//! `willikins_core::rollback`'s `apply_teardown` function **by enclosing
//! function**, not merely "somewhere in `rollback.rs`" — a second
//! production call site elsewhere in that file, or inside a differently
//! named helper it calls, would pass a file-level check and still be a
//! hole the type system no longer closes.
//!
//! Same `syn`-AST technique as `sink_token_guard.rs` (so an alias import
//! or a fully qualified path cannot hide a call from a text search, and a
//! macro invocation's opaque token stream is scanned as raw tokens), and
//! the same per-function exemption shape `expose_secret_guard.rs` already
//! uses for `Credential::authorize`/`authorize_header`
//! (`Exemption::OneFunction`), rather than `sink_token_guard.rs`'s
//! coarser "exempt this whole module" rule: `TeardownToken::new` needs
//! the finer one, because its one legitimate call site shares a file with
//! `Reversal`, `Within` and every other declaration type R1 already put
//! in `rollback.rs`, most of which are plain data and never execute.
//!
//! `apply_teardown` itself is milestone 3m task R6, not yet written: this
//! guard's enclosing-function rule is live today over zero legitimate
//! call sites ([`EXPECTED_SITES`]). R6 flips the pin to `1` in the same
//! commit that adds its own call and its own
//! `#[allow(clippy::disallowed_methods)]` — forgetting either fails this
//! test, the same ratchet idiom as `test_layout_guard::PENDING` and
//! `ratchet.rs`'s counts. See the plan's 2026-10-12 addendum (R2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, Ident, ItemUse, UseTree};

/// The number of legitimate non-test `TeardownToken::new` call sites this
/// workspace holds today. Zero, because `apply_teardown` (task R6) does
/// not exist yet. Bump to `1` — never higher — in the commit that adds
/// it; a second real site anywhere is exactly what this guard exists to
/// catch.
const EXPECTED_SITES: usize = 0;

/// `rollback.rs`'s path, relative to a crate's `src/` directory.
fn rollback_rs_relative() -> PathBuf {
    PathBuf::from("rollback.rs")
}

/// The one function name allowed to call `TeardownToken::new`, inside
/// `rollback.rs`.
const EXEMPT_FN_NAMES: &[&str] = &["apply_teardown"];

/// The workspace's `crates/` directory, from this crate's own manifest
/// directory.
fn crates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/willikins-core has a parent directory")
        .to_path_buf()
}

/// Every crate directory under `crates/`.
fn crate_dirs() -> Vec<PathBuf> {
    std::fs::read_dir(crates_root())
        .expect("crates/ is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

/// One call (or bare reference) to `TeardownToken::new` found outside a
/// test context, with the name of its innermost enclosing function or
/// method (`None` at module scope — a `static`/`const` initializer).
#[derive(Debug)]
struct Violation {
    file: PathBuf,
    enclosing_fn: Option<String>,
    context: String,
}

/// Whether any of `attrs` is `#[cfg(test)]` (exactly; a broader
/// `cfg(any(test, ...))` is not attempted, since nothing in this
/// workspace uses one — the same rule `sink_token_guard.rs` and
/// `expose_secret_guard.rs` use).
fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

/// Whether `attrs` carries exactly `#[allow(clippy::disallowed_methods)]`.
fn has_disallowed_methods_allow(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("allow") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            let segments: Vec<String> = meta
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if segments == ["clippy", "disallowed_methods"] {
                found = true;
            }
            Ok(())
        });
        found
    })
}

/// Every name `TeardownToken` is imported under in `file`, plus the bare
/// name itself.
fn collect_teardown_token_aliases(file: &syn::File) -> HashSet<String> {
    struct AliasCollector {
        aliases: HashSet<String>,
    }

    fn walk_use_tree(tree: &UseTree, aliases: &mut HashSet<String>) {
        match tree {
            UseTree::Path(p) => walk_use_tree(&p.tree, aliases),
            UseTree::Name(n) => {
                if n.ident == "TeardownToken" {
                    aliases.insert("TeardownToken".to_string());
                }
            }
            UseTree::Rename(r) => {
                if r.ident == "TeardownToken" {
                    aliases.insert(r.rename.to_string());
                }
            }
            UseTree::Group(g) => {
                for item in &g.items {
                    walk_use_tree(item, aliases);
                }
            }
            UseTree::Glob(_) => {}
        }
    }

    impl<'ast> Visit<'ast> for AliasCollector {
        fn visit_item_use(&mut self, node: &'ast ItemUse) {
            walk_use_tree(&node.tree, &mut self.aliases);
        }
    }

    let mut collector = AliasCollector {
        aliases: HashSet::from(["TeardownToken".to_string()]),
    };
    collector.visit_file(file);
    collector.aliases
}

/// Flatten `stream` into its leaf tokens, recursing into every
/// [`proc_macro2::Group`] — a macro invocation's body is opaque to
/// `syn`'s AST, so a call written inside one (`vec![TeardownToken::new()]`)
/// is invisible to [`FileWalker`]'s visitor unless this scans raw tokens.
fn flatten_tokens(stream: proc_macro2::TokenStream, out: &mut Vec<proc_macro2::TokenTree>) {
    for tt in stream {
        if let proc_macro2::TokenTree::Group(group) = &tt {
            flatten_tokens(group.stream(), out);
        } else {
            out.push(tt);
        }
    }
}

/// Every `<alias>::new` occurrence found in `tokens`, for any `alias` in
/// `aliases`.
fn scan_tokens_for_teardown_token_new(
    tokens: proc_macro2::TokenStream,
    aliases: &HashSet<String>,
) -> Vec<String> {
    let mut flat = Vec::new();
    flatten_tokens(tokens, &mut flat);
    let mut hits = Vec::new();
    let mut i = 0;
    while i + 3 < flat.len() {
        if let (
            proc_macro2::TokenTree::Ident(owner),
            proc_macro2::TokenTree::Punct(colon1),
            proc_macro2::TokenTree::Punct(colon2),
            proc_macro2::TokenTree::Ident(new_ident),
        ) = (&flat[i], &flat[i + 1], &flat[i + 2], &flat[i + 3])
            && colon1.as_char() == ':'
            && colon2.as_char() == ':'
            && new_ident == "new"
            && aliases.contains(&owner.to_string())
        {
            hits.push(format!("{owner}::{new_ident}"));
        }
        i += 1;
    }
    hits
}

/// Whether `path` (optionally qualified via `qself`) names
/// `<alias>::new` for some `alias` in `aliases`.
fn path_is_teardown_token_new(
    qself: Option<&syn::QSelf>,
    path: &syn::Path,
    aliases: &HashSet<String>,
) -> bool {
    let segments: Vec<&Ident> = path.segments.iter().map(|s| &s.ident).collect();
    let Some(last) = segments.last() else {
        return false;
    };
    if *last != "new" {
        return false;
    }
    if segments.len() >= 2 {
        let owner = segments[segments.len() - 2];
        if aliases.contains(&owner.to_string()) {
            return true;
        }
    }
    if let Some(qself) = qself
        && let syn::Type::Path(type_path) = &*qself.ty
        && let Some(last_ty_segment) = type_path.path.segments.last()
        && aliases.contains(&last_ty_segment.ident.to_string())
    {
        return true;
    }
    false
}

/// A file's exemption: the function names (by bare identifier) allowed
/// to call `TeardownToken::new` inside it. Empty for every file except
/// `rollback.rs`.
type ExemptFns<'a> = &'a [&'a str];

/// Walks one already-parsed file's items, threading `test_only` and
/// `in_exempt_fn` through every nested item, and the name of the
/// innermost enclosing function/method through a stack, so a violation
/// can report which function it was found in.
struct FileWalker<'a> {
    aliases: &'a HashSet<String>,
    exempt_fns: ExemptFns<'a>,
    test_only: bool,
    fn_stack: Vec<String>,
    violations: Vec<(String, Option<String>)>,
    allow_attrs: usize,
    pending_mods: Vec<(String, bool)>,
}

impl<'a> FileWalker<'a> {
    fn new(aliases: &'a HashSet<String>, exempt_fns: ExemptFns<'a>) -> Self {
        Self {
            aliases,
            exempt_fns,
            test_only: false,
            fn_stack: Vec::new(),
            violations: Vec::new(),
            allow_attrs: 0,
            pending_mods: Vec::new(),
        }
    }

    fn in_exempt_fn(&self) -> bool {
        match self.fn_stack.last() {
            Some(name) => self.exempt_fns.contains(&name.as_str()),
            None => false,
        }
    }
}

impl<'ast> Visit<'ast> for FileWalker<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        if node.content.is_none() {
            self.pending_mods
                .push((node.ident.to_string(), self.test_only));
        }
        syn::visit::visit_item_mod(self, node);
        self.test_only = was;
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let was_test = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        if !self.test_only && has_disallowed_methods_allow(&node.attrs) {
            self.allow_attrs += 1;
        }
        self.fn_stack.push(node.sig.ident.to_string());
        syn::visit::visit_item_fn(self, node);
        self.fn_stack.pop();
        self.test_only = was_test;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let was_test = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        if !self.test_only && has_disallowed_methods_allow(&node.attrs) {
            self.allow_attrs += 1;
        }
        self.fn_stack.push(node.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, node);
        self.fn_stack.pop();
        self.test_only = was_test;
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        syn::visit::visit_item_impl(self, node);
        self.test_only = was;
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if !self.test_only && has_disallowed_methods_allow(&node.attrs) {
            self.allow_attrs += 1;
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        syn::visit::visit_item_static(self, node);
        self.test_only = was;
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        syn::visit::visit_item_const(self, node);
        self.test_only = was;
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if !self.test_only
            && !self.in_exempt_fn()
            && path_is_teardown_token_new(node.qself.as_ref(), &node.path, self.aliases)
        {
            let rendered = quote::quote!(#node).to_string();
            self.violations
                .push((rendered, self.fn_stack.last().cloned()));
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if !self.test_only && !self.in_exempt_fn() {
            for hit in scan_tokens_for_teardown_token_new(node.tokens.clone(), self.aliases) {
                self.violations.push((
                    format!("(inside a macro invocation) {hit}"),
                    self.fn_stack.last().cloned(),
                ));
            }
        }
        syn::visit::visit_macro(self, node);
    }
}

/// Resolve `mod_name`, declared in `parent_file`, to the file it names.
fn resolve_mod_file(parent_file: &Path, mod_name: &str) -> PathBuf {
    let parent_dir = parent_file
        .parent()
        .expect("every source file has a parent directory");
    let stem = parent_file
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("source file has a UTF-8 stem");
    let base_dir = if matches!(stem, "lib" | "main" | "mod") {
        parent_dir.to_path_buf()
    } else {
        parent_dir.join(stem)
    };
    let flat = base_dir.join(format!("{mod_name}.rs"));
    if flat.exists() {
        return flat;
    }
    let nested = base_dir.join(mod_name).join("mod.rs");
    if nested.exists() {
        return nested;
    }
    panic!(
        "could not resolve `mod {mod_name};` declared in {}: tried {} and {}",
        parent_file.display(),
        flat.display(),
        nested.display()
    );
}

/// Walk one crate's `src/` module graph starting from `root`, treating
/// `exempt_relative_path`'s file as the one allowed to hold
/// `exempt_fns`-named functions that call `TeardownToken::new`.
/// Returns every visited file, and (through `allow_attrs_out`) the total
/// count of non-test `#[allow(clippy::disallowed_methods)]` attributes
/// found on the exempt file alone.
fn walk_crate(
    root: &Path,
    exempt_relative_path: &Path,
    exempt_fns: ExemptFns<'_>,
    violations: &mut Vec<Violation>,
    allow_attrs_out: &mut usize,
) -> HashSet<PathBuf> {
    let mut queue: Vec<(PathBuf, bool)> = vec![(root.to_path_buf(), false)];
    let mut visited: HashSet<PathBuf> = HashSet::new();

    while let Some((path, inherited_test_only)) = queue.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let is_exempt_file = path.ends_with(exempt_relative_path);
        let fns: ExemptFns<'_> = if is_exempt_file { exempt_fns } else { &[] };

        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        let file = syn::parse_file(&source)
            .unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));

        let aliases = collect_teardown_token_aliases(&file);
        let mut walker = FileWalker::new(&aliases, fns);
        walker.test_only = inherited_test_only;
        for item in &file.items {
            walker.visit_item(item);
        }

        for (context, enclosing_fn) in walker.violations {
            violations.push(Violation {
                file: path.clone(),
                enclosing_fn,
                context,
            });
        }
        if is_exempt_file {
            *allow_attrs_out += walker.allow_attrs;
        }

        for (mod_name, test_only) in walker.pending_mods {
            let child = resolve_mod_file(&path, &mod_name);
            queue.push((child, test_only));
        }
    }
    visited
}

/// Walk one file the module-graph walk never reached, as ordinary
/// (non-test) code.
fn walk_single_file(
    path: &Path,
    exempt_relative_path: &Path,
    exempt_fns: ExemptFns<'_>,
    violations: &mut Vec<Violation>,
) {
    let is_exempt_file = path.ends_with(exempt_relative_path);
    let fns: ExemptFns<'_> = if is_exempt_file { exempt_fns } else { &[] };
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    let file =
        syn::parse_file(&source).unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));
    let aliases = collect_teardown_token_aliases(&file);
    let mut walker = FileWalker::new(&aliases, fns);
    for item in &file.items {
        walker.visit_item(item);
    }
    for (context, enclosing_fn) in walker.violations {
        violations.push(Violation {
            file: path.to_path_buf(),
            enclosing_fn,
            context,
        });
    }
}

/// Every `.rs` file under `dir`, recursively, skipping `target/` and
/// `tests/`.
fn all_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()));
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == "tests" {
                continue;
            }
            all_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Every non-test `TeardownToken::new` call site in the workspace must be
/// inside `willikins-core/src/rollback.rs`'s `apply_teardown` function —
/// no other file, and no other function in that file either.
#[test]
fn every_teardown_token_new_call_site_is_apply_teardown_a_test_item_or_a_tests_file() {
    let mut violations = Vec::new();
    let mut allow_attrs = 0;
    let mut reached_rollback_rs = false;

    for crate_dir in crate_dirs() {
        let src = crate_dir.join("src");
        if !src.exists() {
            continue;
        }
        let root = if src.join("lib.rs").exists() {
            src.join("lib.rs")
        } else if src.join("main.rs").exists() {
            src.join("main.rs")
        } else {
            continue;
        };
        let is_core = crate_dir.file_name().and_then(|n| n.to_str()) == Some("willikins-core");
        let exempt_path = if is_core {
            rollback_rs_relative()
        } else {
            PathBuf::from("__no_exempt_module__")
        };
        let visited = walk_crate(
            &root,
            &exempt_path,
            EXEMPT_FN_NAMES,
            &mut violations,
            &mut allow_attrs,
        );
        if is_core && visited.iter().any(|p| p.ends_with("rollback.rs")) {
            reached_rollback_rs = true;
        }

        let mut every_file = Vec::new();
        all_rs_files(&crate_dir, &mut every_file);
        for path in every_file {
            if visited.contains(&path) {
                continue;
            }
            walk_single_file(&path, &exempt_path, EXEMPT_FN_NAMES, &mut violations);
        }
    }

    assert!(
        reached_rollback_rs,
        "this walk never reached willikins-core/src/rollback.rs: the exemption rule above is \
         vacuous for the real tree (update this test if rollback.rs moved or stopped being \
         declared from lib.rs's module graph)"
    );

    assert!(
        violations.is_empty(),
        "found TeardownToken::new call site(s) outside rollback.rs's apply_teardown and \
         outside any #[cfg(test)] item:\n{}",
        violations
            .iter()
            .map(|v| format!(
                "  {} (in {}) :: {}",
                v.file.display(),
                v.enclosing_fn.as_deref().unwrap_or("<module scope>"),
                v.context
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    assert_eq!(
        allow_attrs, EXPECTED_SITES,
        "rollback.rs's non-test #[allow(clippy::disallowed_methods)] count ({allow_attrs}) \
         must equal EXPECTED_SITES ({EXPECTED_SITES}); update both together when apply_teardown \
         (task R6) adds its real call"
    );
}

/// The call-site count alone, with no placement exemption at all: this
/// isolates `EXPECTED_SITES` from the placement rule the test above
/// checks, exactly as `sink_token_guard.rs`'s
/// `the_executor_has_exactly_one_sink_token_new_call_site` isolates its
/// own pin.
#[test]
fn rollback_rs_has_exactly_the_pinned_count_of_teardown_token_new_call_sites() {
    let path = crates_root().join("willikins-core/src/rollback.rs");
    let mut violations = Vec::new();
    let mut allow_attrs = 0;
    let _ = walk_crate(
        &path,
        Path::new("__no_exempt_module__"),
        &[],
        &mut violations,
        &mut allow_attrs,
    );
    assert_eq!(
        violations.len(),
        EXPECTED_SITES,
        "expected exactly EXPECTED_SITES ({EXPECTED_SITES}) non-test TeardownToken::new call \
         site(s) in rollback.rs: {violations:?}"
    );
}

// -------------------------------------------------------------
// `FileWalker` unit tests: proof the guard actually catches something,
// independent of the real tree (which holds zero call sites today, so an
// always-empty result would look identical to a working guard).
// -------------------------------------------------------------

fn violations_in(
    source: &str,
    exempt_fns: ExemptFns<'_>,
    test_only: bool,
) -> Vec<(String, Option<String>)> {
    let file =
        syn::parse_file(source).unwrap_or_else(|err| panic!("{source:?} failed to parse: {err}"));
    let aliases = collect_teardown_token_aliases(&file);
    let mut walker = FileWalker::new(&aliases, exempt_fns);
    walker.test_only = test_only;
    for item in &file.items {
        walker.visit_item(item);
    }
    walker.violations
}

fn allow_attrs_in(source: &str) -> usize {
    let file =
        syn::parse_file(source).unwrap_or_else(|err| panic!("{source:?} failed to parse: {err}"));
    let aliases = collect_teardown_token_aliases(&file);
    let mut walker = FileWalker::new(&aliases, &[]);
    for item in &file.items {
        walker.visit_item(item);
    }
    walker.allow_attrs
}

#[test]
fn walker_catches_a_call_in_an_unexempt_function() {
    let violations = violations_in("fn other() { TeardownToken::new(); }", &[], false);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].1.as_deref(), Some("other"));
}

#[test]
fn walker_exempts_a_call_in_the_named_function_only() {
    let violations = violations_in(
        "fn apply_teardown() { TeardownToken::new(); }",
        &["apply_teardown"],
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_still_catches_a_call_in_a_differently_named_function_under_the_same_exemption() {
    let violations = violations_in(
        "fn apply_teardown() {} fn helper() { TeardownToken::new(); }",
        &["apply_teardown"],
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].1.as_deref(), Some("helper"));
}

#[test]
fn walker_exempts_a_call_inside_a_cfg_test_function_regardless_of_name() {
    let violations = violations_in(
        "#[cfg(test)] fn other() { TeardownToken::new(); }",
        &[],
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_catches_a_call_through_a_renamed_import() {
    let violations = violations_in(
        "use willikins_types::TeardownToken as T; fn f() { T::new(); }",
        &[],
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_catches_a_fully_qualified_call() {
    let violations = violations_in("fn f() { <TeardownToken>::new(); }", &[], false);
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_treats_the_whole_file_as_test_only_when_told_to() {
    let violations = violations_in("fn f() { TeardownToken::new(); }", &[], true);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_catches_a_call_inside_a_macro_invocation() {
    let violations = violations_in("fn f() { let _ = vec![TeardownToken::new()]; }", &[], false);
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_exempts_a_macro_invocation_inside_the_named_function() {
    let violations = violations_in(
        "fn apply_teardown() { let _ = vec![TeardownToken::new()]; }",
        &["apply_teardown"],
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_counts_the_disallowed_methods_allow_attribute_on_a_let() {
    assert_eq!(
        allow_attrs_in(
            "fn f() { #[allow(clippy::disallowed_methods)] let t = TeardownToken::new(); }"
        ),
        1
    );
}

#[test]
fn walker_counts_the_disallowed_methods_allow_attribute_on_a_function() {
    assert_eq!(
        allow_attrs_in("#[allow(clippy::disallowed_methods)] fn f() {}"),
        1
    );
}

#[test]
fn walker_does_not_count_an_unrelated_allow_attribute() {
    assert_eq!(allow_attrs_in("#[allow(dead_code)] fn f() {}"), 0);
}

#[test]
fn walker_does_not_count_the_allow_attribute_inside_cfg_test() {
    assert_eq!(
        allow_attrs_in(
            "#[cfg(test)] fn f() { #[allow(clippy::disallowed_methods)] let t = TeardownToken::new(); }"
        ),
        0
    );
}
