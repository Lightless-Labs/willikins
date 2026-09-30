# Milestone 3e: adversarial pass 5 — K1 (Buildkite credential port), L1 (credentials only where unbound), D1 (base-config gate wired into Sample), D2 (Sample's Buildkite token from Doppler)

**Date:** 2026-09-30
**Task:** the independent attacker's pass over the four lanes that landed on `main` for
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md` after pass 4
(`docs/research/2026-09-29-m3e-adversarial-pass-4.md`). The attacker wrote none of it.
**Subject:** K1 `052de07`, `b02f3c1`, `5300ac1`; L1 `9ac3ef7`, `4c8921e`, `557331c`; D1 `31e436f`, `794bae1`;
D2 `404e31f`, `25b9ac3`, `1d1783a`, `07f72e9`.
**Method:** read the K1/L1/D1/D2 addenda and the landed code, then attack through tests, the built binary
and mutations. Every mutation followed the same steps: copy the file into the pass's scratchpad
(`cp -p`), apply one exact-text replacement (a script asserted it occurred exactly once), run the
narrowest test target with `--no-fail-fast`, copy the saved file back, `touch` it, and confirm byte
identity with `cmp` (exit 0 every time) and a clean `git status`. Never `git checkout`, `reset` or
`stash`. The characterization ran with `INSTA_UPDATE=no`, and no `.snap.new` was left behind.
**No live test ran and no provider was called.** Every result comes from mocks, the in-memory fake,
or a subprocess with a cleared environment and a shaped, fake Doppler token. Test tokens are named by
their constant and never spelled out, because `secret_literal_guard.rs` scans docs too.

## 0. The state the pass started from

`main` at `07f72e9`, clean apart from `goal.txt` and the host-maintenance todo (both left alone).
Nothing was uncommitted or red. Baseline: `cargo test -p willikins-cli --test sample_document --test
serve_and_live --test sample_apply_blocked_redaction` passed 4 + 16 + 1, 0 failed.

## 1. The questions, and what the evidence says

### Does a missing or non-inheritable base config stop Sample before any write? It stops `inherit`, and only `inherit`. The gate can also be bypassed by overriding the input

**What the gate does (confirmed, mutation M3).** `base_config_gate` expands over `inputs.base_configs`,
and `inherit.inherits` binds the gate's aggregate. **M3** rebound `inherit.inherits` to
`${{ inputs.base_configs }}` (the pre-D1 shape). `a_missing_base_config_blocks_its_gate_and_skips_inherit`
failed at `sample_document.rs:944` (`left: Create, right: Skip`: without the gate edge, `inherit`
plans a write against a config that does not exist). `the_document_reads_the_real_layout_by_name`
failed at `:868`. Restored, `cmp` 0.

**It does not stop Sample "before any write".** Decision (j) holds back only a blocked gate's data
dependents. A plan of the real document against the fixture state with every base config removed
(`plan --fake-state <scratch copy> ...`, defaults for `base_configs`) blocked all three
`base_config_gate` instances, each with `holds_back: ["inherit"]`, and `inherit` planned `skip`. Every
other writer still planned `create`: the three bundle identifiers, `healthkit`, the `doppler`
project, all three `configs` instances, `prd_config`, and `pipeline`. The three `doppler.secret.set`
nodes planned `skip`, but the app-group gates caused that (unmet in this state), not the base
configs. They bind `config` from `prd_config`, never from `inherit`. The only write the base-config
gate prevents is the one inheritance write. D1's own test asserts exactly this, so it is the designed behaviour.
The brief's question implies the whole run halts, and it does not.

**Finding F2: an override of the input bypasses the gate entirely (recorded, not fixed).**
`base_configs` is still a caller-overridable input (D1 kept it one on purpose, because the document
format cannot bind a list literal). `RawInput::from_comma_separated` drops empty items, so
`--input base_configs=` is an empty list. The same scratch-state plan with that one extra argument
exits 0: there are **zero** `base_config_gate` instances, no base-config entry in `blocked`, and
`inherit` plans `noop` with `inherits: []`. Sample would provision `prd_deployment_ios` with no shared
base configs, and nothing would block or report it. A partial or substituted list works the same way:
`--input base_configs=github/lightless-labs` gates only that config (it is checked for existence and
for being inheritable, and nothing else). So a caller who can supply inputs can have Sample's
production config inherit a different, existing, inheritable config's secrets. D1 disclosed that the
input can be overridden, but it did not say that the override defeats the gate D1 had just added.
What limits the damage today: a plan that writes needs approval, and the approver sees the
`inherits` list. Closing it takes a document-format change (a list literal for `for_each` or `with:`,
or a "fixed" marker on a defaulted input), which is a design decision and too large for an attacker
pass on this host. It goes to the coordinator.

### Does planning or applying Sample need any provider credential in the environment besides the Doppler token? Not for `plan <file>` / `apply <file>`. `--plan-id` and `serve` still need every credential

**Confirmed, mutation M2.** **M2** removed `token:` from `pipeline` alone and left
`buildkite_cluster` bound. Both `plan_live_on_sample_needs_neither_github_nor_buildkite_credential`
and `apply_live_on_sample_...` failed with exit 2, and `the_document_reads_the_real_layout_by_name`
failed at `:810`. The built binary, run with only a shaped Doppler token, printed this refusal:

```
{"kind":"Buildkite","error":"environment variable `WILLIKINS_BUILDKITE_TOKEN` is not set",
 "document":"sample-ios-app","node":"pipeline","tool":"buildkite.pipeline.ensure", ...}
