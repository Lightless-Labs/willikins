//! Acceptance test 4, `expose_secret` half: every call site of
//! `secrecy::ExposeSecret::expose_secret` (and of
//! `secrecy::ExposeSecretMut::expose_secret_mut`, the second bytes-out
//! method `secrecy` 0.10 offers) in the workspace is either
//! inside the derive's own codegen emitter
//! (`willikins-derive/src/codegen.rs`, which only ever emits the tokens —
//! it never calls the method itself), inside
//! `willikins_providers_http::Credential::authorize`, inside a
//! `#[cfg(test)]` item, or inside a file under a `tests/` directory.
//!
//! Same technique as `tests/sink_token_guard.rs` (parses every crate's
//! `src/` tree with `syn` rather than grepping, so a renamed import or a
//! fully qualified path cannot hide a call, and a macro invocation's
//! opaque token stream — exactly how the derive's own `quote!{}` body
//! carries its `expose_secret` reference — is scanned as raw tokens
//! rather than skipped), generalised two ways `SinkToken::new` did not
//! need:
//!
//! - `expose_secret` is far more often called through **method-call**
//!   syntax (`secret.expose_secret()`) than the fully-qualified
//!   `Type::method(...)` form `SinkToken::new()` always uses, so this
//!   walker treats *any* `.expose_secret(...)` method call as a hit,
//!   regardless of the receiver's (unknown, to `syn`) type.
//! - one of the three allowed call sites is a whole file
//!   (`willikins-derive/src/codegen.rs`) and another is one specific
//!   function inside an otherwise-ordinary file
//!   (`Credential::authorize`), so the exemption this file resolves per
//!   crate is either "the whole file" or "one named function in this one
//!   file", rather than `sink_token_guard.rs`'s single "one named module
//!   path" shape.
//!
//! The plan's acceptance test 4 says "every `.rs` file in the workspace",
//! so the module graph alone is not enough: a `build.rs`, a `src/bin/`
//! entry point, a file reached only through `include!`, a bench or an
//! example is compiled by cargo and invisible to a walk that follows
//! `mod` declarations from `lib.rs`. Every crate is therefore walked
//! twice: once through its module graph (which is what carries
//! `#[cfg(test)] mod probe;` test-only inheritance into a file that does
//! not say so itself), and once over every remaining `.rs` file under the
//! crate directory, `tests/` excepted.
//!
//! Deliberately not merged into `sink_token_guard.rs` itself: the two
//! guards check an unrelated method on an unrelated crate's clippy entry,
//! and keeping them in separate files means a change to one's exemption
//! rules cannot silently loosen the other's.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, ItemUse, UseTree};

fn crates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/willikins-core has a parent directory")
        .to_path_buf()
}

