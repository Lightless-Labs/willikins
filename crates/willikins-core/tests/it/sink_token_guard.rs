//! Acceptance test 4, `SinkToken` half: every call site of
//! `SinkToken::new` in the workspace is either the one inside
//! `willikins_core::apply`, or inside a `#[cfg(test)]` item, or inside a
//! file under a `tests/` directory.
//!
//! Parses every crate's `src/` tree with `syn` (rather than grepping) so
//! an alias import (`use SinkToken as Foo`) or a fully qualified path
//! (`<SinkToken>::new()`) cannot hide a call from a text search. A file
//! under `tests/` is exempt outright without being parsed at all (a
//! `trybuild` fixture need not even be valid Rust); everything else is
//! walked as part of its crate's module graph starting from `lib.rs` or
//! `main.rs`, so a module declared `#[cfg(test)] mod probe;` in a parent
//! file is correctly treated as test-only even though nothing inside its
//! own file (`probe.rs`) carries a `#[cfg(test)]` attribute itself —
//! `willikins-types::probe` is exactly this case today.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, Ident, ItemUse, UseTree};

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

/// One call (or bare reference) to `SinkToken::new` found outside a test
/// context.
#[derive(Debug)]
struct Violation {
    file: PathBuf,
    context: String,
}

/// Whether any of `attrs` is `#[cfg(test)]` (exactly; a broader `cfg(any(test, ...))`
/// is not attempted, since nothing in this workspace uses one).
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

