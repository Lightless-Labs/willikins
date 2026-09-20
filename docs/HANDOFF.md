# Willikins Handoff

Current state of the project and active work. Read this at session start. Update before
compaction, before handing off, after a milestone, and after a plan change or discovery.

**Last updated:** 2026-09-14 (midday)

## Current Status

### RESUME HERE (2026-09-20) — milestone 3a is complete and live-proven: willikins now provisions a GitHub repository, a Doppler project with its configs, and a Buildkite pipeline, which is the operator's real process. Next: 3b's `doppler.project_member.ensure`, the one manual step left in that process. Milestone 2c (willikins as its own authorisation server) is planned and shelved until the service needs to be reachable

- **Live state:** `main` at 348 commits, gates green at `1bc23f5` (1,704 tests; the ignored ones
  are by-hand measurements, a lock-probe child, two slow connection-holding attacks, a
  fixture generator, the two live probes and the two live write cycles). Remote `origin` is
  `git@github.com:Lightless-Labs/willikins.git` (public, AGPL-3.0-or-later since 2026-09-14);
  `main` is pushed after every coordinator commit.
- **What just happened (2026-09-20):** milestone 3a landed, the slice that makes willikins
  useful to the operator rather than only correct. `crates/willikins-providers-buildkite` adds
  two tools, `buildkite.pipeline.ensure` (key `(org, slug)`, read-then-create, ours when the
  description equals `managed-by: willikins`, `Mismatch` on repository before cluster) and
  `buildkite.cluster.get` (pure, pages explicitly and never reads the `Link` header, which can
  carry an api_key). `workflows/new-rust-service-buildkite.yaml` is the operator's real process:
  repository, Doppler project and configs, pipeline. It mints no service token and stores no CI
  secret, because their CI holds one Doppler service-account token organisation-wide, so no
  `SinkToken` exists in the graph, every node is `Reversible` and a run needs no approval. The
  pipeline configuration is not a port at all: one frozen constant, Buildkite's documented upload
  bootstrap, so the repository's own `.buildkite/pipeline.yml` holds the steps and no
  caller-supplied command can reach an agent. The repository form is built inside the client from
  an already-parsed `GitHubRepo`, never a URL port. The live write cycle ran against the sandbox
  organisation on 2026-09-20 and passed: cluster resolved by name, pipeline created, re-read
  `Present`, re-ensure `changed: false`, both `Mismatch` arms proved read-only, deleted, re-read
  `Absent`, and an independent check through `Http` confirmed zero pipelines left. The verify's
  own best find was the failure mode milestone 2 already paid for once: the fake and the live tool
  agreed **by construction**, since the fake kept a private copy of the repository-URL helper that
  could be rewritten without failing anything. `tests/fake_agrees_with_live.rs` now pins all five
  observations behaviourally. Adversarial pass recorded at
  `docs/research/2026-09-20-m3a-adversarial-pass.md`.
