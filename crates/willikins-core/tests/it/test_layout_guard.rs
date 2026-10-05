//! The layout guard (decision (d7) of
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md`): once a crate's
//! integration tests have moved into one `tests/it/` binary, this keeps
//! the result from eroding, and ratchets the move itself so a half-done
//! crate cannot hide.
//!
//! [`PENDING`] names every crate whose tests have not yet moved. For a
//! crate **not** in [`PENDING`], [`check_crate`] enforces five rules:
//!
//! 1. every top-level `tests/*.rs` is either a gated `[[test]]` target
//!    (one with a `required-features` line in that crate's `Cargo.toml`)
//!    or, in `willikins-cli` only, an `operator_*.rs` private target;
//! 2. `tests/it/main.rs` exists, and every other `tests/it/*.rs` is
//!    declared there by a `mod <stem>;` line -- an undeclared file would
//!    silently stop compiling;
//! 3. `tests/snapshots/` (the old, pre-move location) is absent, or (in
//!    `willikins-cli` only) holds only `operator_*` entries;
//! 4. every `tests/it/snapshots/*.snap` starts with `it__<m>__` for a
//!    module `<m>` that `main.rs` declares -- catching an orphan left by
//!    a later module rename;
//! 5. no top-level `tests/*.proptest-regressions` file exists, since
//!    proptest's `SourceParallel` walk now stops at `tests/it/` and would
//!    otherwise silently never read a seed left behind.
//!
//! For a crate **in** [`PENDING`], [`check_crate`] asserts the opposite:
//! no `tests/it/` yet, so a half-done move cannot hide behind the list.
//!
//! `Cargo.toml` is read as text -- a line scan of `[[test]]` blocks for
//! `name = "..."` and `required-features`, the same scan the per-crate
//! move recipe (R, step 2) uses. No TOML crate enters the tree.

use std::path::{Path, PathBuf};

/// Crates whose integration tests have not yet moved into `tests/it/`.
/// Shrinks by one per milestone 3k task (see the plan's task table,
/// T4-T14) and ends empty at T14. The constant and this guard then stay,
/// so a new crate starts in the target layout.
const PENDING: &[&str] = &[];

/// Which of (d7)'s rules a [`Violation`] names, plus the ratchet's own
/// direction for a crate in [`PENDING`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// Rule 1: a stray top-level `tests/*.rs` that is neither a gated
    /// target nor (for `willikins-cli`) an `operator_*` private one.
    StrayTopLevelFile,
    /// Rule 2: `tests/it/main.rs` is missing, or a `tests/it/*.rs` file
    /// it does not declare with a `mod <stem>;` line.
    UndeclaredModule,
    /// Rule 3: a leftover `tests/snapshots/` entry that is not an
    /// `operator_*` one in `willikins-cli`.
    LeftoverSnapshotDir,
    /// Rule 4: a `tests/it/snapshots/*.snap` that does not start with
    /// `it__<m>__` for a module `<m>` `main.rs` declares.
    OrphanSnapshot,
    /// Rule 5: a leftover top-level `tests/*.proptest-regressions` file.
    LeftoverProptestRegressions,
    /// The ratchet's own direction: a crate still in [`PENDING`] already
    /// has a `tests/it/` directory.
    PendingAlreadyMoved,
}

/// One offense [`check_crate`] found, naming the [`Rule`] it breaks and a
/// human-readable detail (the path or file involved).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    rule: Rule,
    detail: String,
}

/// The workspace's `crates/` directory, from this crate's own manifest
/// directory.
fn crates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/willikins-core has a parent directory")
        .to_path_buf()
}

/// Every `[[test]] name = "..."` entry in `cargo_toml_text` that carries
/// a `required-features` line in the same block, read as a line scan: a
/// block starts at a line that is exactly `[[test]]` and ends at the
/// next line whose trimmed text starts with `[` (another table header)
/// or at the end of the file. A `#`-commented line never counts as
/// either field, so a disabled gate cannot be mistaken for a live one.
fn gated_test_names(cargo_toml_text: &str) -> Vec<String> {
    fn flush(name: &mut Option<String>, gated: bool, out: &mut Vec<String>) {
        if let Some(n) = name.take()
            && gated
        {
            out.push(n);
        }
    }

    let mut names = Vec::new();
    let mut in_block = false;
    let mut current_name: Option<String> = None;
    let mut has_required_features = false;

    for raw_line in cargo_toml_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "[[test]]" {
            flush(&mut current_name, has_required_features, &mut names);
            in_block = true;
            has_required_features = false;
            continue;
        }
        if trimmed.starts_with('[') {
            flush(&mut current_name, has_required_features, &mut names);
            in_block = false;
            continue;
        }
        if !in_block {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name")
            && let Some(rest) = rest.trim_start().strip_prefix('=')
        {
            current_name = Some(rest.trim().trim_matches('"').to_string());
        } else if trimmed.starts_with("required-features") {
            has_required_features = true;
        }
    }
    flush(&mut current_name, has_required_features, &mut names);
    names
}

/// Whether `stem` is a top-level file this crate is allowed to keep
/// outside `tests/it/`: a gated `[[test]]` target, or (`willikins-cli`
/// only) an `operator_*` private target (decision (d4)).
fn is_allowed_top_level(stem: &str, crate_name: &str, gated: &[String]) -> bool {
    gated.iter().any(|g| g == stem)
        || (crate_name == "willikins-cli" && stem.starts_with("operator_"))
}

/// The file stems of every plain file directly under `dir` whose name
/// ends in `.rs`. A directory (`common/`, `fixtures/`, `support/`,
/// `snapshots/`, `it/`, `ui/`, `derive/`, `conversions/`) is never swept
/// by this: only files directly in `tests/` are top-level test targets.
fn top_level_rs_stems(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(
                    path.file_stem()
                        .expect("a .rs file has a stem")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    out
}

/// Every identifier declared by a `mod <ident>;` (or `pub mod <ident>;`)
/// line in `main_rs_text`. A commented-out line (`// mod foo;`) is never
/// a declaration -- matching that would turn rule 2 into exactly the
/// silent loss it exists to catch, since a commented module compiles
/// nothing.
fn declared_modules(main_rs_text: &str) -> Vec<String> {
    main_rs_text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed
                .strip_prefix("mod ")
                .or_else(|| trimmed.strip_prefix("pub mod "))?;
            let ident = rest.strip_suffix(';')?.trim();
            (!ident.is_empty() && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .then_some(ident.to_string())
        })
        .collect()
}

/// Checks one crate directory (holding `Cargo.toml` and `tests/`)
/// against (d7)'s rules, given whether it is still in [`PENDING`].
/// `crate_name` is taken as a parameter rather than derived from `dir`,
/// since a fixture's throwaway directory is never actually named
/// `willikins-cli` on disk but must still be checked as if it were, to
/// prove the `willikins-cli`-only clauses of rules 1 and 3.
// One linear walk over decision (d7)'s five rules, in the plan's order, so a
// reader can match each block to its rule; splitting it would scatter them.
#[allow(clippy::too_many_lines)]
fn check_crate(dir: &Path, crate_name: &str, pending: bool) -> Vec<Violation> {
    let tests_dir = dir.join("tests");
    if !tests_dir.is_dir() {
        return Vec::new();
    }

    let it_dir = tests_dir.join("it");
    if pending {
        return if it_dir.is_dir() {
            vec![Violation {
                rule: Rule::PendingAlreadyMoved,
                detail: format!("{crate_name}: tests/it/ exists but the crate is still in PENDING"),
            }]
        } else {
            Vec::new()
        };
    }

    let mut violations = Vec::new();

    // Rule 1.
    let cargo_toml = dir.join("Cargo.toml");
    let gated = if cargo_toml.is_file() {
        gated_test_names(&std::fs::read_to_string(&cargo_toml).expect("Cargo.toml is readable"))
    } else {
        Vec::new()
    };
    for stem in top_level_rs_stems(&tests_dir) {
        if !is_allowed_top_level(&stem, crate_name, &gated) {
            violations.push(Violation {
                rule: Rule::StrayTopLevelFile,
                detail: format!(
                    "{crate_name}: tests/{stem}.rs is not a gated target or an operator_* file"
                ),
            });
        }
    }

    // Rule 2, and the module set rule 4 needs.
    let main_rs = it_dir.join("main.rs");
    let declared = if main_rs.is_file() {
        let text = std::fs::read_to_string(&main_rs).expect("main.rs is readable");
        let declared = declared_modules(&text);
        for stem in top_level_rs_stems(&it_dir) {
            if stem != "main" && !declared.iter().any(|m| m == &stem) {
                violations.push(Violation {
                    rule: Rule::UndeclaredModule,
                    detail: format!(
                        "{crate_name}: tests/it/{stem}.rs has no `mod {stem};` line in main.rs"
                    ),
                });
            }
        }
        declared
    } else {
        violations.push(Violation {
            rule: Rule::UndeclaredModule,
            detail: format!("{crate_name}: tests/it/main.rs is missing"),
        });
        Vec::new()
    };

    // Rule 3.
    let old_snapshots = tests_dir.join("snapshots");
    if old_snapshots.is_dir()
        && let Ok(entries) = std::fs::read_dir(&old_snapshots)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let allowed = crate_name == "willikins-cli" && name.starts_with("operator_");
            if !allowed {
                violations.push(Violation {
                    rule: Rule::LeftoverSnapshotDir,
                    detail: format!("{crate_name}: leftover tests/snapshots/{name}"),
                });
            }
        }
    }

    // Rule 4.
    let it_snapshots = it_dir.join("snapshots");
    if it_snapshots.is_dir()
        && let Ok(entries) = std::fs::read_dir(&it_snapshots)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if std::path::Path::new(&name)
                .extension()
                .is_none_or(|ext| ext != "snap")
            {
                continue;
            }
            let ok = name.strip_prefix("it__").is_some_and(|rest| {
                declared
                    .iter()
                    .any(|m| rest.starts_with(format!("{m}__").as_str()))
            });
            if !ok {
                violations.push(Violation {
                    rule: Rule::OrphanSnapshot,
                    detail: format!(
                        "{crate_name}: tests/it/snapshots/{name} names no module main.rs declares"
                    ),
                });
            }
        }
    }

    // Rule 5.
    if let Ok(entries) = std::fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".proptest-regressions") {
                violations.push(Violation {
                    rule: Rule::LeftoverProptestRegressions,
                    detail: format!("{crate_name}: leftover tests/{name}"),
                });
            }
        }
    }

    violations
}

/// A throwaway scratch directory for fixture `case`, removed first so a
/// previous run's leftovers (or a left-behind `.snap.new`) cannot leak
/// into this one.
fn scratch(case: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("test_layout_guard_{case}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

/// Writes a minimal, rule-compliant `tests/it/` under `dir`: one module
/// `a`, declared, no snapshots, no stray top-level files. Every fixture
/// below starts from this and plants exactly one defect, so a positive
/// result can be attributed to that defect alone.
fn write_well_formed_it(dir: &Path) {
    let it_dir = dir.join("tests").join("it");
    std::fs::create_dir_all(&it_dir).expect("creates tests/it/");
    std::fs::write(it_dir.join("main.rs"), "mod a;\n").expect("writes main.rs");
    std::fs::write(it_dir.join("a.rs"), "#[test]\nfn it_works() {}\n").expect("writes a.rs");
}

#[test]
fn a_well_formed_non_pending_crate_has_no_violations() {
    let dir = scratch("well_formed");
    write_well_formed_it(&dir);
    assert_eq!(check_crate(&dir, "willikins-example", false), Vec::new());
}

#[test]
fn a_pending_crate_with_no_it_has_no_violations() {
    let dir = scratch("pending_clean");
    std::fs::create_dir_all(dir.join("tests")).expect("creates tests/");
    std::fs::write(
        dir.join("tests").join("old_style.rs"),
        "#[test]\nfn it_works() {}\n",
    )
    .expect("writes a pre-move top-level file");
    assert_eq!(check_crate(&dir, "willikins-example", true), Vec::new());
}

/// Rule 1: a stray top-level `tests/*.rs` with no gated `[[test]]` entry
/// for it (and, in this fixture, a `[[test]]` block that names it but
/// without `required-features`, proving that clause is checked too, not
/// just the block's presence).
#[test]
fn rule_1_fires_on_a_stray_top_level_file() {
    let dir = scratch("stray_top_level");
    write_well_formed_it(&dir);
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"willikins-example\"\n\n[[test]]\nname = \"stray\"\n",
    )
    .expect("writes Cargo.toml");
    std::fs::write(
        dir.join("tests").join("stray.rs"),
        "#[test]\nfn it_works() {}\n",
    )
    .expect("writes the stray file");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::StrayTopLevelFile,
            detail: "willikins-example: tests/stray.rs is not a gated target or an operator_* file"
                .to_string(),
        }]
    );
}

/// Rule 1's `willikins-cli`-only clause: an `operator_*.rs` top-level
/// file, and an `operator_*` entry in the old `tests/snapshots/`, are
/// both allowed with no `Cargo.toml` gate at all.
#[test]
fn rule_1_and_rule_3_exempt_cli_operator_files() {
    let dir = scratch("cli_operator_exemption");
    write_well_formed_it(&dir);
    std::fs::write(
        dir.join("tests").join("operator_example.rs"),
        "#[test]\nfn it_works() {}\n",
    )
    .expect("writes the private file");
    std::fs::create_dir_all(dir.join("tests").join("snapshots")).expect("creates tests/snapshots/");
    std::fs::write(
        dir.join("tests")
            .join("snapshots")
            .join("operator_example__case.snap"),
        "snap\n",
    )
    .expect("writes the private snapshot");

    assert_eq!(check_crate(&dir, "willikins-cli", false), Vec::new());
}

/// Rule 2: a `tests/it/*.rs` file main.rs does not declare with a `mod`
/// line silently stops compiling. A commented-out `// mod b;` must not
/// count as a declaration.
#[test]
fn rule_2_fires_on_an_undeclared_module() {
    let dir = scratch("undeclared_module");
    write_well_formed_it(&dir);
    std::fs::write(
        dir.join("tests").join("it").join("main.rs"),
        "mod a;\n// mod b;\n",
    )
    .expect("rewrites main.rs");
    std::fs::write(
        dir.join("tests").join("it").join("b.rs"),
        "#[test]\nfn it_works() {}\n",
    )
    .expect("writes the undeclared file");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::UndeclaredModule,
            detail: "willikins-example: tests/it/b.rs has no `mod b;` line in main.rs".to_string(),
        }]
    );
}

#[test]
fn rule_2_fires_on_a_missing_main_rs() {
    let dir = scratch("missing_main");
    std::fs::create_dir_all(dir.join("tests").join("it"))
        .expect("creates tests/it/ with no main.rs");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::UndeclaredModule,
            detail: "willikins-example: tests/it/main.rs is missing".to_string(),
        }]
    );
}

/// Rule 3: a leftover `tests/snapshots/` entry, in a crate that is not
/// `willikins-cli`, is never allowed -- even one shaped like a private
/// name, since that exemption is `willikins-cli`-only.
#[test]
fn rule_3_fires_on_a_leftover_snapshot_directory() {
    let dir = scratch("leftover_snapshot_dir");
    write_well_formed_it(&dir);
    std::fs::create_dir_all(dir.join("tests").join("snapshots")).expect("creates tests/snapshots/");
    std::fs::write(
        dir.join("tests").join("snapshots").join("a__case.snap"),
        "snap\n",
    )
    .expect("writes the leftover snapshot");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::LeftoverSnapshotDir,
            detail: "willikins-example: leftover tests/snapshots/a__case.snap".to_string(),
        }]
    );
}

/// Rule 4: a snapshot under `tests/it/snapshots/` whose module segment
/// names no module `main.rs` declares -- the orphan a later module
/// rename can leave behind.
#[test]
fn rule_4_fires_on_an_orphan_snapshot() {
    let dir = scratch("orphan_snapshot");
    write_well_formed_it(&dir);
    std::fs::create_dir_all(dir.join("tests").join("it").join("snapshots"))
        .expect("creates tests/it/snapshots/");
    std::fs::write(
        dir.join("tests")
            .join("it")
            .join("snapshots")
            .join("it__gone__case.snap"),
        "snap\n",
    )
    .expect("writes the orphan snapshot");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::OrphanSnapshot,
            detail: "willikins-example: tests/it/snapshots/it__gone__case.snap names no module main.rs declares"
                .to_string(),
        }]
    );
}

/// Rule 4 must not false-positive on a module whose name is a prefix of
/// another declared module's name (`a` vs. `ab`): matching requires the
/// full `it__<m>__` separator, not a bare `starts_with` on the module
/// name alone.
#[test]
fn rule_4_does_not_confuse_a_module_name_prefix() {
    let dir = scratch("orphan_snapshot_prefix");
    let it_dir = dir.join("tests").join("it");
    std::fs::create_dir_all(&it_dir).expect("creates tests/it/");
    std::fs::write(it_dir.join("main.rs"), "mod a;\n").expect("writes main.rs");
    std::fs::write(it_dir.join("a.rs"), "#[test]\nfn it_works() {}\n").expect("writes a.rs");
    std::fs::create_dir_all(it_dir.join("snapshots")).expect("creates tests/it/snapshots/");
    std::fs::write(it_dir.join("snapshots").join("it__ab__case.snap"), "snap\n")
        .expect("writes a snapshot for an undeclared module `ab`");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::OrphanSnapshot,
            detail: "willikins-example: tests/it/snapshots/it__ab__case.snap names no module main.rs declares"
                .to_string(),
        }]
    );
}

/// Rule 5: a leftover `tests/*.proptest-regressions` file, left in the
/// old location proptest's `SourceParallel` walk no longer reaches once
/// `tests/it/main.rs` exists (decision (d8)).
#[test]
fn rule_5_fires_on_a_leftover_proptest_regressions_file() {
    let dir = scratch("leftover_proptest_regressions");
    write_well_formed_it(&dir);
    std::fs::write(
        dir.join("tests")
            .join("ensure_properties.proptest-regressions"),
        "# seed\n",
    )
    .expect("writes the leftover seed file");

    let violations = check_crate(&dir, "willikins-example", false);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::LeftoverProptestRegressions,
            detail: "willikins-example: leftover tests/ensure_properties.proptest-regressions"
                .to_string(),
        }]
    );
}

/// The ratchet's own direction: a crate still named in `PENDING` must
/// have no `tests/it/` yet, so a half-done move cannot hide behind the
/// list.
#[test]
fn the_pending_direction_fires_when_tests_it_already_exists() {
    let dir = scratch("pending_already_moved");
    write_well_formed_it(&dir);

    let violations = check_crate(&dir, "willikins-example", true);
    assert_eq!(
        violations,
        vec![Violation {
            rule: Rule::PendingAlreadyMoved,
            detail: "willikins-example: tests/it/ exists but the crate is still in PENDING"
                .to_string(),
        }]
    );
}

/// The real tree, walked the same way T4-T14 will ratchet it: every
/// `crates/*/` package with a `tests/` directory is checked against
/// [`check_crate`] with `pending` read from [`PENDING`]. Green today
/// means `willikins-dsl` and `willikins-core` (T1, T2, already moved)
/// satisfy the five rules, and every crate still in [`PENDING`] has no
/// `tests/it/` yet.
///
/// Two anchors keep this from passing vacuously if `crates_root` ever
/// resolved to the wrong directory or the `tests/`-dir filter excluded
/// everything: `willikins-core` itself must be among the crates checked
/// (this very binary is built from its `tests/`), and every entry of
/// [`PENDING`] must be visited too -- both survive T14 emptying the
/// list, since an empty `PENDING` still requires `willikins-core` to
/// have been visited.
#[test]
fn the_real_workspace_tree_satisfies_the_layout_guard() {
    let mut visited = Vec::new();
    let mut all_violations = Vec::new();

    for entry in std::fs::read_dir(crates_root()).expect("crates/ is readable") {
        let entry = entry.expect("readable directory entry");
        let path = entry.path();
        if !path.is_dir() || !path.join("tests").is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .expect("a crate directory has a name")
            .to_string_lossy()
            .into_owned();
        let pending = PENDING.contains(&name.as_str());
        let violations = check_crate(&path, &name, pending);
        if !violations.is_empty() {
            all_violations.push((name.clone(), violations));
        }
        visited.push(name);
    }

    assert!(
        visited.iter().any(|n| n == "willikins-core"),
        "the walk never visited willikins-core itself; crates_root() is probably wrong: {visited:?}"
    );
    for pending_crate in PENDING {
        assert!(
            visited.contains(&pending_crate.to_string()),
            "PENDING names {pending_crate}, but the walk never visited it: {visited:?}"
        );
    }
    assert!(all_violations.is_empty(), "{all_violations:#?}");
}
