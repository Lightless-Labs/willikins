# Milestone 3e: adversarial pass 4 — R1 (Doppler name grammar), R2 (GitHub credential port), R3 (base-config gate), R4 (Sample on the real layout)

**Date:** 2026-09-29
**Task:** the independent attacker's pass over the four R lanes that landed on `main` for
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md` after pass 3
(`docs/research/2026-09-29-m3e-adversarial-pass-3.md`), written by nobody who wrote any of it.
**Subject:** R1 `c74ad8f`, `9785280`, `db0836c`, `59b4229`; R2 `63bec99`, `ad8c190`; R3 `0810174`, `e789f37`;
R4 `93caebc`.
**Method:** read the plan's R1–R4 addenda and every landed file; attack through tests. Every mutation was
made by a script in the pass's scratchpad (`mutate.py`, `sample_mut.py`) that copied the file aside, applied
one exact-text replacement (asserting it occurs exactly once), ran the narrowest test target, copied the
saved file back and touched it, then confirmed byte identity with `filecmp` and with `cmp` (exit 0 every
time), and with an empty `git diff --stat` on the file (never `git checkout`, `reset` or `stash`). The
characterization runs under a mutated document used `INSTA_UPDATE=no`, and no `.snap.new` was left behind.
**No live test ran and no provider was called**: every result is against mocks or the in-memory fake.
Test markers are named by their constant, never spelled out (`secret_literal_guard.rs` scans docs too).

## 0. The state the pass started from

`main` at `93caebc`, clean apart from `goal.txt` and the host-maintenance todo (both left alone). Nothing
was uncommitted; the baseline `cargo test -p willikins-types --lib` was green (419 passed).

## 1. The questions, and what the evidence says

### Does the widened grammar admit anything Doppler would refuse, or smuggle a path? Not a path; but nothing said so (gap closed, `e00d8f3`)

`DopplerConfigName` is interpolated **unescaped** into every Doppler query string
(`crates/willikins-providers-doppler/src/client.rs`: `?project={project}&config={name}`, and the token and
secret paths alike), so the type's grammar is the only guard against a smuggled `&project=…`, a `#`, a
`?`, a `/` or a `..`. `[a-z0-9_-]+` (anchored by the derive) admits none of them, and `DopplerConfig::parse`
still requires exactly one `/` and sends the name half through `DopplerConfigName::parse`. But **mutation A**
— widening the name's pattern *and* `DOPPLER_CONFIG_PATTERN` to also admit `& = ? # . / % + ;`, space and
newline — left all 419 existing lib tests green. R1 widened by one character and no test pinned that it
widened by only that one. Two tests now refuse each metacharacter on the bare name and through the combined
identity; both were red under mutation A (421 run, 2 failed, nothing else) and green after restoring.

What the grammar admits that nobody has seen Doppler accept: `-`, `--`, `-prd`, `prd-`. The probe proved two
strings (`prd_example-org`, `example-org`), not these edge shapes. None of them is a URL hazard; whether
Doppler refuses them is a verify item (§4), not a finding.

### Can the GitHub token reach a non-secret port, a plan, the journal, an error or a log? No — but the one tool Sample uses had no proof (gap closed, `72659fe`)

- **Ports.** `GitHubToken` is `secret`; `github.token.parse` outputs it; the three tools' `token` port is
  `exact("GitHubToken", false)` (the `false` is `required`). `check` refuses a secret into a non-secret port
  and the registry refuses a secret type for any literal or input, so the token can only arrive from a
  resolver chain. The derive's rejection reason names the constraint, never the value, and
  `github.token.parse` reports only `err.reason`.
- **Headers.** The grammar `(?:github_pat_|ghp_)[A-Za-z0-9_]+` excludes CR/LF, so a bound token cannot
  inject a header. `Http::with_credential` clones the default headers (Accept, API version, User-Agent) and
  swaps only the credential: mutation **C** below proves the bound token is what authorizes.