fn crate_dirs() -> Vec<PathBuf> {
    std::fs::read_dir(crates_root())
        .expect("crates/ is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    context: String,
}

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

/// The traits whose methods hand out a secret's bytes. `ExposeSecretMut`
/// is `secrecy` 0.10's second one: it returns `&mut str`, which is just
/// as much a way out of a `SecretString` as `expose_secret`'s `&str`.
const EXPOSE_TRAITS: [&str; 2] = ["ExposeSecret", "ExposeSecretMut"];

/// The methods those traits offer.
const EXPOSE_METHODS: [&str; 2] = ["expose_secret", "expose_secret_mut"];

/// Whether `ident` names one of [`EXPOSE_METHODS`].
fn is_expose_method(ident: &syn::Ident) -> bool {
    EXPOSE_METHODS.iter().any(|method| ident == method)
}

/// Every name `ExposeSecret` or `ExposeSecretMut` is imported under in `file`, plus the bare
/// name itself (so a fully qualified path that never went through a
/// `use` — every real call site in this workspace today — still
/// matches).
fn collect_expose_secret_aliases(file: &syn::File) -> HashSet<String> {
    struct AliasCollector {
        aliases: HashSet<String>,
    }

    fn walk_use_tree(tree: &UseTree, aliases: &mut HashSet<String>) {
        match tree {
            UseTree::Path(p) => walk_use_tree(&p.tree, aliases),
            UseTree::Name(n) => {
                let name = n.ident.to_string();
                if EXPOSE_TRAITS.contains(&name.as_str()) {
                    aliases.insert(name);
                }
            }
            UseTree::Rename(r) => {
                if EXPOSE_TRAITS.contains(&r.ident.to_string().as_str()) {
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
        aliases: EXPOSE_TRAITS.iter().map(|t| (*t).to_string()).collect(),
    };
    collector.visit_file(file);
    collector.aliases
}

fn flatten_tokens(stream: proc_macro2::TokenStream, out: &mut Vec<proc_macro2::TokenTree>) {
    for tt in stream {
        if let proc_macro2::TokenTree::Group(group) = &tt {
            flatten_tokens(group.stream(), out);
        } else {
            out.push(tt);
        }
    }
}

/// Every hit of either shape inside a macro invocation's (or
/// `macro_rules!` definition's) raw token stream: `<alias>::expose_secret`
/// (the UFCS form the derive's `quote!{}` body uses) and
/// `.expose_secret` (a method call written inside a macro, such as an
/// `assert_eq!` wrapping one).
fn scan_tokens_for_expose_secret(
    tokens: proc_macro2::TokenStream,
    aliases: &HashSet<String>,
) -> Vec<String> {
    let mut flat = Vec::new();
    flatten_tokens(tokens, &mut flat);
    let mut hits = Vec::new();
    let mut i = 0;
    while i < flat.len() {
        // `<alias>::expose_secret`
        if i + 3 < flat.len()
            && let (
                proc_macro2::TokenTree::Ident(owner),
                proc_macro2::TokenTree::Punct(colon1),
                proc_macro2::TokenTree::Punct(colon2),
                proc_macro2::TokenTree::Ident(method),
            ) = (&flat[i], &flat[i + 1], &flat[i + 2], &flat[i + 3])
            && colon1.as_char() == ':'
            && colon2.as_char() == ':'
            && is_expose_method(method)
            && aliases.contains(&owner.to_string())
        {
            hits.push(format!("{owner}::{method}"));
        }
        // `.expose_secret`
        if i + 1 < flat.len()
            && let (proc_macro2::TokenTree::Punct(dot), proc_macro2::TokenTree::Ident(method)) =
                (&flat[i], &flat[i + 1])
            && dot.as_char() == '.'
            && is_expose_method(method)
        {
            hits.push(format!(".{method}"));
        }
        i += 1;
    }
    hits
}

/// Whether `path` (optionally qualified via `qself`) names
/// `<alias>::expose_secret` for some `alias` in `aliases` — the UFCS call
/// form (`ExposeSecret::expose_secret(&x)`, `<T as
/// ExposeSecret>::expose_secret(&x)`).
fn path_is_expose_secret_ufcs(
    qself: Option<&syn::QSelf>,
    path: &syn::Path,
    aliases: &HashSet<String>,
) -> bool {
    let segments: Vec<&syn::Ident> = path.segments.iter().map(|s| &s.ident).collect();
    let Some(last) = segments.last() else {
        return false;
    };
    if !is_expose_method(last) {
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

/// One file's exemption: either the whole file is the allowed call site
/// (the derive's codegen emitter), or one named function in it is
/// (`Credential::authorize`), or neither.
#[derive(Clone, Copy)]
enum Exemption {
    WholeFile,
    OneFunction(&'static [&'static str]),
    None,
}

struct FileWalker<'a> {
    aliases: &'a HashSet<String>,
    exemption: Exemption,
    test_only: bool,
    in_exempt_fn: bool,
    violations: Vec<String>,
    pending_mods: Vec<(String, bool)>,
}

impl<'a> FileWalker<'a> {
    fn new(aliases: &'a HashSet<String>, exemption: Exemption) -> Self {
        Self {
            aliases,
            exemption,
            test_only: false,
            in_exempt_fn: false,
            violations: Vec::new(),
            pending_mods: Vec::new(),
        }
    }

    fn exempt_here(&self) -> bool {
        self.test_only || self.in_exempt_fn
    }

    fn is_exempt_fn_name(&self, ident: &syn::Ident) -> bool {
        matches!(self.exemption, Exemption::OneFunction(names) if names.iter().any(|name| ident == name))
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
        let was_exempt = self.in_exempt_fn;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        if self.is_exempt_fn_name(&node.sig.ident) {
            self.in_exempt_fn = true;
        }
        syn::visit::visit_item_fn(self, node);
        self.test_only = was_test;
        self.in_exempt_fn = was_exempt;
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
        let was_test = self.test_only;
        let was_exempt = self.in_exempt_fn;
        if has_cfg_test(&node.attrs) {
            self.test_only = true;
        }
        if self.is_exempt_fn_name(&node.sig.ident) {
            self.in_exempt_fn = true;
        }
        syn::visit::visit_impl_item_fn(self, node);
        self.test_only = was_test;
        self.in_exempt_fn = was_exempt;
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if !self.exempt_here() && is_expose_method(&node.method) {
            self.violations
                .push(format!("(method call) {}", quote::quote!(#node)));
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if !self.exempt_here()
            && path_is_expose_secret_ufcs(node.qself.as_ref(), &node.path, self.aliases)
        {
            self.violations.push(quote::quote!(#node).to_string());
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if !self.exempt_here() {
            for hit in scan_tokens_for_expose_secret(node.tokens.clone(), self.aliases) {
                self.violations
                    .push(format!("(inside a macro invocation) {hit}"));
            }
        }
        syn::visit::visit_macro(self, node);
    }
}

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

/// One crate's set of exempt files: each a relative path from the
/// crate's `src/` directory (e.g. `credential.rs`), paired with either
/// the function names exempt inside it or `None` to exempt the whole
/// file. Almost always zero or one entry; `willikins-types` has two
/// (`secret.rs`'s `reveal_for_transform`, `appstore.rs`'s hand-written
/// `expose`/`eq`) because it holds two independently hand-written secret
/// types.
type Exemptions<'a> = &'a [(PathBuf, Option<&'static [&'static str]>)];

/// Resolve `path`'s [`Exemption`] against `exemptions`: the first entry
/// whose relative path `path` ends with, or [`Exemption::None`] if none
/// matches.
fn exemption_for(path: &Path, exemptions: Exemptions<'_>) -> Exemption {
    for (rel, fn_names) in exemptions {
        if path.ends_with(rel) {
            return match fn_names {
                None => Exemption::WholeFile,
                Some(names) => Exemption::OneFunction(names),
            };
        }
    }
    Exemption::None
}

/// Walk one crate's `src/` module graph starting from `root`, resolving
/// each file's exemption against `exemptions`.
fn walk_crate(
    root: &Path,
    exemptions: Exemptions<'_>,
    violations: &mut Vec<Violation>,
) -> HashSet<PathBuf> {
    let mut queue: Vec<(PathBuf, bool)> = vec![(root.to_path_buf(), false)];
    let mut visited: HashSet<PathBuf> = HashSet::new();

    while let Some((path, inherited_test_only)) = queue.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let exemption = exemption_for(&path, exemptions);
        let whole_file_exempt = matches!(exemption, Exemption::WholeFile);

        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        let file = syn::parse_file(&source)
            .unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));

        let aliases = collect_expose_secret_aliases(&file);
        let mut walker = FileWalker::new(&aliases, exemption);
        walker.test_only = inherited_test_only;
        for item in &file.items {
            walker.visit_item(item);
        }

        if !whole_file_exempt {
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

/// Walk one file that the module-graph walk never reached, as ordinary
/// (non-test) code.
fn walk_single_file(path: &Path, exemptions: Exemptions<'_>, violations: &mut Vec<Violation>) {
    let exemption = exemption_for(path, exemptions);
    if matches!(exemption, Exemption::WholeFile) {
        return;
    }
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    let file =
        syn::parse_file(&source).unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));
    let aliases = collect_expose_secret_aliases(&file);
    let mut walker = FileWalker::new(&aliases, exemption);
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
fn every_expose_secret_call_site_is_the_codegen_emitter_authorize_a_test_item_or_a_tests_file() {
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
        let crate_name = crate_dir.file_name().and_then(|n| n.to_str());
        let exemptions: Vec<(PathBuf, Option<&'static [&'static str]>)> = match crate_name {
            Some("willikins-derive") => vec![(PathBuf::from("codegen.rs"), None)],
            Some("willikins-providers-http") => vec![(
                PathBuf::from("credential.rs"),
                Some(&["authorize", "authorize_header"][..]),
            )],
            Some("willikins-types") => vec![
                (
                    PathBuf::from("secret.rs"),
                    Some(&["reveal_for_transform"][..]),
                ),
                (PathBuf::from("appstore.rs"), Some(&["expose", "eq"][..])),
            ],
            _ => Vec::new(),
        };
        let visited = walk_crate(&root, &exemptions, &mut violations);

        // Second pass: everything cargo compiles that the module graph
        // never mentions — `build.rs`, a `src/bin/` entry point, a bench,
        // an example, a file reached only through `include!`.
        let mut every_file = Vec::new();
        all_rs_files(&crate_dir, &mut every_file);
        for path in every_file {
            if visited.contains(&path) {
                continue;
            }
            walk_single_file(&path, &exemptions, &mut violations);
        }
    }

    assert!(
        violations.is_empty(),
        "found expose_secret call site(s) outside the codegen emitter, \
         Credential::authorize, and outside any #[cfg(test)] item:\n{}",
        violations
            .iter()
            .map(|v| format!("  {} :: {}", v.file.display(), v.context))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Proof the derive's exemption is not vacuous: `codegen.rs` really does
/// reference `expose_secret` today (inside a `quote!{}` body), so the
/// whole-file exemption is doing real work.
#[test]
fn derive_codegen_rs_references_expose_secret_and_is_exempt_as_a_whole_file() {
    let path = crates_root().join("willikins-derive/src/codegen.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(
        source.contains("ExposeSecret::expose_secret"),
        "expected {} to reference ExposeSecret::expose_secret (this test's premise); \
         update this test if the derive's codegen changed shape",
        path.display()
    );
}

/// Proof `Credential::authorize`'s and `Credential::authorize_header`'s
/// exemption is not vacuous, and is scoped to exactly those two
/// functions: walking `credential.rs` with an exemption naming a function
/// that does not exist in it must still find both real call sites.
#[test]
fn credential_rs_calls_expose_secret_inside_authorize_and_authorize_header_and_nowhere_else() {
    let path = crates_root().join("willikins-providers-http/src/credential.rs");

    let mut with_correct_exemption = Vec::new();
    let _ = walk_crate(
        &path,
        &[(
            PathBuf::from("credential.rs"),
            Some(&["authorize", "authorize_header"][..]),
        )],
        &mut with_correct_exemption,
    );
    assert!(
        with_correct_exemption.is_empty(),
        "authorize's and authorize_header's call sites should be exempt: {with_correct_exemption:?}"
    );

    let mut with_wrong_exemption = Vec::new();
    let _ = walk_crate(
        &path,
        &[(
            PathBuf::from("credential.rs"),
            Some(&["not_a_real_function"][..]),
        )],
        &mut with_wrong_exemption,
    );
    assert_eq!(
        with_wrong_exemption.len(),
        2,
        "expected exactly two call sites in credential.rs (authorize and \
         authorize_header) when neither is the exempted function: {with_wrong_exemption:?}"
    );
}

/// Proof the second pass is not decorative: a file cargo compiles that no
/// `mod` declaration mentions (here a `build.rs`) is walked, and the
/// directories that are exempt outright are skipped.
#[test]
fn a_file_outside_the_module_graph_is_walked_and_tests_and_target_are_skipped() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("expose_secret_guard_second_pass");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests")).expect("creates tests/");
    std::fs::create_dir_all(dir.join("target")).expect("creates target/");
    let build_rs = dir.join("build.rs");
    std::fs::write(
        &build_rs,
        "fn main() { let s = secret(); let _ = s.expose_secret(); }",
    )
    .expect("writes build.rs");
    for skipped in ["tests/fixture.rs", "target/generated.rs"] {
        std::fs::write(
            dir.join(skipped),
            "fn main() { let s = secret(); let _ = s.expose_secret(); }",
        )
        .expect("writes a file that must be skipped");
    }

    let mut files = Vec::new();
    all_rs_files(&dir, &mut files);
    assert_eq!(files, vec![build_rs.clone()], "{files:?}");

    let mut violations = Vec::new();
    walk_single_file(&build_rs, &[], &mut violations);
    assert_eq!(violations.len(), 1, "{violations:?}");

    std::fs::remove_dir_all(&dir).expect("cleans up");
}

/// `expose_secret_mut` is `secrecy` 0.10's second way out of a
/// `SecretString`, and is caught the same way.
#[test]
fn walker_catches_expose_secret_mut() {
    let violations = violations_in(
        "fn f(s: &mut SecretString) { s.expose_secret_mut(); }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");

    let violations = violations_in(
        "use secrecy::ExposeSecretMut as Poke; fn f(s: &mut SecretString) { Poke::expose_secret_mut(s); }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");

    let violations = violations_in(
        "fn f(s: &mut SecretString) { let _ = vec![s.expose_secret_mut()]; }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
}

// -------------------------------------------------------------
// `FileWalker` unit tests, independent of the real workspace tree.
// -------------------------------------------------------------

fn violations_in(source: &str, test_only: bool) -> Vec<String> {
    let file =
        syn::parse_file(source).unwrap_or_else(|err| panic!("{source:?} failed to parse: {err}"));
    let aliases = collect_expose_secret_aliases(&file);
    let mut walker = FileWalker::new(&aliases, Exemption::None);
    walker.test_only = test_only;
    for item in &file.items {
        walker.visit_item(item);
    }
    walker.violations
}

#[test]
fn walker_catches_a_method_call() {
    let violations = violations_in("fn f(s: &SecretString) { s.expose_secret(); }", false);
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_catches_a_ufcs_call() {
    let violations = violations_in(
        "fn f(s: &SecretString) { ExposeSecret::expose_secret(s); }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_catches_a_call_through_a_renamed_import() {
    let violations = violations_in(
        "use secrecy::ExposeSecret as Peek; fn f(s: &SecretString) { Peek::expose_secret(s); }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_exempts_a_call_inside_a_cfg_test_function() {
    let violations = violations_in(
        "#[cfg(test)] fn f(s: &SecretString) { s.expose_secret(); }",
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_exempts_a_call_inside_a_cfg_test_inline_module() {
    let violations = violations_in(
        "#[cfg(test)] mod t { fn f(s: &SecretString) { s.expose_secret(); } }",
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_treats_the_whole_file_as_test_only_when_told_to() {
    let violations = violations_in("fn f(s: &SecretString) { s.expose_secret(); }", true);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn walker_catches_a_method_call_inside_a_macro_invocation() {
    let violations = violations_in(
        "fn f(s: &SecretString) { let _ = vec![s.expose_secret()]; }",
        false,
    );
    assert_eq!(
        violations.len(),
        1,
        "a macro invocation's body is opaque to syn's AST and needs the token scan: {violations:?}"
    );
}

#[test]
fn walker_catches_a_ufcs_call_inside_a_macro_invocation() {
    let violations = violations_in(
        "fn f(s: &SecretString) { let _ = vec![ExposeSecret::expose_secret(s)]; }",
        false,
    );
    assert_eq!(violations.len(), 1, "{violations:?}");
}

#[test]
fn walker_exempts_a_macro_invocation_inside_a_cfg_test_function() {
    let violations = violations_in(
        "#[cfg(test)] fn f(s: &SecretString) { let _ = vec![s.expose_secret()]; }",
        false,
    );
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn one_named_function_exemption_only_exempts_that_function() {
    let source = "fn authorize(s: &SecretString) { s.expose_secret(); } \
                  fn other(s: &SecretString) { s.expose_secret(); }";
    let file = syn::parse_file(source).unwrap();
    let aliases = collect_expose_secret_aliases(&file);
    let mut walker = FileWalker::new(&aliases, Exemption::OneFunction(&["authorize"]));
    for item in &file.items {
        walker.visit_item(item);
    }
    assert_eq!(
        walker.violations.len(),
        1,
        "only `other`'s call site should count: {:?}",
        walker.violations
    );
}