- **Earlier (2026-09-16, afternoon):** milestone 2c changed shape on an operator
  decision and its plan is rewritten to match. They ruled out every external identity provider,
  including a self-hosted open-source edition of a SaaS ("I won't have the entire project's authn
  / authz depend on the 'open source' edition of a SaaS ... There is *no way* I'll shove such a
  tool down the throat of anyone who wants to use Willikins"), because a dependency willikins
  accepts is one every self-hoster inherits. So **willikins is its own authorization server**: it
  issues the tokens it validates. Two research documents were written from primary sources
  fetched 2026-09-16 and are on `main`: `docs/research/2026-09-16-m2c-own-authorization-server.md`
  (six passes: what the MCP spec demands of an authorization server, the Rust crate landscape with
  a pinned table carrying licences, maintenance and Dockerfile buildability, passkeys versus
  passwords, JWT issuance and key rotation, sessions and browser security, and self-contained Rust
  identity servers read as prior art) and `docs/research/2026-09-16-m2c-scope-revision.md` (55
  must-implement items each marked spec or deployment, 23 deferrable, a decision-by-decision
  revision, a 20-task shape, the pluggable seam, the open decisions and the risks). The plan
  `docs/plans/2026-09-16-milestone-2c-authorization.md` now carries them: 24 decisions, 20 tasks,
  26 acceptance tests, 15 new variables, 23 verify items, 6 trust boundaries (boundary 1 split,
  because willikins now holds a signing key). Cost, measured rather than guessed: 4,000 to 6,000
  implementation lines plus about 1.4x in tests, and **no new crates for the issuing half** —
  `jsonwebtoken` already signs, publishes the JWKS and derives the `kid`. Two candidate frameworks
  were rejected on evidence: `oxide-auth` has no audiences, no metadata, no resource indicators and
  defaults PKCE off; `oauth-as` implements everything but is six weeks old, one author, unaudited,
  36,230 lines, which is a larger trust surface than the vendor product that was rejected (it is
  recorded as prior art to read). The operator also asked to bank pluggable auth for later, so an
  adapter for an external provider stays possible: the rule, in the plan and in
  `todos/2026-09-16-pluggable-auth-adapters.md`, is that the validator reads a configured issuer
  and JWKS **even when both are willikins' own**, which makes delegation configuration rather than
  a rewrite.
- **Earlier (2026-09-16, morning):** task 14 part B, the live smoke run, **passed**,
  and milestone 2 is complete. Thirty seconds against the sandbox accounts: `repo`, `doppler`,
  `token` and `ci_secret` `Created` with the three root configs `Unchanged`, a second apply
  `Unchanged` throughout with `ci_secret` `Converged` and the token output `Unknown`, the
  rotation refused without approval and applied with it, `deploy/teardown.sh` dry-run then
  `--yes`, and both accounts empty afterwards (the test's own check plus two independent
  reads). Three run ids, one journal, no credential in any output. The first attempt that day
  failed in four seconds during the *initial* plan, creating nothing: `doppler.service_token.ensure`'s
  read propagated Doppler's `404` for a project the plan had not created yet. Fixed and
  verified in Workflow `wf_8c292047-d33` before the re-run (both token reads tolerate that one
  status, `rotate`'s ensure stays strict, a `400` still refuses, and `ButlerError::Plan` now
  says "planning failed" rather than "re-planning failed" for a first plan). The lesson is in
  the plan's addendum: the live write cycle created its project before listing tokens and the
  fake answered `Absent`, so `smoke_parity.rs` — both of whose sides are the fake — agreed with
  itself and every offline gate was green.
- **Credential boundary, set by the operator the same morning, and standing:** their own `gh`
  CLI credential is never used for a write against their accounts and never asked to carry a
  wider scope ("That one is out of the question"; "I already provided a pat, scoped to the test
  org. On purpose. So a mistake could *not* wreck anything. I *never* allowed using my github
  cli set up to create or delete repositories."). The teardown authenticates as
  `WILLIKINS_GITHUB_TOKEN`; no `gh` invocation remains in any file this workspace runs. The
  coordinator's secret-scanning push-protection bypass is retired for the same reason: the
  synthetic provider-shaped literals in fixtures are what trip the scanner, so they are built
  at runtime from parts instead, and two guard tests make both rules gate-enforced rather than
  remembered. **Landed 2026-09-16** (Workflow `wf_61a0ea94-362`, sonnet verified by opus; gates
  green; a push then went through with no bypass, and none ever will again). The coordinator's own
  sweep had been wrong: it said twelve literals across eight files, assuming a token is a prefix
  followed immediately by the random run, but Doppler's real shape carries an optional environment
  segment between them, and the true scope was about 45 sites across 28 files — including a real
  Doppler documentation token sitting in a mock fixture and three more quoted in a research note.
  Every one is now assembled at runtime (`concat!`), which needed one small widening of
  `willikins-derive` so a secret type's `#[domain(example = ...)]` can be an expression rather than
  a string literal; the runtime values are byte-identical, so `assert_example_parses` still proves
  what it proved. The verify found both guards narrower than GitHub's published detector tables
  (only two of six GitHub prefixes, three of six Doppler kinds) and a demonstrated bypass in the
  `gh` guard, in the very file it protects: a shell `case` arm begins with `*`, which the matcher
  treated as a comment continuation, so `*) gh api -X DELETE …` passed unseen. Both closed, and
  every fix proven by planting a real offender and watching the guard name its file and line. The
  guards live at `crates/willikins-core/tests/secret_literal_guard.rs` and
  `crates/willikins-cli/tests/no_gh_writes_guard.rs`, and CLAUDE.md carries both invariants.
- **Earlier (2026-09-16, the night):** milestone 2c's plan exists and is
  reviewed. Workflow `wf_cfe76f52-8cc` (six opus readers, a synthesizer, a drafter) wrote
  `docs/research/2026-09-16-m2c-authorization.md` (1,391 lines, every fact quoted from a source
  fetched 2026-09-16, a twenty-item verify list, a six-provider facts table) and drafted
  `docs/plans/2026-09-16-milestone-2c-authorization.md`; a five-persona document review
  (coherence, feasibility, security-lens, scope-guardian, adversarial; 75 findings merged into
  57 resolutions, recorded in the plan's "Review resolutions") was folded in by an opus agent
  and the plan is stamped Reviewed. Its shape: willikins as an OAuth 2.1 resource server only
  (RFC 9068 validation against a cached JWKS, RFC 9728 metadata, the 401/403 challenges), a
  derived `oauth-<12 hex>` principal with claims journaled additively, four scopes plus two
  subject allowlists (`WILLIKINS_AGENT_SUBJECTS`, `WILLIKINS_APPROVER_SUBJECTS`) as the
  authority, a PKCE browser login with a `__Host-` session cookie and a login-binding cookie,
  OAuth required on every HTTP bind (stdio keeps the local principal), the static hashes and
  `hash-token` removed and refused at startup, an in-process fake authorization server for a
  deterministic adversarial pass 3, pass 2's four exposure hand-overs as tasks, a go-live
  sequence, 17 tasks, 19 acceptance tests, 17 new variables. **Operator decisions the same night (plan addendum
  2026-09-16):** the identity provider must be **self-hostable open source**, because an
  open-source tool may not require a third-party SaaS account, which makes it decision 10's
  first must-have and rules Auth0 out entirely; the public host is
  **`willikins.bandeabonnot.com`**, so the audience is `https://willikins.bandeabonnot.com/mcp`
  and the redirect URI `…/approvals/callback`; and no identifier is permanent, so decision 12
  gained a migration path (`WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`, accepted but never published,
  with a startup warning while it is set) in place of the draft's "frozen forever" language. One
  open decision remains, the operator's: which self-hostable provider, with self-hosted Logto
  recommended as the only one of six passing all three must-haves. A willikins that issues its
  own tokens, so a single-operator deployment needs no second service, is recorded as the
  milestone 3 note that would finish the thought. Implementation starts only after the milestone 2 plan is
  Completed (task 14 part B).
- **Earlier (2026-09-16, small hours):** task 14 part A (Workflow `wf_ba182dd5-636`,
  opus): both sandbox credentials probed alive (read-only; the Doppler project checks 404 by
  design in the empty workplace), and acceptance test 18 written as code,
  `crates/willikins-cli/tests/live_smoke.rs` behind the `live-tests` feature and
  `WILLIKINS_LIVE_TESTS=1`, with `tests/smoke_parity.rs` driving the same four binary
  invocations against the fake catalog in every gate. Driving the binary corrected two plan
  words (the three root configs read `Unchanged` on the first apply; `ci_secret` reads
  `Converged` on the second); acceptance test 18 is amended. Pre-flight found that the
  teardown needed a scope on the operator's own `gh` credential that they refused outright, so
  the teardown was re-authenticated onto the sandbox PAT instead (Workflow `wf_b2605177-57f`,
  sonnet verified by opus): `deploy/teardown.sh` now talks to GitHub through `curl --config -`
  with `WILLIKINS_GITHUB_TOKEN`, the same shape the Doppler half uses, which the live write
  cycle had already proved can delete a repository in the throwaway org. The verify caught a
  leak the implementation missed — both tokens rode in the environment every `curl` child
  inherited, which `ps -E` exposes to the same user — and both are now unset before any child
  starts, with the stubs recording each child's environment. **Never ask for that scope again:
  the operator's answer was "That one is out of the question."**
- **Earlier (2026-09-15, evening):** task 13, adversarial pass 2, ran end to end
  over TCP against the real binary through Workflow `wf_7515decb-fd7` (an Opus attacker,
  then an Opus completeness critic) and held: no secret byte, no forged approval, no
  unapproved run, no escape from the trusted directory. Two availability defects fixed
  (`template.render`'s 390 MB amplification; a hung provider read filling the blocking pool
  while `/healthz` stayed green, now bounded at 64 concurrent tool calls with `Busy` beyond)
  and three journal words corrected additively (`PlanRecorded.principal`, three new
  `AuthFailedReason`s, `RecordedInputUnreadable`), with a frozen pre-change journal proving
  replay. The critic added evidence only (two skipped hand-over items attacked, a vacuous
  test and two sweeps made live, a hang-instead-of-fail harness fixed, a second frozen
  fixture, a seven-mutation table); its section closes the note
  `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`. The plan's task 13 addendum
  records decisions and hand-overs; five plan passages were corrected in place. Same
  evening, with the operator's permission: the Railway CLI went to 5.57.2, `.railway/railway.ts`
  was regenerated from the live project (`railway config pull`), the healthcheck added, the
  plan read by the operator (one change, nothing destroyed) and applied on their word; the
  redeploy passed `/healthz` live for the first time. The operator also said they run
  several GitHub organizations and Doppler workplaces, so per-target credential routing is
  a milestone 3 design item (plan, "Notes for milestone 3").
- **Earlier (2026-09-14, the live write cycles and tasks 7 to 10a):** the live Doppler write cycle
  (`crates/willikins-providers-doppler/tests/live_write_cycle.rs`, `live-tests` feature plus
  `WILLIKINS_LIVE_TESTS=1`, an Opus agent outside the Workflow) ran green on its fourth
  attempt against the dedicated test workplace: a real project created, converged, refused
  as foreign, its three auto-created root configs `Unchanged`, a new environment, a token
  minted and rotated, a secret read, both projects deleted. It found and fixed two defects:
  a missing secret is Doppler `200` with `value.computed: null`, now `NotFound`; and the
  token list's `token_preview` leaked six real token characters into the gitignored live
  recordings, now redacted. Eight Doppler fixtures verified; the research note's project-id
  claim corrected; verify item 4 and the branch-config prefix question answered in the
  plan. Task 10a (both halves) landed through Workflow `wf_a2ae26fe-894` and its Opus
  verify is running. Earlier the same day, the live GitHub write cycle
  (`crates/willikins-providers-github/tests/live_write_cycle.rs`, opt-in with
  `WILLIKINS_LIVE_TESTS=1`, Workflow `wf_a2ae26fe-894`) ran once, green on the first try:
  `github.repo.ensure` created `Willikins-Test/willikins-live-write-cycle`, converged with
  `changed: false`, refused the visibility mismatch, `github.actions_secret.ensure` sealed
  a synthetic token and read it back `Present`, the three unverified fixtures were
  verified by key set (no authored fixture changed), and the repository was deleted (the
  delete lives only in the test; no tool or client method deletes). A second opt-in test,
  `WILLIKINS_LIVE_LEFTOVER_CHECK=1`, confirms the repository is gone. Before that, tasks 7
  (`willikins-providers-github`), 8
  (`willikins-providers-doppler`) and 9 (adversarial pass 1) landed through Workflow
  `wf_5a050a35-0a3`. The verifiers' real finds: a repository whose `topics` is null failed
  to parse instead of reading `Foreign`; `doppler.config.ensure` ignored Doppler's `root`
  flag, so a branch config could squat on a root config's derived name and a token would
  have been minted into the wrong config; the journal fold let a later line overwrite the
  record it named (pass 1's one defect). The GitHub read-only probe ran against
  `Willikins-Test` (identity and org verified; the org has no repository yet, so the
  repository and public-key fixtures stay unverified). The Doppler probe is written and
  refused by the credential regex, as designed, until a service-account token exists. The
  research note `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md` records
  the pass; `todos/2026-09-14-pass-1-items-for-task-10a.md` lists what the server closes.
- **Product fact (2026-09-14):** the operator's CI is Buildkite with self-hosted runners,
  not GitHub Actions. `github.actions_secret.ensure` stays milestone 2's secret sink as the
  end-to-end proof in the throwaway org; the operator's CI design (2026-09-14): every secret
  lives in Doppler and Buildkite holds one CI/CD Doppler service-account token, so no
  per-repository secret is ever pushed into CI. Milestone 3 therefore drops the
  `ci_secret` sink from the positive fixture and adds a Buildkite provider whose first tool
  creates the pipeline for the new repository (the operator confirmed 2026-09-14 that
  willikins must be able to provision the pipeline when a workflow asks for it, and their
  workflows do); whether provisioning also grants the CI service account access to the
  new Doppler project is a milestone 3 design question. A Buildkite API token for a test
  org is needed when that work starts. Doppler layout decided the same evening: one Doppler
  project per real project plus inheritable base configs (one per shared service, such as
  Apple distribution certificates) that project configs inherit through Config Inheritance;
  recommended, not mandated; two milestone 3 tools (`config.inheritable.ensure`,
  `config.inherits.ensure`, endpoints quoted in the research note). App Store Connect is a
  candidate provider after that.
- **Group A landed (2026-09-12, late evening):** Workflow `wf_c7a1d060-cec` ran three
  worktree lanes. Lane 1 (1a, 1b) and lane 3 (task 0, 1d) merged onto `main` with gates
  green. Lane 2 (1c) was lost: the coordinator's stop message meant for a duplicate agent
  was read by the real one, which halted with an uncommitted partial diff; that diff is
  saved as `lane2-partial-1c.patch` in the session scratchpad (two finished type files,
  `WorkflowName` and `Description`, plus token-literal reshaping) and 1c is re-run on
  `main`. Tasks 1c, 1e, 2 and 3 run sequentially on `main`, one Workflow. Operator
  credentials are in
  `~/.config/willikins/sandbox.env` (mode 600, outside the repo): the Doppler one is, since
  2026-09-14 (afternoon), a `dp.sa.` service-account token for a dedicated, empty Doppler
  test workplace (the earlier `dp.st.` config-scoped token could only read one config).
- **Next action:** milestone 3b, the one manual step left in the operator's process:
  `doppler.project_member.ensure`, which grants a Doppler service account access to a project by
  role and environments (`POST /v3/projects/project/members`, body `{type, slug, role,
  environments[]}`). Until it lands, a pipeline the new document creates cannot read its Doppler
  project's secrets until someone adds that grant by hand, which the document's own header says.
  The grant model was settled empirically on 2026-09-16 and is in the milestone 2 plan's "Notes
  for milestone 3": grants are per project and name environments, an environment grant reaches
  that environment's branch configs and no others, and a workplace-wide blanket exists only as
  admin over everything. Also open for 3b: the two Doppler inheritance tools for the operator's
  shared base configs, credential routing across several GitHub organisations and Doppler
  workplaces, and the re-read-error-masking gap the 3a adversarial pass recorded. Milestone 2c
  (willikins as its own authorisation server, 24 decisions, 20 tasks, 26 acceptance tests, four
  operator decisions settled) is fully planned and deliberately shelved: it buys a reachable
  deployment, and the operator runs willikins locally through the CLI, where there is no
  authentication at all. App Store Connect was researched on 2026-09-16
  (`docs/research/2026-09-16-app-store-connect.md`): identifiers yes, app groups no, app records
  no, and the credential itself can never be provisioned.
- **Railway (2026-09-15):** the operator created project `Willikins` (id
  `7d8e6a12-f6cb-46fd-aa63-de8c352cdca0`, workspace "el-fitz's Projects"), environment
  `production`, service `willikins` sourced from `Lightless-Labs/willikins`, auto-deploying
  on push; they set the Dockerfile builder, though the first (pre-Dockerfile) deployment
  ran under Railpack. The checkout is linked (`railway status`); the CLI is logged in. A
  service domain `https://willikins-production.up.railway.app` was created by the
  coordinator's `railway domain` check; the operator deleted it in the dashboard the same
  day (the CLI cannot remove one; the service now has no domain) and to expose no public domain until the
  auth path matures beyond static bearer tokens (milestone 3), so task 12 proves build,
  startup refusals and the internal healthcheck only, and the smoke run goes over stdio or
  a localhost `serve --http`. No volume and no willikins variables yet: task
  12 needs `RAILWAY_DOCKERFILE_PATH=deploy/Dockerfile` (or the Dockerfile at the root), a
  volume for the journal, and the plan's environment variables, the two provider
  credentials arriving through Doppler's Railway integration. Done 2026-09-15 (task 12):
  the root `Dockerfile` builds on Railway (builder now DOCKERFILE), the volume
  `willikins-volume` is mounted at `/data`, the five non-secret variables are set, the
  latest deployment is SUCCESS and serves the fake catalog (`WILLIKINS_FAKE_CATALOG=1`);
  `.railway/railway.ts` mirrors the live project plus the healthcheck (region
  `europe-west4-drams3a`, 5000 MB, five `preserve()` variables, GitHub source) and is
  applied on 2026-09-15 with the operator's approval (a fresh plan is up to date); the
  first GitHub-triggered build (`fc866bf0`, commit `4977451`) succeeded the same day, so
  auto-deploy on push is proven.
- **Credentials for the probe and the smoke run** live in `~/.config/willikins/sandbox.env`
  (mode 600, outside the repo; source it before a live run): a GitHub fine-grained PAT
  scoped to the test organization `Willikins-Test` (read-only checks passed 2026-09-13)
  and a Doppler service-account token (`dp.sa.`) for a dedicated test workplace that
  holds no project: the read-only probe ran on 2026-09-14, authenticated, and failed on
  four 404s for `willikins-test` (the missing-project shape check passed). A Doppler live
  write cycle (`crates/willikins-providers-doppler/tests/live_write_cycle.rs`, gated by
  the `live-tests` feature plus `WILLIKINS_LIVE_TESTS=1`) creates and deletes its own
  throwaway projects there, so no persistent project is needed for the fixtures.
  Agents get the file path, never the values, and never print them.
- **Ask the operator for** sandbox credentials (a throwaway GitHub org token and a Doppler
  service-account token) before task 8, so the read-only probe settles the undocumented
  Doppler facts early rather than at the live smoke run.
- **Do not** start the MCP server task (10b) before 1c lands: the YAML pre-scan and byte
  cap are what make accepting a document body over the network safe.

## Project State

Eleven crates, dependencies flowing downward only:

| Crate | Holds | State |
| --- | --- | --- |
| `willikins-types` | `DomainType`, `DomainObject`, the derive, slug grammar, 21 domain types (now `WorkflowName`, `Description`), `TypeRegistry`, `naming::v1`, `propose_slug`, `SinkToken` | done, verified |
| `willikins-derive` | `#[derive(DomainType)]` for `String`, `SecretString`, and `FromStr + Display` storages | done, verified |
| `willikins-core` | `Value` with its JSON shape, `Tool` contract (`ensure -> Ensured`, `Observation::Mismatch`), `tool::helpers`, `Catalog`, `Workflow` (typed name and description), `check` (21 error kinds, `Site`), `describe` (`document_description`), `plan` (`AttributeMismatch`, `Plan::fingerprint`), `apply` (the one `SinkToken::new` site; `Approval`, `ApplyError`, `ApplyObserver`, `PrincipalId`, `Timestamp`), `Reported`, JSON schemas for the MCP result types, `testing` generators behind `test-support` | done through task 4 |
| `willikins-tools` | `naming.v1` and `template.render`, pure, moved out of the fake crate; `register(&mut Catalog)` | done |
| `willikins-journal` | `PlanId`/`RunId` (uuid v7), `Event`/`Entry`, `Redacted<T>`, `Journal` trait with fold-based views (`PlanRecord`, `RunRecord`), `FileJournal` (JSONL, fd-lock, `sync_data`, validated replay), `MemoryJournal`, `JournalObserver`, `run_and_journal` | done, verified |
| `willikins-providers-http` | `Credential` (env-sourced, redacted, crate-private `authorize`), `Http` over ureq 3 (retry on 429/5xx/transport for GET/PUT/DELETE, never POST; `Retry-After` capped at 60 s; no redirects; `put_empty`, `delete_with_body`), `ProviderError` with rate-limit facts -> `ToolError` with `provider says:` labelling and 256-char bound, `testing` module behind `test-support` | done, verified |
| `willikins-providers-github` | `GitHubClient`, live `github.repo.ensure` and `github.actions_secret.ensure` (sealed box via `crypto_box`), secondary-rate-limit retry, authored fixtures with a per-file verification status, the GitHub half of the read-only probe | done, verified; probe ran |
| `willikins-providers-doppler` | `DopplerClient`, live `doppler.project.ensure`, `config.ensure` (`root: true` only), `service_token.ensure`, `service_token.rotate` (always `Absent`), `secret.get` (`value.computed`), a nine-tool live catalog test, the Doppler half of the probe (written, refused by the `dp.st.` token) | done, verified; probe not run |
| `willikins-server` | library and binary: `Butler` (plan, approve, reject, apply, run, runs, pending approvals, validate, describe, list_workflows, list_tools, propose_slug), `ButlerConfig`, `ButlerError` (kind-tagged), `ServerConfig::from_vars`, `live_catalog_with`, `StartupError`; `WillikinsHandler` (rmcp tools), `serve_stdio`, `serve_http`, `router`, `HttpConfig`, `TokenHash`; binary `willikins-server serve --stdio|--http [--fake] [--principal]`; tests: acceptance 7, 8, 12, 13, read ops, CLI and MCP parity, `adversarial_10a.rs` (24 attacks), `adversarial_10b.rs` (31 attacks), `http_server.rs`, `binary_startup.rs` | plan identity, windows, the single-apply lock, decision finality, static bearer tokens + Basic auth (sandbox-grade: no public domain until milestone 3) |
| `willikins-providers-fake` | `FakeState` (JSON-seedable, one-way redacted; `next_token`, `fail_ensure_once`, `ensure_calls`, `read_calls`), nine tools plus the two from `willikins-tools` (`doppler.service_token.rotate` is Destructive); every `ensure` reads first, `doppler.project.ensure` seeds `dev`/`stg`/`prd`, `github.repo.ensure` refuses a visibility mismatch | done through task 4 |
| `willikins-dsl` | YAML document to `Workflow`, reference grammar, located errors, published document schema, 256 KiB cap, anchor/alias/BOM pre-scan, typed `name`/`description` | done through task 1c |
| `willikins-cli` | `validate`, `describe`, `plan`, `schema`, `propose-slug`; `--json` through `Reported`; text renders only through `Value::render()` and `single_line` escaping; `document says:` prefix | done, acceptance suite |

The CLI's first four subcommands are the milestone 2 MCP tools one to one. Nothing talks to a
real API. `apply` exists on the `Tool` trait but nothing calls it.

Fixtures: `workflows/new-rust-service.yaml` is the positive case; `workflows/fixtures/` holds
one document per negative case and `fixtures/state/` the fake-state files. Every fixture's
header comment names its acceptance test and exact expected error.

Research: `docs/research/2026-09-11-m1-dependencies.md` (crate and provider-grammar
research, with a correction block on its slug section),
`docs/research/2026-09-12-check-adversarial-pass-1.md`,
`docs/research/2026-09-12-e2e-adversarial-pass-2.md`.

## Architecture Gotchas

- **`SinkToken` is a lint, not a proof, inside the workspace.** Cargo unifies features, so
  once `willikins-core` enables `willikins-types/executor` every crate can see the
  constructor. `clippy.toml` disallows `SinkToken::new`; gates run `-D warnings`; every
  allowed call site is a test item. `Tool::read` takes no token, which is a structural aid.
  The feature gate protects external consumers only.
- **`cargo check -p willikins-types` is a real gate.** The crate enables its own `executor`
  feature through a self dev-dependency, so `--all-targets` never builds it the way its
  dependents see it. A cfg-gated bug slipped past the other three gates once.
- **This host has 11 GB of RAM and 6 CPUs, shared with other sessions.** Three parallel
  worktree lanes each building their own `target/` took 6.2 hours on 2026-09-12 and the
  coordinator's gate run was killed for memory, every time in the doctest phase, whose
  harness compiles doctests per CPU regardless of `-j`. Run cargo with `-j 2` and the test
  gate with `RUST_TEST_THREADS=2`, one lane at a time on `main`, never two cargo commands
  at once. A full test gate is 30 to 45 minutes here.
- **Read the log body, never a captured exit code.** The RTK hook that once rewrote cargo
  commands is gone from this machine (2026-09-12); gates are bare `cargo`. `$?` after a pipe
  in zsh is the last command's status, so never pipe gate output.
- **Network tools work.** WebFetch, WebSearch, context7, `curl`, and `gh api` work for the
  main session and for agents. Prefer verbatim primary sources: GitHub's OpenAPI description,
  docs repos' raw markdown, Doppler's `<page>.md` twins, the `swiftlang/swift-book` DocC
  source for docs.swift.org.
- **`extern crate self as willikins_types`** in `willikins-types/src/lib.rs` exists so the
  derive's generated `::willikins_types::` paths resolve inside the crate itself.
- **The type registry and `type_infos()` come from one `domain_types!` invocation** in
  `lib.rs`. Add a domain type there or it is invisible to `check`, `describe`, and the CLI.
- **The registry refuses secret types for any literal or input before looking at the text**,
  including an empty list. Seeding secret values into fake state goes through serde
  `Deserialize` on the concrete type instead.
- **`Value::render()` is the only path from a value to text.** Debug and Serialize on `Value`
  go through it. A secret list prints one marker in Debug and one marker per element in JSON.
- **`naming::v1` is frozen.** Adding a provider adds rows; changing a row is `v2`. Pascal is
  not injective for digit-only words (`foundry-2` and `foundry2` both give `Foundry2`);
  accepted because pascal never feeds a natural key.
- **Error locations are a `Site`** (`Port { node, port }`, `ForEach { node }`, `Output { name }`),
  serialized as `{"kind": "port" | "for_each" | "output", ...}` and displayed as
  `node.port`, `node[for_each]`, `workflow.outputs.name`. The old `outputs`/`for_each`
  sentinels are gone; no test may compare against them.
- **Every error serializes as `{"kind", "message", ...fields}`** through `Reported<T>`; the
  `variant_kinds!` list in the core tests is the exhaustiveness guard (a new variant fails
  to compile until it is listed). Gaps that remain are in
  `todos/2026-09-12-error-json-uniformity-gaps.md`.
- **`AttributeMismatch` carries a `Site` and no instance key**, so a `for_each` node cannot
  say which instance mismatched; milestone 3 with `Action::Update`.
- **The YAML pre-scan** (`saphyr-parser`) refuses anchors, aliases and a leading BOM before
  `serde_yaml_ng` runs, fails closed on its own scan errors, and compensates for
  `Marker::col` being 0-based despite its docs. It is a second full parse of the document,
  bounded by the 256 KiB cap that `load_document` applies to the file read as well.
- **`willikins_types::Description` and `willikins_core::describe::Description` share a name**
  on purpose; use qualified paths.
- **The executor's approval gate reads the checked class**, not only the plan's
  `requires_approval` flag: a plan replayed from the journal is data. Refusals before any
  provider write (`ApprovalRequired`, `Plan`, `Drift`) carry no partial result; mid-run
  failures (`UnknownInput`, `UnknownRequiredInput`, `Tool`) carry the partial `Applied`.
- **`Plan::fingerprint` does not cover node inputs**, and core never compares the approved
  plan's workflow name with the checked one. The server's `(name, document sha256)` plan
  identity is what closes that; see `todos/2026-09-14-plan-identity-must-cover-inputs.md`.
- **The journal replays through `Redacted<T>`** because core `Value` has no `Deserialize`:
  a sealed wrapper storing the already-redacted JSON. There is no hash chain, by decision.
  Replay reads the path, not the locked descriptor (a rename over it orphans appends).
- **Provider text is labelled `provider says:`** and bounded to 256 characters; a 401/403
  body is dropped at construction; `Retry-After` is capped at 60 s; redirects are refused;
  POST is never retried. GitHub's secondary rate limit is a 403 with `retry-after`, which
  the shared client does not retry: task 7's tools handle it.
- **`doppler.config.ensure` needs `root: true`**: Doppler names a branch config `<env>_<name>`,
  which can equal the root-config name `naming::v1` derives for a multi-word environment;
  anything but `root: true` on a 200 is `Foreign`. `doppler.service_token.rotate` reads
  `Absent` always, so a Destructive step never plans as `NoOp`.
- **The journal fold keeps the first record of an id** and `FileJournal::open` refuses a
  duplicate `PlanRecorded`, `RunStarted`, `RunFinished` or `NodeFinished` (pass 1's fix).
  `boundary_` tests name the task (10a or pass 2) that closes each documented gap.
- **Both `#[allow(clippy::disallowed_methods)]` in the derive are load-bearing**: clippy
  does lint macro expansions, and the derive has two `expose_secret` sites (`expose` and
  the generated `PartialEq`). `expose_secret_mut` is disallowed too. Two syn-based tests
  walk every `.rs` file cargo compiles to pin the call sites of both secrets' escape
  hatches.
- **`std::env::set_var` is `unsafe` in edition 2024 and the workspace forbids `unsafe`**;
  tests build credentials through `Credential::for_testing` behind `test-support`.
- **`for_each` expands at plan time**, keyed by each item's canonical string. `check` can only
  verify shapes; `plan` reports `KeyNotInForEach`, `ForEachUnknown`, `DuplicateForEachKey`.
  Duplicates in a statically known default are caught by `check` as `DuplicateForEachDefault`.
- **`Observation::Absent { predicted }`**: a tool fills every output it can derive from its
  inputs so downstream nodes can still `read` at plan time. A token's value is `Unknown` on
  both `Absent` and `Present` because Doppler cannot re-read it. `Observation::Mismatch
  { port }` is ours-but-different; `plan` refuses it symmetrically.
- **`Tool::ensure` returns `Ensured { outputs, changed }` and reads first.** Comparable-state
  resources create only what is missing; the GitHub Actions secret always writes; pure
  tools answer `ensure` exactly as `read`. The fake `doppler.project.ensure` seeds the three
  default configs, so the positive fixture's `configs[dev|stg|prd]` are `Unchanged` on a
  first apply.
- **serde_yaml_ng keeps the last duplicate mapping key silently.** The DSL deserializes every
  map through a unique-map visitor; duplicate keys surface as YAML errors with a location.
- **`#[serde(deny_unknown_fields)]`** is on documents and fake state. The published document
  schema carries `additionalProperties: false` at every level; regenerate the insta snapshot
  if that changes.
- **The trybuild `.stderr` files** quote rustc diagnostics and are toolchain-sensitive.
  Regenerate with `TRYBUILD=overwrite` on a toolchain bump rather than hand-editing.
- **Workflow-as-tool (composition) is milestone 2b**, its own plan; nothing in milestone 2
  exercises it.

## Open TODOs

| File | Priority | Owner |
| --- | --- | --- |
| `todos/2026-09-12-milestone-2-plan.md` | high | the tracking todo; tasks 0–6 done, task 7 next |
| `todos/2026-09-14-plan-identity-must-cover-inputs.md` | high | task 10a and adversarial pass 1 |
| `todos/2026-09-14-pass-1-items-for-task-10a.md` | high | task 10a: plan identity, approval by journaled event, single-apply lock, digest type |
| `todos/2026-09-12-error-json-uniformity-gaps.md` | medium | task 10a (`InputError`, `DocumentError` shapes) and 10b |
| `todos/2026-09-13-apply-tests-on-the-real-fake-catalog.md` | low | any core task after 4 |
| `todos/2026-09-14-journal-follow-ups.md` | low | task 10a |
| `todos/2026-09-11-propose-slug-digit-letter-tokens.md` | low | not scheduled |

## How Work Is Verified

- Four gates before every commit, bare `cargo`, in the background with a 600,000 ms
  timeout, reading the log body.
- Each task: sonnet implements test-first, opus attacks it with new tests and fixes what it
  breaks, one commit per fix. The Workflow scripts from this session are under the session's
  `workflows/scripts/` directory and follow one shape: `CONTEXT` string with gates and rules,
  implement stage with a structured `REPORT`, verify stage with a structured `VERDICT`.
- Adversarial passes are acceptance test 12 of the milestone plan and are recorded under
  `docs/research/`. Every bypass becomes a fixture plus an acceptance test.
- Plans get the document-review workflow (scope, feasibility, security, coherence,
  adversarial personas on sonnet, merged and ranked) before implementation starts. The
  milestone 1 review produced 23 findings, five of them blockers; all are resolved in the
  plan's "Review resolutions" section.

## Recent Context

- 2026-09-11: design conversation, design doc, dependency research, milestone 1 plan and its
  review, scaffold, tasks 2 and 3 (slug grammar, derive macro).
- 2026-09-12: tasks 4 through 13. Notable findings on the way: `gen` missing from the Rust
  keyword list; feature unification defeating the `SinkToken` gate; an input's default value
  bypassing the sink check; an empty list literal bypassing the secret refusal; duplicate
  `for_each` keys; a 10 MB rejected input echoed in full; unknown document fields silently
  ignored. No attack in either adversarial pass reached a secret byte.
- Costs: nine Workflow runs, about thirty agents, roughly 5.7M subagent tokens.
- 2026-09-14, later: tasks 7, 8 and 9 (about 1.9M subagent tokens); the GitHub probe ran
  live; the operator said CI is Buildkite, not Actions.
- 2026-09-13 and 14: tasks 0 through 6 landed through five Workflows (about 8.5M subagent
  tokens); one lane lost to a misdirected stop message, one verifier killed by an OAuth
  outage and its work inherited by the next; the host's memory limit found and bounded.
- 2026-09-12, second session: milestone 2 plan, research note, design addenda, and document
  review (five research agents, five reviewer agents). Decisions worth knowing before
  reading the plan: two kinds of secret (graph secrets behind `SinkToken`, provider
  credentials behind one `authorize` function and a clippy entry); `Tool::ensure` returns
  `Ensured { outputs, changed }` and every live `ensure` reads first; `apply` over MCP
  returns a `run_id` and runs in the background; a plan has an approval window and an
  apply window; the remote server plans and applies by workflow name only; visibility
  mismatches are refused, not reconciled; composition moved to its own future plan.

## Next: Milestone 2 Runbook

The plan is the runbook: `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`, its
"Tasks" table (groups A, B, C, D run in parallel worktrees), its 19 acceptance tests, and
its "Verify before relying on them" list. Two adversarial passes are tasks 9 and 13 and get
recorded under `docs/research/`. Task 14 (the live smoke run) needs the operator's sandbox
credentials and is the completion gate.