/// Every name `SinkToken` is imported under in `file`, found by walking
/// every `use` item anywhere in the file (top level, inside a function
/// body, inside an impl — anywhere `syn` can see one), regardless of
/// `cfg(test)` gating: an alias is collected wherever it is declared, so a
/// call site earlier in the file that happens to use an alias declared
/// later cannot escape detection.
fn collect_sink_token_aliases(file: &syn::File) -> HashSet<String> {
    struct AliasCollector {
        aliases: HashSet<String>,
    }

    fn walk_use_tree(tree: &UseTree, aliases: &mut HashSet<String>) {
        match tree {
            UseTree::Path(p) => walk_use_tree(&p.tree, aliases),
            UseTree::Name(n) => {
                if n.ident == "SinkToken" {
                    aliases.insert("SinkToken".to_string());
                }
            }
            UseTree::Rename(r) => {
                if r.ident == "SinkToken" {
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
        aliases: HashSet::from(["SinkToken".to_string()]),
    };
    collector.visit_file(file);
    collector.aliases
}

/// Flatten `stream` into its leaf tokens (idents and puncts), recursing
/// into every [`proc_macro2::Group`] rather than stopping at it: `syn`
/// does not parse a macro invocation's body as Rust syntax at all (it is
/// an opaque [`proc_macro2::TokenStream`] on [`syn::Macro::tokens`]), so a
/// call to `SinkToken::new()` written inside one (`vec![SinkToken::new()]`,
/// `assert_eq!(x, SinkToken::new())`) is invisible to [`FileWalker`]'s
/// `syn`-AST-only visitor unless this file scans the raw tokens itself.
fn flatten_tokens(stream: proc_macro2::TokenStream, out: &mut Vec<proc_macro2::TokenTree>) {
    for tt in stream {
        if let proc_macro2::TokenTree::Group(group) = &tt {
            flatten_tokens(group.stream(), out);
        } else {
            out.push(tt);
        }
    }
}

/// Every `<alias>::new` occurrence found in `tokens` (a macro invocation's
/// body), as a rendered `"<alias>::new"` string, for any `alias` in
/// `aliases`.
fn scan_tokens_for_sink_token_new(
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

/// Whether `path` (optionally qualified via `qself`, the `<Type>::` of a
/// fully qualified call) names `<alias>::new` for some `alias` in
/// `aliases`.
fn path_is_sink_token_new(
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
    // `<SinkToken>::new()` / `<SinkToken as Trait>::new()`: the type
    // lives in `qself`, and `path` is then just `new` (or
    // `Trait::new`, in which case the owner-segment check above would
    // not have fired).
    if let Some(qself) = qself
        && let syn::Type::Path(type_path) = &*qself.ty
        && let Some(last_ty_segment) = type_path.path.segments.last()
        && aliases.contains(&last_ty_segment.ident.to_string())
    {
        return true;
    }
    false
}

/// Walks one already-parsed file's items, threading a `test_only` flag
/// through every nested item (an inline `mod`, a `fn`, an `impl`, an
/// `impl`'s own methods) so a `#[cfg(test)] mod tests { ... }` block deep
/// inside an otherwise-live file is recognised, and collects every
/// external `mod name;` declaration found along the way (with the
/// `test_only` status it should inherit) for the caller to resolve to a
/// file and walk next.
struct FileWalker<'a> {
    aliases: &'a HashSet<String>,
    test_only: bool,
    violations: Vec<String>,
    pending_mods: Vec<(String, bool)>,
}

impl<'a> FileWalker<'a> {
    fn new(aliases: &'a HashSet<String>) -> Self {
        Self {
            aliases,
            test_only: false,
            violations: Vec::new(),
            pending_mods: Vec::new(),
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
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        syn::visit::visit_item_fn(self, node);
        self.test_only = was;
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        syn::visit::visit_item_impl(self, node);
        self.test_only = was;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let was = self.test_only;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        syn::visit::visit_impl_item_fn(self, node);
        self.test_only = was;
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
        if !self.test_only && path_is_sink_token_new(node.qself.as_ref(), &node.path, self.aliases)
        {
            let rendered = quote::quote!(#node).to_string();
            self.violations.push(rendered);
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if !self.test_only {
            for hit in scan_tokens_for_sink_token_new(node.tokens.clone(), self.aliases) {
                self.violations
                    .push(format!("(inside a macro invocation) {hit}"));
            }
        }
        syn::visit::visit_macro(self, node);
    }
}

/// Resolve `mod_name`, declared in `parent_file`, to the file it names —
/// `<dir>/<mod_name>.rs` or `<dir>/<mod_name>/mod.rs`, where `<dir>` is
/// `parent_file`'s own directory when `parent_file` is itself `lib.rs`,
/// `main.rs`, or `mod.rs`, and `<dir>/<parent-stem>/` otherwise (the
/// `tool.rs` + `tool/helpers.rs` layout this workspace uses).
///
/// # Panics
///
/// Panics if neither candidate file exists: a bug in this resolver, or a
/// `#[path = "..."]` attribute this test does not yet handle (none exist
/// in this workspace today — grepped for on introduction).
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

/// Walk every file in one crate's `src/` module graph starting from
/// `root` (`lib.rs` or `main.rs`), collecting every non-test call site of
/// `SinkToken::new` under `crate_name::exempt_module_path` treated as the
/// one allowed executor site.
fn walk_crate(
    root: &Path,
    exempt_relative_path: &Path,
    violations: &mut Vec<Violation>,
) -> HashSet<PathBuf> {
    let mut queue: Vec<(PathBuf, bool)> = vec![(root.to_path_buf(), false)];
    let mut visited: HashSet<PathBuf> = HashSet::new();

    while let Some((path, inherited_test_only)) = queue.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let is_exempt_module = path.ends_with(exempt_relative_path);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        let file = syn::parse_file(&source)
            .unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));

        let aliases = collect_sink_token_aliases(&file);
        let mut walker = FileWalker::new(&aliases);
        walker.test_only = inherited_test_only;
        for item in &file.items {
            walker.visit_item(item);
        }

        if !is_exempt_module {
            for context in walker.violations {
                violations.push(Violation {
                    file: path.clone(),
                    context,
                });
            }
        }

        for (mod_name, test_only) in walker.pending_mods {
            let child = resolve_mod_file(&path, &mod_name);
            queue.push((child, test_only));
        }
    }
    visited
}

/// Walk one file the module-graph walk never reached, as ordinary
/// (non-test) code: a `build.rs`, a `src/bin/` target, a file reached only
/// through `include!`, a bench, an example. The plan's acceptance test 4
/// says "every `.rs` file in the workspace", and cargo compiles all of
/// these even though no `mod` declaration mentions them.
fn walk_single_file(path: &Path, exempt_relative_path: &Path, violations: &mut Vec<Violation>) {
    if path.ends_with(exempt_relative_path) {
        return;
    }
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    let file =
        syn::parse_file(&source).unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));
    let aliases = collect_sink_token_aliases(&file);
    let mut walker = FileWalker::new(&aliases);
    for item in &file.items {
        walker.visit_item(item);
    }
    for context in walker.violations {
        violations.push(Violation {
            file: path.to_path_buf(),
            context,
        });
    }
}

/// Every `.rs` file under `dir`, recursively, skipping `target/` (build
/// artefacts, not source) and `tests/` (exempt outright, and a `trybuild`
/// fixture under it need not even parse).
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

#[test]
fn every_sink_token_new_call_site_is_the_executor_a_test_item_or_a_tests_file() {
    let mut violations = Vec::new();

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
        // The one allowed non-test site: `willikins-core`'s own `apply`
        // module (`src/apply.rs`, its `apply` function).
        let exempt = if crate_dir.file_name().and_then(|n| n.to_str()) == Some("willikins-core") {
            PathBuf::from("apply.rs")
        } else {
            // No other crate has any legitimate call site; an empty,
            // never-matching relative path keeps `walk_crate`'s logic
            // uniform.
            PathBuf::from("__no_exempt_module__")
        };
        let visited = walk_crate(&root, &exempt, &mut violations);

        // Second pass: every `.rs` file cargo compiles that no `mod`
        // declaration mentions.
        let mut every_file = Vec::new();
        all_rs_files(&crate_dir, &mut every_file);
        for path in every_file {
            if visited.contains(&path) {
                continue;
            }
            walk_single_file(&path, &exempt, &mut violations);
        }
    }

    assert!(
        violations.is_empty(),
        "found SinkToken::new call site(s) outside the executor and outside any \
         #[cfg(test)] item:\n{}",
        violations
            .iter()
            .map(|v| format!("  {} :: {}", v.file.display(), v.context))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A file under any crate's `tests/` directory is exempt from the guard
/// above outright (never even walked into it), matching acceptance test
/// 4's own wording. This test proves that exemption is not vacuous: at
/// least one `tests/` file in the workspace really does call
/// `SinkToken::new` today (several fake-tool test modules do, and this
/// crate's own `tests/apply.rs`-adjacent helpers do too), so the crate
/// still compiles and this whole suite still runs — which it would not if
/// the exemption were actually enforced against `tests/` files.
#[test]
fn a_tests_directory_file_calling_sink_token_new_is_not_itself_a_failure() {
    let path = crates_root().join("willikins-cli/tests/acceptance.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(
        source.contains("SinkToken::new()"),
        "expected {} (a file under a `tests/` directory, never walked by \
         `walk_crate` at all) to still call SinkToken::new (this test's premise), \
         but it did not: update this test if that file changed",
        path.display()
    );
}

/// `willikins-types`'s `probe` module is declared `#[cfg(test)] mod
/// probe;` in `lib.rs`, so nothing inside `probe.rs` itself needs to
/// carry `#[cfg(test)]` for its `SinkToken::new()` calls to be exempt —
/// this pins that `walk_crate`'s parent-declared-cfg(test) inheritance is
/// not vacuous, the same way the previous test pins the `tests/`
/// directory exemption.
#[test]
fn probe_rs_calls_sink_token_new_and_is_exempt_via_its_parent_declaration() {
    let path = crates_root().join("willikins-types/src/probe.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(
        source.contains("SinkToken::new()"),
        "expected {} to call SinkToken::new with no #[cfg(test)] of its own \
         (this test's premise), but it did not: update this test if that file changed",
        path.display()
    );
    let lib_rs = std::fs::read_to_string(crates_root().join("willikins-types/src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("#[cfg(test)]\nmod probe;"),
        "expected willikins-types/src/lib.rs to declare `#[cfg(test)] mod probe;`: \
         update this test (and re-check `walk_crate`'s inheritance) if that changed"
    );
}

/// The executor's own call site is asserted to be exactly one, through
/// the same `walk_crate` machinery the main guard test uses (not a text
/// search): walking `apply.rs` with a never-matching exempt path (so its
/// own call site is *not* filtered out this time) must find exactly one
/// violation. `apply.rs` declares `mod principal; mod timestamp;`, both
/// walked too as a side effect; neither calls `SinkToken::new`, so this
/// still isolates the count to `apply.rs`'s own site.
#[test]
fn the_executor_has_exactly_one_sink_token_new_call_site() {
    let path = crates_root().join("willikins-core/src/apply.rs");
    let mut violations = Vec::new();
    let _ = walk_crate(&path, Path::new("__no_exempt_module__"), &mut violations);
    assert_eq!(
        violations.len(),
        1,
        "expected exactly one non-test SinkToken::new call site starting from {}: {violations:?}",
        path.display()
    );
}

/// Proof the second pass is not decorative: a file cargo compiles that no
/// `mod` declaration mentions (here a `build.rs`) is walked, and the
/// directories that are exempt outright are skipped.
#[test]
fn a_file_outside_the_module_graph_is_walked_and_tests_and_target_are_skipped() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sink_token_guard_second_pass");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests")).expect("creates tests/");
    std::fs::create_dir_all(dir.join("target")).expect("creates target/");
    let build_rs = dir.join("build.rs");
    std::fs::write(&build_rs, "fn main() { let _ = SinkToken::new(); }").expect("writes build.rs");
    for skipped in ["tests/fixture.rs", "target/generated.rs"] {
        std::fs::write(dir.join(skipped), "fn main() { let _ = SinkToken::new(); }")
            .expect("writes a file that must be skipped");
    }

    let mut files = Vec::new();
    all_rs_files(&dir, &mut files);
    assert_eq!(files, vec![build_rs.clone()], "{files:?}");

    let mut violations = Vec::new();
    walk_single_file(
        &build_rs,
        Path::new("__no_exempt_module__"),
        &mut violations,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");

    std::fs::remove_dir_all(&dir).expect("cleans up");
}

// -------------------------------------------------------------
// `FileWalker` unit tests: proof the guard actually catches something.
//
// Every test above this point passes vacuously if `path_is_sink_token_new`
// returned `false` unconditionally -- the workspace has zero violations
// today, so an always-empty `violations` list would look identical to a
// working guard. These tests run `FileWalker` directly over small,
// hand-written sources and assert on what it finds, independent of
// anything in the real workspace tree.
// -------------------------------------------------------------

/// Every violation `FileWalker` finds walking `source` as a whole file,
/// starting `test_only` as given.
fn violations_in(source: &str, test_only: bool) -> Vec<String> {
    let file =
        syn::parse_file(source).unwrap_or_else(|err| panic!("{source:?} failed to parse: {err}"));
    let aliases = collect_sink_token_aliases(&file);
    let mut walker = FileWalker::new(&aliases);
    walker.test_only = test_only;
    for item in &file.items {
        walker.visit_item(item);
    }
    walker.violations
}

#[test]
fn walker_catches_a_bare_unqualified_call() {
    let violations = violations_in("fn f() { SinkToken::new(); }", false);
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_exempts_a_call_inside_a_cfg_test_function() {
    let violations = violations_in("#[cfg(test)] fn f() { SinkToken::new(); }", false);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_exempts_a_call_inside_a_cfg_test_inline_module() {
    let violations = violations_in("#[cfg(test)] mod t { fn f() { SinkToken::new(); } }", false);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_catches_a_call_through_a_renamed_import() {
    let violations = violations_in(
        "use willikins_types::SinkToken as T; fn f() { T::new(); }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_catches_a_fully_qualified_call() {
    let violations = violations_in("fn f() { <SinkToken>::new(); }", false);
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_treats_the_whole_file_as_test_only_when_told_to() {
    // Mirrors how `walk_crate` treats a file reached only through a
    // `#[cfg(test)] mod foo;` declaration in its parent (`probe.rs`'s own
    // situation): the file itself carries no `#[cfg(test)]`, but the
    // caller starts the walk with `test_only: true`.
    let violations = violations_in("fn f() { SinkToken::new(); }", true);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_catches_a_call_inside_a_macro_invocation() {
    let violations = violations_in("fn f() { let _ = vec![SinkToken::new()]; }", false);
    assert_eq!(
        violations.len(),
        1,
        "a macro invocation's body is opaque to syn's AST and needs the token scan: {violations:?}"
    );
}

#[test]
fn walker_exempts_a_macro_invocation_inside_a_cfg_test_function() {
    let violations = violations_in(
        "#[cfg(test)] fn f() { let _ = vec![SinkToken::new()]; }",
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

/// `#[cfg(not(test))]` is not `#[cfg(test)]`: an item that exists only in
/// a *non*-test build is exactly the place a real call site could hide, so
/// [`has_cfg_test`] must not treat it as test-only. (It does not today,
/// because its nested-meta walk looks for the literal path `test` and
/// `not(test)`'s own path is `not` — but a future rewrite of that function
/// as a substring check over the attribute's tokens would silently exempt
/// this, which is what this test is here to catch.)
#[test]
fn walker_catches_a_call_inside_a_cfg_not_test_item() {
    let violations = violations_in("#[cfg(not(test))] fn f() { SinkToken::new(); }", false);
    assert_eq!(violations.len(), 1, "{violations:?}");
}

/// A `macro_rules!` *definition*'s body is a token stream too, not just a
/// macro *invocation*'s: a call written inside one is expanded at every
/// call site and must be found.
#[test]
fn walker_catches_a_call_inside_a_macro_rules_definition() {
    let violations = violations_in(
        "macro_rules! mint { () => { SinkToken::new() }; } fn f() { let _ = mint!(); }",
        false,
    );
    assert!(
        !violations.is_empty(),
        "a macro_rules! body must be scanned: {violations:?}"
    );
}