```

The refusal names the variable. It also names the second Buildkite node, the one left unbound, not
the first one. On the restored document (`cmp` 0) the same probe exits 1 with only the missing inputs
listed (`app_identifier` first) and nothing about GitHub or Buildkite.
`signoz_http_from_env`/`credential_from_env` check shape only and make no network call. App Store
Connect tools are inserted with no environment gate.

**Finding F1: the Buildkite half of L1's "still refuses when unbound" had no test (fixed,
`753d1e8`).** GitHub has a subprocess control: `new-rust-service.yaml` refuses with exit 2, naming the
node. Buildkite had none. `live_catalog_for_document` is called only from the CLI, and
`willikins-server`'s unit tests cover `first_unbound_node_for` but never `live_catalog_for_document`
itself. **Mutation M5** replaced the Buildkite arm's `match first_unbound_node_for(document,
Provider::Buildkite)` with `match None::<…>`, so the Buildkite environment credential was never
required and a credential-less client was built even for unbound nodes. It **survived**
`serve_and_live` (16/16), `sample_document` (4/4), `acceptance_m3a_buildkite` (11/11) and
`willikins-server --lib` (73/73). With the mutant a document that leaves the port unbound still fails,
but only later, inside a node's `read()` (`Http::run_retrying`'s local refusal), and not with the up-front,
node-naming refusal L1 promises. The new test
`plan_live_on_a_document_leaving_buildkite_unbound_still_refuses_naming_the_variable` runs `plan
workflows/new-rust-service-buildkite.yaml --live` with only the Doppler token and a short, shaped
GitHub token. It asserts exit 2 with `kind: Buildkite`, `node: buildkite_cluster`,
`tool: buildkite.cluster.get`, and an `error` that names `WILLIKINS_BUILDKITE_TOKEN`. The test was
written first and run against M5, where it failed at `serve_and_live.rs:524` (`left: 1, right: 2`)
while the other 16 passed. It passes on the restored tree (`cmp` 0). `cargo fmt --all --check`,
`cargo clippy -p willikins-cli --all-targets -D warnings` and `secret_literal_guard` are all green.

**Finding F4: the two-step apply still needs every credential (recorded, L1's stated boundary).**
`apply --plan-id --live` goes through `build_catalog` → `live_catalog_from_env`, and so does `serve`.
Both need `WILLIKINS_GITHUB_TOKEN`, `WILLIKINS_DOPPLER_TOKEN`, `WILLIKINS_BUILDKITE_TOKEN`, and the
`SigNoz` credential plus host. The `SigNoz` check is shape-only, but the operator's key expired on
2026-09-23. For the operator's real Sample run, the only path that needs just the Doppler token is the
one-shot form: `apply workflows/sample-ios-app.yaml --live --approve --journal <path>`. That form
(`cmd_apply_file` → `build_catalog_for_document`, self-approving) takes the one-document route; the
code was read and D2's subprocess test proves catalog construction. A `plan` → `approve` → `apply
--plan-id` sequence does not take it. Also, `ApplyArgs::live`'s doc comment says `--plan-id` "still
requires all three", but it requires four (`SigNoz` too). That comment is stale; this pass did not
edit it.

### Are Sample's base configs, org, slug, monorepo, Buildkite org and cluster literals? Five of six are, and `base_configs` is not

**Confirmed, mutation M4.** **M4** reintroduced `buildkite_org` as a defaulted input and bound
`pipeline.org` to it. `the_document_reads_the_real_layout_by_name` failed at `:722`
(`` `pipeline.org` must be a literal, got Some(Input(InputName("buildkite_org"))) ``), and the two
scenario tests failed with `workflow input buildkite_org was not supplied`. Restored, `cmp` 0. The test
pins `names.org` = `Example-Org`, `names.slug` = `sample`, `monorepo_ref.repo` =
`Example-Org/monorepo`, `org` = `example-bk-org` on both Buildkite nodes and
`buildkite_cluster.name` = `Default cluster` as literals, and it pins that none of them is a declared
input. `base_configs` is still an input with the right default. See F2 for what that allows.

### Does Sample read exactly `PIPELINE_CREATION_TOKEN` from `buildkite/prd`, never the environment? Yes

**Confirmed, mutation M1.** **M1** renamed the secret to another name.
`the_document_reads_the_real_layout_by_name` failed at `:789` (`left: "PIPELINE_OTHER_TOKEN"`), both
scenario tests failed (the seeded secret was not found), and `sample_apply_blocked_redaction` failed
(exit 1 instead of 3). Restored, `cmp` 0. The same test pins `bk_token_secret`'s tool
(`doppler.secret.get`), which is what would catch a swap to an `env.get` of the environment variable.
It also pins `config: buildkite/prd`, `bk_token` = `buildkite.token.parse` over `bk_token_secret.value`,
and exactly two Buildkite provider nodes, both binding `token` from `bk_token.value`.

### Can the Buildkite token reach a non-secret port, a plan, the journal, an error or a log? No

These are `validate` runs of the built binary on scratch variants of
`buildkite-cluster-token-from-doppler.yaml`. The exact errors:

| Variant | Result |
|---|---|
| `token.value` → `cluster_ref.name` | `SecretToNonSecretSink` … `flows into non-secret sink at cluster_ref.name` |
| → `naming.v1`'s `slug` | `SecretToNonSecretSink` … `at names.slug` |
| → `buildkite.pipeline.ensure`'s `slug` | `SecretToNonSecretSink` … `at pipe.slug` |
| a literal token on `token` | `SecretLiteral`: `a literal cannot supply a secret value` |
| a workflow input of type `BuildkiteToken` | `SecretWorkflowInput` |
| the unparsed `DopplerSecretValue` on `token` | `TypeMismatch`: `expected BuildkiteToken, found DopplerSecretValue` |
| the token as a document **output** | accepted: a secret output is allowed by design (`check.rs`'s `check_outputs`) |

The one accepted variant got the end-to-end attempt. It was planned and then applied (`--approve`,
file journal) against a scratch state holding a real-shaped token assembled at runtime. The output
rendered as `[REDACTED BuildkiteToken]` with `redacted: true` in the plan, the apply stdout and the
journal. The token's distinctive run appears **0** times across plan stdout, apply stdout, stderr and
the 13-line journal. This supplies the CLI- and journal-level proof for a bound `BuildkiteToken` that
K1 said was only inherited. D2 then gave Sample a bound Buildkite token, and
`sample_apply_blocked_redaction`'s leak assertion now covers it too.

Errors: the derive's parse checks (`willikins-derive/src/codegen.rs`, `checks`) say what the constraint
is and never quote the value. `buildkite.token.parse` wraps only `err.reason`, and
`refuses_a_non_token_value_with_a_clear_error_and_no_content` pins this.
`BuildkiteToken::reveal_for_authorization` is reached only from `client_for_token`, which builds a
bearer `Credential` against the default client's own base URL (`client_for_token_preserves_the_default_clients_base_url`).
No tool takes a URL.

### Does an unbound Buildkite port still fall back to the environment exactly as before? Yes, at both levels

**Tool level, mutations M6 and M7.** **M6** made `buildkite.cluster.get` ignore its bound token
(`token.as_ref().filter(|_| false)`). `read_authorizes_with_the_bound_token_port_not_the_default_credential`
and `a_bound_token_port_is_used_even_when_the_default_credential_would_be_refused` failed, and
`unbound_read_authorizes_with_the_tools_own_default_credential` stayed green (8 passed, 2 failed).
**M7** made `ScopedClient::default_for`'s `None` arm authorize with a fixed, assembled token instead of
the tool's own default client. `unbound_read_authorizes_with_the_tools_own_default_credential` failed
in **both** `cluster_get_mock` and `pipeline_ensure_mock` (9+17 passed, 1+1 failed). This is the gap
pass 4 found for GitHub (its finding 3), and it is closed for Buildkite. Both files were restored with
`cmp` 0, then both suites re-ran green (10 and 18).

**Catalog level:** F1 above.

### Does every existing document plan byte-identically? Yes. Sample's own entry grew because Sample was edited in place

`git diff 37136a2..HEAD` of the characterization snapshot (the whole span K1–D2) is **21 insertions,
0 deletions**:

- one new `=== workflows/buildkite-cluster-token-from-doppler.yaml ===` block (14 lines);
- six `TYPES:` lines inside Sample's own entry (`base_config_gate.config`, `bk_token_secret.config`,
  `.name`, `bk_token.value`, `buildkite_cluster.token`, `pipeline.token`);
- insta's own `assertion_line: 284` metadata line, which previous commits have added and dropped
  more than once (`ffc418d`, `93caebc`, `5300ac1`). It is noise, not document content.

No line of any other document's block moved, and no `PLAN` line moved anywhere. Sample's `PLAN`
section is still `PLAN ERROR: node issuer_id_text: NotFound` against the empty catalog, which also
means the characterization cannot show whether Sample's own plan changed. `sample_document.rs` covers
that. `cargo test -p willikins-dsl --test acceptance` (`INSTA_UPDATE=no`) is green, 5 passed.

## 2. Mutation ledger

| # | File | Mutation | Killed by | Restored |
|---|---|---|---|---|
| M1 | `workflows/sample-ios-app.yaml` | `PIPELINE_CREATION_TOKEN` → another name | `the_document_reads_the_real_layout_by_name` + 2 scenario tests + `sample_apply_blocked_redaction` | `cmp` 0 |
| M2 | same | drop `pipeline.token` only | both Sample `serve_and_live` tests (exit 2, `node: pipeline`) + literal-pinning test | `cmp` 0 |
| M3 | same | `inherit.inherits` → `inputs.base_configs` | `a_missing_base_config_...` (`Create` vs `Skip`) + literal-pinning test | `cmp` 0 |
| M4 | same | `buildkite_org` back as an input on `pipeline.org` | literal-pinning test + 2 scenario tests | `cmp` 0 |
| M5 | `crates/willikins-server/src/catalog.rs` | never require the Buildkite env credential | **survived** everything; killed only by the new test (`753d1e8`) | `cmp` 0 |
| M6 | `crates/willikins-providers-buildkite/src/tools/cluster_get.rs` | ignore the bound token | 2 bound-token mock tests | `cmp` 0 |
| M7 | `crates/willikins-providers-buildkite/src/client.rs` | unbound arm authorizes with a fixed token | `unbound_read_...` in both mock suites | `cmp` 0 |

## 3. Commits

- `753d1e8` Pin that a Buildkite-unbound document still refuses live up front (F1).
- This record and the plan addendum.

## 4. Carried forward (not fixed by this pass)

1. **F2:** `base_configs` can be overridden, so the gate can be emptied or redirected. Closing it needs a
   document-format decision (a list literal for `for_each`/`with:`, or a fixed-input marker).
2. **F4:** `apply --plan-id --live` and `serve` need all four provider credentials, including `SigNoz`'s
   expired key. For the operator's Sample run, the one-shot `apply <file> --live --approve --journal`
   is the only path that needs just the Doppler token. `ApplyArgs::live`'s doc comment says "three"
   where four are required.
3. Buildkite scope sufficiency (`read_pipelines`/`write_pipelines`/`read_clusters`) is still a static
   reading of the client, not a live proof (D2's own caveat).
4. The characterization's Sample `PLAN` section stops at the first node against the empty catalog, so
   it cannot catch a change in Sample's own plan. `sample_document.rs` is the real guard.