- **Errors.** **Mutation E** (write the bound token into `github.repo.ensure`'s `read` error message) is
  killed by `redaction.rs`'s `a_bound_token_port_marker_reaches_no_observation_or_error`. **Mutation E2**, the
  same leak in `github.repo.get`'s `lookup` — the one GitHub node Sample binds `token` on — **survived every
  test in the crate** (lib, `redaction`, `repo_get_mock`, `github_token_documents`, `fake_agrees_with_live`):
  R2's redaction proofs only drive `repo.ensure`. New test
  `a_bound_token_port_marker_reaches_no_repo_get_error` reads `github.repo.get` with `PORT_TOKEN_MARKER` bound
  against a 404, a 401 and a 500 and asserts the marker is in neither the error's `Debug` nor its `message`;
  red under E2 (the 404 case printed the marker in a `NotFound`), green restored.
- **Plan, journal, CLI output.** `sample_document.rs` (three runs, plan and applied JSON) and
  `sample_apply_blocked_redaction.rs` (stdout, stderr, the journal file) assert the seeded token's raw value
  never appears; both green on the committed tree.

### Does an unbound credential port still fall back to the environment exactly as before? Yes, proven both ways, one direction thinly

`ScopedClient::default_for` is the only switch. **Mutation C** (a bound token ignored, the default used) is
killed in all five targets run: the lib's `scoped_client_default_for_builds_a_fresh_client_with_a_bound_token`,
`repo_get_mock` (two tests), `repo_ensure_mock`, `actions_secret_ensure_mock` and `redaction`. **Mutation D**
(an unbound port authorizes with a fixed bogus token instead of the default credential) is killed by exactly
two tests — the lib's `scoped_client_default_for_borrows_the_default_without_a_bound_token` and `redaction`'s
`a_marker_credential_reaches_no_header_but_authorization` — while all three tool-level mock suites stayed
green: no tool mock pins that an unbound request carries the environment credential's value (finding 3). The
server's credential logic is untouched since R1 (`git diff 59b4229..HEAD -- crates/willikins-server/src/catalog.rs`
shows only tool-name additions), so `WILLIKINS_GITHUB_TOKEN` is read exactly as before.

### Does every existing document plan byte-identically? Yes

The characterization snapshot moved since the handoff (`6b81bac..HEAD`) only by the new
`github-repo-token-from-doppler.yaml` block and Sample's own entry (read line by line), and
`cargo test -p willikins-dsl --test acceptance` is green on the committed tree. **Fingerprints cannot move
either:** `InstanceFingerprint` (`crates/willikins-core/src/plan.rs`) holds a node's name, instance, action and
rendered outputs only, never a `ToolSpec`, so the three GitHub tools' new optional input port changes no
existing document's fingerprint; the server's approval check is the document's own SHA-256, which only
Sample's changed.

### Does a missing or non-inheritable base config stop before any write? At the tool level yes; **for Sample, no** (finding 1)

**Mutations B1** (`inheritable == Some(true)` → `!= Some(false)`) and **B2** (a genuine provider failure read
as `Absent`), applied together in one run and attributed by test name: B1 is killed by
`read_reports_absent_when_inheritable_is_never_mentioned`, B2 by
`read_propagates_a_genuine_provider_failure_rather_than_blocking` (12 run, exactly those 2 failed). The gate
is sound. But it is not wired into `workflows/sample-ios-app.yaml`: `inherit` still binds
`${{ inputs.base_configs }}` directly. The evidence is already in the tree: `sample_document.rs`'s
`seeded_state()` seeds no Doppler config at all, run 1's blocked set is asserted equal to exactly the six
gates (so `inherit` was not blocked), and run 3 applies clean — `inherit` planned and applied against three
base configs that do not exist in the fake. A real apply of Sample today would create project `sample` and its
configs and branch config before `inherit` failed on a missing base config.

### Does the Sample document read exactly the real names from the real configs, and the GitHub token from `github/example-org`? Yes; now pinned against the document itself (gap closed, `4a3d0dd`)

Every Sample test supplies `base_configs` explicitly and seeds its fake state under the same names the
document reads, so the document's own choices were checked only indirectly. Five document mutations, each run
against `sample_document`, `sample_apply_blocked_redaction` and the characterization suite:

| Mutation | Caught before this pass by | Caught by the new layout test |
|---|---|---|
| G: `base_configs` default `github/example-org` → `github/example_org` | **nothing** | yes |
| F: `gh_token_secret.config` → `github/example_org` | both Sample scenario tests | yes |
| H: `gh_token_secret.name` → `GITHUB_TOKEN` | both Sample scenario tests | yes |
| I: `monorepo_ref`'s `token` binding removed (falls back to the environment) | only the characterization **snapshot** | yes |
| J: `key_id_text.name` → `ASC_API_KEY_ID` | both Sample scenario tests | yes |

`the_document_reads_the_real_layout_by_name` asserts: the three App Store Connect reads name
`appstore-connect/deploy_ios` and `APP_STORE_CONNECT_API_KEY_ISSUER_ID`/`_ID`/`_BASE64` with the right tools;
`gh_token_secret` is `doppler.secret.get` of `github/example-org#GH_CLONE_TOKEN`; `gh_token` is
`github.token.parse` bound from it; the document's only GitHub provider node is `monorepo_ref` and its `token`
binds `steps.gh_token.value`; and `base_configs` defaults to the three real base configs in order. Green on the
committed document.

The graph resolves the GitHub token from Doppler; the process still does not (R2's gap, finding 2).

## 2. Findings

1. **Sample does not stop on a missing or non-inheritable base config** (evidence above). The fix is R3's own
   recorded wiring: one `doppler.config.inheritable.gate` per `base_configs` entry, `inherit.inherits` rebound
   from the gate's aggregated output. Not done here: it moves Sample's characterization entry, which this
   pass's boundary forbids. **Open, the coordinator's.**
2. **The live server still reads `WILLIKINS_GITHUB_TOKEN` for any document naming a `github.*` tool**, Sample
   included, although Sample's graph never uses it. R2's recorded gap, unchanged. **Open.**
3. **No tool-level mock pins the unbound path's credential** (mutation D survived all three mock suites). The
   lib unit test and one redaction test do kill it, so the behaviour is pinned, thinly. Not fixed; low.
4. **`github.repo.get`'s bound-token error path had no redaction proof** (mutation E2). **Fixed, `72659fe`.**
5. **Nothing pinned that R1 widened the grammar by only one character** (mutation A). **Fixed, `e00d8f3`.**
6. **Nothing pinned Sample's `base_configs` default; only a snapshot pinned its token binding** (mutations G,
   I). **Fixed, `4a3d0dd`.**
7. **The operator's names are still caller-overridable inputs.** `base_configs`, `org`, `slug` and `monorepo`
   are inputs with defaults, against the operator's "the document's names are the policy, not inputs" (and
   `buildkite_org`/`cluster` have no value in the document at all). Editing them into literals moves Sample's
   characterization entry; not done here. **Open.**
8. **The sandbox and the real layout have diverged.** Every other App Store Connect document and fixture
   (`appstore-*-from-doppler.yaml`, `apple-signing-credential-*.yaml`, the appstore live tests, the fake's
   `doppler_value_get`) still reads `ASC_API_KEY_*`; Sample alone reads the real names, so no sandbox dry run of
   Sample can resolve its credential until the operator reseeds or retires the sandbox names. Known (R4's
   header); recorded so it is not mistaken for a defect in either path.

## 3. Mutations

| # | File | Mutation | Result |
|---|---|---|---|
| A | `willikins-types/src/doppler.rs` | name and combined patterns admit `& = ? # . / % + ;`, space, newline | survived 419 lib tests; killed by the 2 new tests |
| B1+B2 | `willikins-providers-doppler/src/tools/config_inheritable_gate.rs` | omitted `inheritable` reads Present; a 500 reads Absent | killed, one test each |
| C | `willikins-providers-github/src/client.rs` | a bound token is ignored | killed in 5 of 5 targets |
| D | same | an unbound port uses a bogus token, not the default | killed by 2 tests; 3 mock suites survived |
| E | `…/tools/repo_ensure.rs` | bound token written into `read`'s error | killed by `redaction` |
| E2 | `…/tools/repo_get.rs` | bound token written into `lookup`'s error | survived the crate; killed by the new test |
| F, G, H, I, J | `workflows/sample-ios-app.yaml` | see the table in §1 | G survived, I snapshot-only before; all killed by the new test |

Every file was restored from its saved copy, touched, and `cmp`'d byte-identical.

## 4. Verify items

- **`GH_CLONE_TOKEN`'s shape.** `GitHubToken` accepts only `ghp_` and `github_pat_`. If the real
  `github/example-org#GH_CLONE_TOKEN` is any other kind (an app installation token, an OAuth token), Sample's
  `gh_token` fails at plan. Nobody may read the secret to check; the operator can by eye.
- **Hyphen edge shapes.** Whether Doppler refuses a config name that is `-`, starts or ends with a hyphen, or
  doubles one. The grammar admits all four; one live create per shape in the sandbox would settle it.

## 5. Not settled, for the coordinator

Findings 1, 2, 7 and 8, in that order. Finding 1 is the one that matters before a real apply.

## 6. Gate after the pass

Scoped only (the full workspace gate is the coordinator's): `cargo fmt --all --check`; `cargo clippy -p
willikins-types`, `-p willikins-providers-github`, `-p willikins-cli`, each `--all-targets -j 2 -- -D warnings`;
`cargo test -p willikins-types --lib` (421 passed); `cargo test -p willikins-providers-github` over `--lib`,
`redaction`, `repo_get_mock`, `repo_ensure_mock`, `actions_secret_ensure_mock`; `cargo test -p willikins-cli
--test sample_document --test sample_apply_blocked_redaction`; `cargo test -p willikins-dsl --test acceptance`;
`cargo check -p willikins-types -j 2`; `cargo test -p willikins-providers-doppler --lib config_inheritable_gate`;
`cargo test -p willikins-core --test secret_literal_guard` with this record and the plan addendum on disk. All
green on the committed tree.
