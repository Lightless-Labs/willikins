# Project survey and the workflow library it implies

**Created:** 2026-09-20
**Plan:** none yet. This is evidence for the workflow catalogue, not a design.
**Related:** `todos/2026-09-20-ios-scaffolding-step-and-entitlements-as-inputs.md`,
`todos/2026-09-20-agent-authored-workflows.md`,
`docs/research/2026-09-16-app-store-connect.md`,
`docs/plans/2026-09-11-willikins-design.md`

The operator, 2026-09-20: *"I want to be able to easily, reliably, and deterministically
provision everything for three new iOS / watchOS app ideas I have, one with a backend, and two
other tools open-source self-hostable tools for AI agents... Setting them up, badly, always in a
different way, always forgetting something, always juggling credentials, and always wondering
whether or not I can trust some coding agents with access to these things is tiring."*

Five of their real projects were surveyed to find out what "everything" actually is. The blunt
headline, stated once here and defended in §3 and §6: **most of what they are asking for is not
provisioning.** It is scaffolding — files written into a repository — and willikins cannot write
a file. Provider calls are the minority of the work in every project surveyed, and in two of the
five they are *zero*. That fact sets the sequencing (§6).

---

## 1. What was read, and the reading rule

Five projects were read in depth, one per surveyor, all on local disk:

| Project | Path | Shape |
| --- | --- | --- |
| descartes | `~/Projects/lightless-labs/public/descartes` | standalone repo, `Lightless-Labs/descartes` |
| pessimal | `~/Projects/Banade-a-Bonnot/pessimal` | standalone repo, `Lightless-Labs/pessimal` |
| phil-connors | `~/Projects/Banade-a-Bonnot/phil-connors` | standalone repo, `Bande-a-Bonnot/phil-connors`, migrating into the monorepo |
| danksworth | `~/Projects/bande-a-bonnot/apps/danksworth` | directory in `Bande-a-Bonnot/monorepo` |
| brassica-ex | `~/Projects/bande-a-bonnot/apps/brassica-ex` | directory in `Bande-a-Bonnot/monorepo` |

The task named four; there were five readings. phil-connors was read at its **standalone
checkout**, because `~/Projects/bande-a-bonnot/apps/phil-connors/` holds only a two-line
comment-only `BUILD.bazel` — a landing pad for a migration in progress, with no content to read.

Read in addition, as CI evidence only and not in depth: `apps/pocket-claw/.buildkite/` (the only
Buildkite directory inside the monorepo), the monorepo root (`MODULE.bazel`, `.bazelrc`,
`Cargo.toml`, `Package.swift`, `Gemfile`, `build/visibility/BUILD.bazel`, `.github/workflows/`,
root `AGENTS.md`/`CLAUDE.md`, `fastlane/`), and `ls -A` of the nine `apps/` siblings (barnum,
brassica-ex, danksworth, infinidash, kumbaya, phil-connors, pocket-claw, ten-a-day, walter).

**The reading rule.** Structure only: file and directory *names* anywhere; *contents* only of
build configuration, CI configuration, agent/doc markdown, licences, ignore files and manifests.
Never opened, grepped or quoted: any `.env`/`.envrc`, anything under `.ignored/`, `logs/`,
`nohup.out`, `runs/`, `outputs/`, `materials/`, any key, certificate, provisioning profile,
keychain or credential file. `credentials/` in phil-connors and `cdk.out/` anywhere were listed
by name and never opened — for phil-connors the roster of *names* in `credentials/` is itself
the provisioning inventory, and is reported that way. `~/.config/willikins/sandbox.env` was never
touched.

**One credential was encountered and is not reproduced.**
`apps/danksworth/ios/Resources/Info.plist` contains a key `SignozApiKey` whose value is a
live-shaped SigNoz ingest credential, committed to the repository and shipped inside the IPA. The
surveyor stopped reading that file at that point. The app's own `CLAUDE.md` acknowledges it
("SigNoz API key hardcoded in Info.plist (PoC — move to secrets for production)"). The fix already
exists in the same monorepo: the root `fastlane/Fastfile` generates
`apps/pocket-claw/ios/Resources/Generated/Telemetry.plist` from environment at build time and
blanks it afterwards. This is worth acting on independently of this survey, and it yields a
product rule recorded in §4.

Two read-only `git remote -v` / `git branch -a` invocations were run in pessimal and phil-connors
before the boundary rule was re-read; nothing was written, no other git command ran outside
willikins, no cargo ran anywhere, and no provider API was called.

---

## 2. One section per project

### 2.1 descartes — a Node CLI that calls itself a Rust project

**What it is.** A local-first, read-only operations triage CLI for a single machine. The shipped
product is a Node.js ESM CLI at `tools/descartes-cli/` (~55 test files). Rust is one crate,
`crates/descartes-root-helper`, a fixed-argv `/proc` resolver with exactly one runtime dependency
under a written zero-dep policy. One Swift file (`DescartesNotifier.swift`, built by a bare
`swiftc` invocation into a hand-assembled `.app`). Python 3 as an inline-heredoc HTTP client
inside bash. Build systems: npm and Cargo, side by side. **No Bazel at all** — `README.md`
§Building and testing states the deviation outright: *"The larger Lightless Labs monorepo prefers
Bazel. This repo builds with npm and Cargo directly for now."*

**Provisioned.** GitHub repo `Lightless-Labs/descartes` (public); a second repo
`Lightless-Labs/homebrew-tap` holding `Formula/descartes.rb`, which CI commits to on every release
tag; a third, private, cross-referenced repo `Lightless-Labs/tart-ci` (the Buildkite plugin);
GitHub Releases as the binary store, driven over plain REST with no `gh` dependency; a GitHub
token with push on the tap (`HOMEBREW_TAP_GITHUB_TOKEN`, narrow, overriding the broader
`GITHUB_TOKEN`); Buildkite org `la-bande-a-bonnot`, pipeline `descartes`, webhooks on branches
*and* tags; self-hosted agents on `big-cabbage` in queues `ci-linux-arm64` and
`ci-macos-apple-silicon`; two baked Tart images (`ci-linux-arm64-rust-bazel`,
`ci-macos-rust-bazel-ios-20260910-v2`); Doppler project `lightless-labs-descartes` with config
`prd_notarisation`; a Doppler service account reachable as `DOPPLER_SERVICE_ACCOUNT_TOKEN`; Apple
**Developer ID** material (Developer ID Application cert + p12 password, notarytool ASC key id /
issuer id / .p8, bundle id `com.bande-a-bonnot.lightless-labs.descartes.macos.notifier`); an npm
scope reserved in name only — nothing is ever published to npmjs.org.

**Scaffolding, not content.** `AGENTS.md` (7.7 KB, the deep one) + `CLAUDE.md` (1.4 KB, a
pointer); `docs/HANDOFF.md` with a `RESUME HERE` block; `docs/ROADMAP.md`; `todos/` (40 files);
`docs/plans/` (45 dated plans with `Addendum`/`Supersedes` headers); `docs/reviews/`,
`docs/solutions/` (YAML frontmatter, foldered by area), `docs/research/`, `docs/design/`,
`docs/operator/` (human runbooks); four `*-validation-brief.md` files at the repo root addressed
to "an infrastructure-running agent"; `.buildkite/pipeline.yml` (the only CI file);
`scripts/` (12 files, 38 KB of release machinery); `.claude/worktrees` gitignored.
**Missing and expected:** no LICENSE anywhere, in a public repo whose `package.json` says
`UNLICENSED`; no `rust-toolchain.toml`, `rustfmt.toml` or `clippy.toml`; no linter or formatter of
any kind for the Node CLI that is ~90% of the shipped code; no CONTRIBUTING, SECURITY.md or
CODEOWNERS.

**What a willikins workflow would have had to do.** Reserve four names in three namespaces from
one slug; create the repo; **write ~15 files into it, without which nothing downstream runs** —
the Buildkite pipeline is bootstrapped to read `.buildkite/pipeline.yml` out of the repository;
add a LICENSE (skipped in reality); create Doppler project `lightless-labs-descartes` and its
non-standard `prd_notarisation` config; **grant** the existing shared CI service account read on
it (not mint a token — descartes migrated *away* from per-project tokens in 2026-09); place five
Apple secrets a human must produce; register the bundle id; ensure the tap repo and a token scoped
to two repositories; create the pipeline with branch *and* tag triggers; ensure agents and images
exist; run the first build and the first tag; write the plan, the HANDOFF block, an operator
runbook and a validation brief for the parts no automation reaches.

### 2.2 pessimal — the Apple-bundle exemplar, and why Bazel is there

**What it is.** A Cargo workspace (edition 2024, MSRV 1.95 forced by `sysinfo`) of nine members,
plus Bazel 8.2.1 via bzlmod, plus Swift/SwiftUI clients, plus Python and bash for release.
`MODULE.bazel`'s own module docstring answers the Bazel question: *"Bazel builds the Apple clients
only. The agents build with Cargo, because Linux and Windows cross-compilation is far simpler that
way."* All eleven `BUILD.bazel` files sit under `clients/`, `common/`, `platforms/` and the root;
none under `agents/`. **Bazel arrives with the Apple bundle, not with Rust.** Cargo cannot produce
a signed `.ipa`. Even so the split is not clean: the macOS menu-bar app is built by
`scripts/build-macos-app.sh` with plain `swiftc`, not Bazel.

Bazel and Cargo share **one** `Cargo.lock` by design: `crate.from_cargo` reads `//:Cargo.lock` so
"a dependency cannot differ between the cargo build that CI runs and the Bazel build that ships
the app".

**Provisioned.** GitHub `Lightless-Labs/pessimal` (public, AGPL-3.0-or-later, LICENSE committed);
Buildkite pipeline `la-bande-a-bonnot/pessimal` with `build_pull_request_forks` **off** — listed
under **Cautions** in both agent files, because the repo is public and the self-hosted cluster
holds signing certificates; Doppler `lightless-labs-pessimal` with `prd_ios_deployment` and
`prd_macos_notarisation`; one service account as a Buildkite **cluster** secret; Apple **App
Store** material (App ID `com.lightless-labs.pessimal.ios`, a distribution profile whose *name
must equal the bundle id in three places*, Apple Distribution cert under team `PKPPLFK854`, a
Developer ID cert for the macOS side, an ASC app record, an ASC API key, a notary key); an iCloud
key-value container that CLAUDE.md marks must-never-change; `LL_CLI_RELEASE_GH_TOKEN` with write
on *two* repositories; the shared `Lightless-Labs/homebrew-tap`; a SigNoz endpoint and ingestion
key injected at Bazel build time via `--action_env`.

**Scaffolding.** `[workspace.package]` + `[workspace.dependencies]` + `[workspace.lints]`
(`unsafe_code = "forbid"`); a `clippy.toml` that exists only for `doc-valid-idents`;
`MODULE.bazel` + `.bazelversion` + `.bazelrc` (with a fixed config vocabulary where distribution
variants extend `ci-base`, never `ci`, so a `select` cannot match twice) + `platforms/BUILD.bazel`;
`cog.toml` (conventional commits, `tag_prefix = "v"`, ordered post-bump pushes); an 862-line
hand-written `.buildkite/pipeline.yml`; 28 release scripts; `packaging/`; `tools/uniffi/` with a
pinned bindgen and a CI check that the committed bindings are byte-identical to a regeneration;
`AGENTS.md` and `CLAUDE.md` as hand-maintained near-copies, 32 diff lines apart.

**What a workflow would have had to do.** The same spine as descartes, plus the Apple App Store
chain, plus `build_pull_request_forks: false`, plus branch configs under `prd` that
`doppler.config.ensure` today reports as `Foreign`.

### 2.3 phil-connors — already on Buildkite, and the only one with a backend

**What it is.** iOS + Rust backend. Repo root holds deploy/CI glue only; the Bazel workspace is one
level down at `phil-connors-app/`. **Four build systems**: Bazel (bzlmod), Cargo (two workspaces —
`server/Cargo.toml` opts out with an empty `[workspace]`), SPM (only to resolve one dependency for
Bazel), and npm/TypeScript/AWS CDK. Languages: Rust, Swift, TypeScript, Ruby, Python, shell.

**It has already dropped GitHub Actions.** There is no `.github/` directory at all;
`.buildkite/pipeline.yml`'s header says it *"Replaces .github/workflows/ci.yml and
.github/workflows/beta.yml"*. This confirms the operator's recollection. It also **sharpens** it:
the exemplar an app *inside the monorepo* would copy is not phil-connors but **pocket-claw**, whose
`apps/pocket-claw/.buildkite/` is the only Buildkite directory anywhere under
`~/Projects/bande-a-bonnot` and is a complete, app-scoped setup (six pipeline YAMLs selected at
runtime by `upload-pipeline.sh`, two locally-authored plugins, a credential-free default route,
signing gated behind a tag regex).

**What the Buildkite port did that Actions did not.** `validate.sh`'s own header records that the
port deliberately **dropped** the Actions `changes` path filter — *"exists to save hosted-runner
minutes… on our own hardware the minutes are free and a skipped step is a coverage hole rather
than a saving"* — and **added** ~900 server Rust tests that "never ran in GitHub CI at all".

**Provisioned.** Private repo `Bande-a-Bonnot/phil-connors`, cloned through a **second GitHub
identity** (`git@elfitz-github.com:…`, an SSH host alias); a Buildkite pipeline plus a **stored
bootstrap step** pasted into Pipeline Settings (the in-repo `.buildkite/bootstrap.yml` is only a
*record* of it — editing the file changes nothing); a cluster secret
`DOPPLER_SERVICE_ACCOUNT_TOKEN`; Doppler project `phil-connors` with **four** configs
(`prd_deployment`, `prd_app-ios`, `prd_backend_railway`, legacy `prd_backend`); **nested tokens** —
a `prd_app-ios` service token stored as a *secret* named `DOPPLER_TOKEN_APP_IOS` inside
`prd_deployment`, and a backend token as a Railway service variable; a GitHub clone token
`GH_CLONE_TOKEN` stored as a Doppler secret and fetched by git's credential helper at checkout;
the full Apple chain (two App IDs, Push, App Groups, Keychain Sharing, Sign In With Apple, App
Attest, AlarmKit, StoreKit products, APNs, a SIWA key and Services ID); Railway project
`bande-a-bonnot` / service `phil-connors` / a `postgres` service / a cross-service
`DATABASE_URL=${{postgres.DATABASE_URL}}` / a persistent volume, with "Wait for CI" deliberately
off; and, as archaeology, AWS OIDC + five accounts + CDK stacks, and a Scaleway VPS systemd unit.
`credentials/` **file names alone** evidence nine or ten external API relationships: Anthropic,
ElevenLabs, Mapbox, OpenWeatherMap, PirateWeather, Honeycomb, SigNoz, Embrace, plus Apple SIWA and
APNs.

**What a backend adds that a pure app does not.** Four things, all absent elsewhere: a deployment
target with its own topology that config-as-code cannot declare (hence a 130-line imperative,
check-before-act `infra/railway/provision.sh` — effectively a bespoke mini-willikins); a container
image, registry and a runtime that fetches its own secrets; a database with an independent
lifecycle, a cross-service variable reference and a volume whose loss loses data; and runtime
secrets distinct from build secrets, which is exactly why there are three live Doppler configs and
not one.

**Migration is not a from-nothing run.** `BUILD_NUMBER_OFFSET=400` exists only because GitHub's run
counter reached 334 before the move and App Store Connect refuses a non-increasing build number.
A migration workflow must *read* the last consumed build number from ASC. There is no greenfield
equivalent of that step. (Per `todos/2026-09-20-agent-authored-workflows.md`, migration is
explicitly out of the catalogue: *"that's a future improvement"*.)

### 2.4 danksworth — pure Swift in a monorepo, and the app that borrowed a sibling's secrets

**What it is.** iOS host app + custom keyboard extension + iMessage stickers extension. **Swift 6
and SwiftUI only — no Rust, no UniFFI.** The root `Cargo.toml`'s members list `platform/*` and
`apps/brassica-ex/*` and nothing under `apps/danksworth`. This contradicts the Lightless Labs
`CLAUDE.md` convention that iOS apps carry a Rust core through UniFFI, and the rails for that path
exist unused at the root (`rules_rust` with three iOS triples, `platform/ffi/bande_a_bonnot_ffi`,
`tools/uniffi/`).

It owns exactly six things: a comment-only `BUILD.bazel`, `CLAUDE.md`, `Gemfile`(+lock),
`fastlane/`, `ios/`. Everything else is at the monorepo root. The real build is one 400-line
`apps/danksworth/ios/BUILD.bazel`: seven `config_setting` rows over the root `.bazelrc`'s
`--define`s, three `objc_library` SDK wrappers, `apple_bundle_version`, four
`local_provisioning_profile` rules, five `swift_library` modules, two `expand_template` plist
expansions, three bundle rules and three test rules — including an `ios_ui_test` pinned to
`iPhone 17` / os `26.2` as a hard-coded literal inside the BUILD file.

**Provisioned, and what got forgotten.** One Apple team, hard-coded in **four** places against an
unused `--define=APPLE_TEAM_ID` proposed in `.bazelrc.local.template`; three explicit App IDs; one
App Group (and a second, cross-app one proposed but not provisioned); a keychain access group;
three distribution profiles each named byte-for-byte as its bundle id; the team distribution cert
and ASC API key; an ASC app record with TestFlight. **And no Doppler project of its own**:
`.github/workflows/danksworth-beta.yml` runs `doppler run --project pocket-companion --config
prd_deployment` authenticated by `secrets.POCKET_COMPANION_DOPPLER_TOKEN_DEPLOYMENT`. It borrows a
sibling app's Doppler project *and* a sibling app's service token. That is the single clearest
"forgot something" in the sample, and it is **expressible with tools willikins already has**.

**The in-place edits are the hard part.** Three existing shared files must gain a row before
anything works: the `ios` `package_group` in `build/visibility/BUILD.bazel`; the root
`Cargo.toml` members (for anything with a Rust core); `.gitignore`, which has grown app-specific
lines. (A fourth — the CI job's explicit Bazel target list in `.github/workflows/ci.yml` — should
*not* be edited: that file is being dropped.)

### 2.5 brassica-ex — the project that provisioned nothing at all

**What it is.** A CLI-first, text-only narrative-RPG engine, four Cargo crates in four role-named
directories, all members of the monorepo-root workspace. Rust only for shipped code; Python 3 and
bash for local tooling. **Cargo only — no `BUILD.bazel`, no `.bzl`, no `.buildkite/` anywhere under
`apps/brassica-ex`.** Its own README: *"a Cargo-only bootstrap. There are no Bazel targets for this
app yet."*

**Provisioned: nothing.** No repository, no Doppler project, no config, no token, no pipeline, no
domain, no Apple identifier, no registry entry, no deployment target, no CI credential. The engine
reads JSON off disk and talks to nothing. Its entire external-facing act of creation was appending
four paths to `members` in the root `Cargo.toml` plus six rows to `[workspace.dependencies]`.

**CI coverage is incidental and about to become zero.** `grep -rn brassica .github/
apps/pocket-claw/.buildkite/` returns nothing. Its only coverage is the generic
`cargo test --workspace` line in `.github/workflows/ci.yml` — the file being dropped. The
`bazel build --config=ci -- //...` step in the same workflow sees nothing of it, because it has no
Bazel targets.

**Its README is its CI definition, and nothing executes it.** A `## Validate` block holds the exact
fmt/clippy/test commands with `CARGO_TARGET_DIR=/tmp/brassica-pipeline-target` and all four `-p`
flags spelled out.

**The product lesson.** brassica-ex is a complete, working project with no secret store, no CI and
no deployment target. A workflow language whose every document ends in a Doppler project and a
token cannot describe it. **A workflow must be allowed to be legitimately small.**

---

## 3. The layers

### 3.1 The base — what recurs in all five, and it is smaller than it looks

Lead with the blunt version: **the base layer contains zero provider calls.** It is entirely files.

| Base element | Evidence in all five |
| --- | --- |
| `docs/HANDOFF.md`, read-first, with a dated RESUME/state block | descartes `docs/HANDOFF.md`; pessimal `docs/HANDOFF.md`; phil-connors `phil-connors-app/docs/HANDOFF.md`; danksworth inherits the monorepo root's; brassica-ex `docs/HANDOFF.md`. Same convention as willikins' own `CLAUDE.md` mandates. |
| `docs/plans/`, one dated plan per milestone, `Created`/`Reviewed`/`Addendum`/`Completed` headers | descartes (45 plans, stacked addenda); pessimal; phil-connors; brassica-ex (30, `YYYY-MM-DD-<slug>-plan.md`); wording identical in the Bande-a-Bonnot `AGENTS.md` and the Lightless Labs `CLAUDE.md` |
| `todos/`, one file per item — **naming unsettled, see §4** | all five |
| An agent-instruction file — **name unsettled, see §4** | all five |
| Org-prefixed naming across several namespaces from one slug | `bande_a_bonnot_brassica_ex_core`, `phil_connors_{name}`/`PhilConnors{Name}`, `BandeABonnotDanksworth*`, `pessimal_{area}_{name}`/`Pessimal{Name}` |
| Trunk-based development, path-scoped commits (`git commit -m … -- <paths>`) | owner ruling 2026-07-09 in the monorepo root `CLAUDE.md` and `AGENTS.md` and in `apps/brassica-ex/AGENTS.md`, with the incident that produced it dated 2026-07-10; pessimal AGENTS.md; phil-connors CLAUDE.md; willikins' own coordinator rule |
| Documentation-as-first-class: a multi-way `docs/` taxonomy | all five, with **different subdirectory sets** (§4) |
| ast-grep over grep; commit early and eagerly; TDD; run gates often | verbatim in phil-connors, pessimal, the monorepo root, and the Lightless Labs root `CLAUDE.md` — a pasted template |

Everything else that *feels* universal is not. Buildkite: absent from brassica-ex and danksworth.
Doppler: absent from brassica-ex, borrowed in danksworth. A LICENSE: present in **one of five**
(pessimal). Bazel: absent from descartes and brassica-ex.

### 3.2 Per kind

**A Rust CLI / service that ships binaries** (descartes, pessimal's agent half):
Cargo workspace with `[workspace.package]`/`[workspace.dependencies]`/`[workspace.lints]`;
release on a `vX.Y.Z` tag with the regex asserted in both the pipeline condition and the script;
GitHub Releases as the artifact store; a Homebrew tap as the distribution channel, written to by
CI; a release token scoped to *two* repositories; a Doppler config per purpose, read over REST
rather than the CLI.

**An Apple app** (pessimal, phil-connors, danksworth): Bazel with `rules_apple`/`rules_swift`;
per-app `config_setting` rows over shared `--define`s; `apple_bundle_version`;
`local_provisioning_profile` with `profile_name` == bundle id in three places (portal, BUILD file,
Fastfile); `ios_application` plus one bundle per extension; an entitlements file per bundle; an
`expand_template` Info.plist; `PrivacyInfo.xcprivacy`; fastlane lanes `fetch_signing` / `build` /
`upload_testflight`; an Apple distribution cert and ASC API key delivered as base64 through
Doppler; TestFlight as the destination. **Bazel arrives with the Apple bundle, not with Rust** —
pessimal's module docstring says so, and descartes (Rust, no bundle) has no Bazel.

**A project inside a monorepo** (danksworth, brassica-ex, and Walter): no repository creation, no
LICENSE, no `MODULE.bazel`, no `.bazelrc`, no root `.gitignore` — all inherited. Instead: a
directory of files, a `BUILD.bazel` (a two-line comment stub for a name reservation, real targets
for an app), crates appended to the root `Cargo.toml` `members`, new rows in
`[workspace.dependencies]`, a row in `build/visibility/BUILD.bazel`'s `package_group`, a
lockfile regeneration, and — per pocket-claw — an **app-scoped** `.buildkite/` directory plus a
pipeline whose `filter_condition` does the path filtering Buildkite has no native equivalent for.

**A backend** (phil-connors only, so strictly this is n=1): a Dockerfile and registry; a deploy
target with topology config-as-code cannot express; a database as a separate service with a
cross-service variable reference and a volume; runtime secrets in their own config, distinct from
build secrets.

### 3.3 Exactly once, and therefore that project's own business

- **descartes**: `docs/operator/` human runbooks; root-level `*-validation-brief.md` addressed to
  "an infrastructure-running agent"; `materials/` and `.ignored/` as local drawers; a read-only
  credential **preflight** (`scripts/check-homebrew-tap-token.sh`) that proves a token has push
  rights *before* the release tag needs it, printing no value. That last one is a provisioning
  butler's instinct written as a shell script, and §6 argues willikins should steal it.
- **pessimal**: `cog bump --auto` deciding autonomously whether to cut a tag on every green main;
  the forward-only release ladder draft → prerelease → anonymous download-and-execute verification
  on both OSes → latest; splitting build from sign into two guests because `ps -E` can read a
  parent's starting environment.
- **phil-connors**: four coexisting deployment-target generations (AWS CDK → Scaleway → Railway →
  Railway+Postgres); a second GitHub SSH identity; nested Doppler tokens; a `.githooks/pre-commit`
  running `gitleaks` that nothing wires up (no repo file sets `core.hooksPath`).
- **danksworth**: a custom keyboard and an iMessage stickers extension; borrowing a sibling's
  Doppler project.
- **brassica-ex**: `docs/bible/`; a JSON content pack as the source of truth with a generated flat
  mirror kept honest by `mirror.py --check`; `docs/agent-cli-protocol.md` — one process per move,
  exactly one JSON document on stdout, empty stderr, exit codes 0/1/2/3 with distinct meanings.
  Given the operator's stated interest in tools for AI agents, that last is a pattern to preserve.

---

## 4. The disagreements

The operator's complaint is "always in a different way, always forgetting something". Here it is
as evidence. Each entry says whether it is **drift** (one convention, unevenly applied, usually
dateable) or a **difference of kind** (two legitimate answers that a workflow must be *told*).

### 4.1 Drift, with a date

**The Doppler CI token.** descartes, pessimal and phil-connors use one shared cluster secret
`DOPPLER_SERVICE_ACCOUNT_TOKEN`. pocket-claw passes `doppler_token_secret:
POCKET_COMPANION_DOPPLER_SERVICE_ACCOUNT_API_KEY` — a per-app named secret. This looks like a live
disagreement and is not: the house cookbook
(`docs/solutions/ci-cd-patterns/2026-09-10-buildkite-self-hosted-ios-cicd-cookbook.md`) is dated
2026-09-10 with pocket-claw as its §9 reference implementation, and descartes' HANDOFF records the
move to the shared account on **2026-09-14**. pocket-claw is the *older* pattern despite being the
in-monorepo exemplar. The target is one shared service account plus a **grant** per project.
Consequence: `doppler.service_token.ensure` produces the pattern CI migrated away from — but see
§4.3, it is not dead.

**tart-ci pinning.** descartes and pessimal pin the plugin to a tag (`#v0.2.4`); phil-connors and
pocket-claw pin a SHA (`#11fc3363…`). The plugin's own README says to pin an immutable tag, never a
branch. Both satisfy that; the org has not settled which.

**Agent-file staleness.** pessimal's `AGENTS.md` and `CLAUDE.md` both still say *"Bazel… arrive
with M4/M5. There is no `MODULE.bazel` yet"* — `MODULE.bazel` exists and Bazel builds the shipping
iOS app. descartes' `AGENTS.md` prescribes four cargo gates; `.buildkite/pipeline.yml` runs one,
and there is no linter at all for the Node CLI that is 90% of the product. The prescription
outlived the scaffolding meant to enforce it.

**Version-pin disagreement inside one repo.** phil-connors: Rust 1.85.0 in `MODULE.bazel` vs
`rust:1.88-bookworm` in the Dockerfile; iOS 18 in `Package.swift` vs `--ios_minimum_os=26.1` in
`.bazelrc` vs `minimum_os_version = "26.0"` in the app target; Ruby 3.4.5 in `.ruby-version` vs
3.4.4 baked in the image. descartes: Node 22.21.1 hardcoded twice, Rust and Bazel existing only as
words inside VM image names.

**The LICENSE, forgotten four times out of five.** Only pessimal has one. descartes is public and
says `UNLICENSED`. The Bande-a-Bonnot monorepo root declares `license = "MIT"` in
`[workspace.package]` and `.github/workflows/ci.yml` lists `LICENSE` in `paths-ignore` — a
reference to a file that has never existed. phil-connors has none.

**A fix applied on one lane and not its sibling.** pessimal's macOS release path was split into a
no-token build and a no-build sign step because of `ps -E`; the iOS TestFlight step has not had the
same treatment (`todos/007-pending-p2-…`). danksworth's Fastfile creates a keychain inside
`~/Library/Keychains` and prints `security` argv; the root Fastfile is the hardened successor that
*refuses* such a path and runs password-bearing calls through `Open3.capture3`. Same problem, two
opposite answers, six months apart.

**The secret in the bundle.** `apps/danksworth/ios/Resources/Info.plist` ships a live-shaped SigNoz
ingest key, while the same monorepo's root Fastfile already generates pocket-claw's equivalent at
build time and blanks it afterwards. **Product rule that falls out: a workflow that scaffolds an
Info.plist must never scaffold a slot to paste a key into.**

**willikins itself.** `ls -A` shows no `.buildkite` and no `.github`. The repository whose
`CLAUDE.md` names four gates and says "run all four before every commit" has no CI at all. It does
have the LICENSE nobody else has.

### 4.2 Genuine differences of kind — a workflow must be told

**`AGENTS.md` vs `CLAUDE.md`: five projects, five arrangements.** descartes: `AGENTS.md` deep,
`CLAUDE.md` a 1.4 KB pointer. pessimal: near-copies 32 diff lines apart. phil-connors:
byte-identical (`diff -q` reports no difference) — plus a *third*, stale copy inside
`AlarmKitDemo/`, evidence the block is pasted rather than authored. danksworth: `CLAUDE.md` only.
brassica-ex: `AGENTS.md` only. The monorepo root has both, drifted (8314 bytes Jul 13 vs 9522 bytes
Sep 10, not symlinks). willikins: `CLAUDE.md` deep. **A workflow cannot "follow the convention"
here.** It must take the name as an input, or emit one as a symlink to the other.

**Todo file naming, and it does not split by org.** `NNN-STATUS-PRIORITY-slug.md` with
`status/priority/issue_id/tags` frontmatter (pessimal, phil-connors, brassica-ex, and per
brassica's `AGENTS.md` copied from pocket-claw) vs `YYYY-MM-DD-slug.md` with
`title/created/status/priority/area/related` (willikins, the monorepo's stated convention).
descartes has **both**, split by date: May-era files carry YAML, September-era files use bold
markdown headers instead. The finding is not "two orgs, two schemes" — pessimal is Lightless-Labs
and uses the Bande-a-Bonnot numbering, citing kumbaya and phil-connors. **It propagates by
copy-from-sibling, not by organisation.** Which is exactly what a catalogue mechanises.

**Doppler project prefix, which does split by org side.** `lightless-labs-descartes` and
`lightless-labs-pessimal` (org-prefixed) vs `phil-connors` and `pocket-companion` (bare).
`naming::v1::doppler_project` yields the bare slug — right for Bande-a-Bonnot, wrong for
Lightless-Labs. **Verify before writing a v2 row:** whether these are one Doppler workplace or two
was not established, and the answer decides whether this is a prefix row or a
workplace-routing question.

**Doppler config names, where the org agrees and willikins does not.** Every project that has
configs uses `<env>_<purpose>`: `prd_notarisation`, `prd_ios_deployment`,
`prd_macos_notarisation`, `prd_deployment`, `prd_app-ios`, `prd_backend_railway`.
`naming::v1::doppler_root_config` derives one root config per environment (`dev`/`stg`/`prd`),
which matches **none** of them, and `doppler.config.ensure` reports a branch config sitting at that
name as `Foreign` (`config_ensure.rs:68-75`). Worse: `prd_app-ios` contains a hyphen, which
`DopplerConfigName`'s documented grammar (lowercase, digits, underscore) rejects outright. That is
a **type bug**, not a layout preference — a real Doppler config name this workspace cannot parse.

**Standalone repo vs monorepo directory.** The sample splits 3–2 (descartes, pessimal,
phil-connors / danksworth, brassica-ex), with phil-connors visibly crossing. Per
`todos/2026-09-20-ios-scaffolding-step-and-entitlements-as-inputs.md` this belongs to the document,
never to an input.

**Apple flavour.** descartes demonstrates Developer ID + notarytool (direct distribution);
pessimal, phil-connors and danksworth demonstrate Apple Distribution + provisioning profile + ASC
(App Store/TestFlight). Same provider, different object graph. Two documents, not a switch.

**Process vs configuration.** The monorepo root `CLAUDE.md` forbids feature branches; `ci.yml`
triggers on `pull_request` and pocket-claw's `provider-settings.json` sets
`build_pull_requests: true`. CI is configured for a branching model the process document bans.

### 4.3 One claim to state carefully

**Doppler service tokens are not the abandoned pattern; per-project *CI* tokens are.**
phil-connors still uses service tokens for Railway runtime (`DOPPLER_TOKEN` as a service variable)
and for the nested `DOPPLER_TOKEN_APP_IOS`. `doppler.service_token.ensure` keeps a job. What has
been superseded is minting one *per project for CI*, which the shared service account plus a grant
replaces.

Likewise, **the premise that Actions is being dropped is confirmed and sharpened**: phil-connors
has no `.github/` at all, and pocket-claw is the in-monorepo destination exemplar. But the
monorepo's shared Rust/Swift/Bazel gates have **not** crossed — `.github/workflows/ci.yml` is still
the only repo-wide CI. One app has migrated; the shared gates have not. A consequence worth naming:
dropping Actions orphans the AWS OIDC trust relationships scoped to
`token.actions.githubusercontent.com` across five accounts.

---

## 5. The catalogue

Nine documents. The build system and the monorepo layout live in the document, never in an input
(`todos/2026-09-20-ios-scaffolding-step-and-entitlements-as-inputs.md`): *"a parameter that selects
which tool runs cannot be checked statically."* So: several small documents, each statically
checkable end to end, some of them composable fragments rather than whole processes.

**Excluded deliberately:** "migrate an existing project into a monorepo", per
`todos/2026-09-20-agent-authored-workflows.md` — *"that's a future improvement"*. It needs stateful
cross-provider reads (the ASC build number behind `BUILD_NUMBER_OFFSET=400`) that no from-nothing
document has an equivalent of.

| # | Name | G/M | Provisions | Inputs | Derived from |
| --- | --- | --- | --- | --- | --- |
| 1 | `project-docs-scaffold` | both | Files only, no provider call: `docs/HANDOFF.md` with a dated RESUME block, `docs/plans/`, `docs/solutions/`, `todos/`, the agent-instruction file, README skeleton, LICENSE | `slug`, `agent_file_name` (AGENTS.md \| CLAUDE.md \| both), `todo_convention`, `license` | The base layer, all five |
| 2 | `new-rust-service-buildkite` | G | **Exists.** Repo + Doppler project + configs + Buildkite pipeline | `slug`, `org`, `buildkite_org`, `cluster`, `visibility`, `environments` | willikins' own fixture; inert until #1's files exist |
| 3 | `new-rust-cli-repo` | G | #2 plus: release-on-tag pipeline, GitHub Releases as artifact store, a Homebrew tap entry, a release token scoped to two repos, a purpose config (`prd_<purpose>`) | `slug`, `org`, `buildkite_org`, `cluster`, `tap_repo`, `purpose_configs` | descartes; pessimal's agent half |
| 4 | `new-ios-app-repo` | G | Own `MODULE.bazel`/`.bazelrc`/`.bazelversion`/`platforms/`, `tools/uniffi/`, Apple App ID + capabilities + profile + ASC record, Doppler `prd_ios_deployment`, a Buildkite pipeline with `build_pull_request_forks: false` | `slug`, `org`, `buildkite_org`, `bundle_id_prefix`, `apple_team`, `entitlements`, `cluster` | pessimal |
| 5 | `new-ios-app-in-monorepo` | M | No repo, no licence, no MODULE.bazel. A directory: `BUILD.bazel` with real `rust_library`/`swift_library`/`rules_apple` targets, `ios/Resources/` (Info.plist, entitlements per bundle, `PrivacyInfo.xcprivacy`, AppIcon), a Rust core crate + UniFFI wiring; **edits** the root `Cargo.toml` members and `build/visibility/BUILD.bazel`'s `package_group`; Apple identifiers | `monorepo`, `app_path`, `slug`, `bundle_id_prefix`, `apple_team`, `entitlements`, `extension_bundles` | danksworth's Apple half + phil-connors/pessimal's UniFFI half. **This is Walter.** |
| 6 | `new-rust-crate-in-monorepo` | M | Files only: role directories, thin inheriting `Cargo.toml`s, a `BUILD.bazel` (stub or real); **edits** root `members` and `[workspace.dependencies]`; regenerates `Cargo.lock` | `monorepo`, `app_path`, `slug`, `crate_roles`, `new_workspace_deps` | brassica-ex |
| 7 | `buildkite-pipeline-for-monorepo-app` | M | An app-scoped `.buildkite/` directory, a pipeline whose bootstrap uploads *that* file, `provider-settings` incl. `build_tags`, `build_pull_request_forks: false` and the `filter_condition` regex that replaces `paths-ignore`, agent queue, concurrency group, pinned image | `monorepo`, `app_path`, `buildkite_org`, `cluster`, `queue`, `tag_regex` | pocket-claw (`apps/pocket-claw/.buildkite/`) |
| 8 | `apple-signing-config` (fragment) | both | A Doppler `prd_<purpose>` config, an assertion that the required secret **names** exist (cert p12 + password, ASC key id/issuer/p8), the CI service account grant. Never the values. | `doppler_project`, `purpose`, `required_secret_names` | descartes (`prd_notarisation`), pessimal (`prd_ios_deployment`), phil-connors (`prd_deployment`) |
| 9 | `add-railway-backend` (fragment) | both | Railway project/service/environment, a Postgres service, the cross-service `DATABASE_URL` reference, a volume, a runtime config `prd_backend_<target>` and its service token | `railway_project`, `service`, `environment`, `doppler_project` | phil-connors (`railway.toml` + `infra/railway/provision.sh`) |

Composition: a real process is #1 + one of {#2,#3,#4,#5,#6} + optionally #7, #8, #9. That is the
"pick one, compose existing ones, or write a new one" surface the operator described, and it is why
the catalogue needs names and technology tags rather than three files on disk.

---

## 6. The gaps, ordered by how many catalogue entries each unblocks

### Rank 1 — the one missing capability: **no tool can put a file in a repository**. Unblocks 9 of 9.

The GitHub provider has exactly two tools: `repo_ensure.rs` and `actions_secret_ensure.rs`. Every
entry above is mostly files. It is not a missing nicety — it breaks a step willikins already ships:
`new-rust-service-buildkite.yaml` creates a pipeline *"bootstrapped to read the repository's own
.buildkite/pipeline.yml"*, a file willikins cannot write. **The graph today provisions a pipeline
that cannot run.** For the greenfield case the cheap first version is not a templating engine —
GitHub creates a repository from a template repository in one call, so the hard part is authored
once as a real, testable repository and willikins stamps it and edits the few derived files.

### Rank 2 — **structured edit of an existing file**. Unblocks all three monorepo entries (#5, #6, #7).

Distinct from Rank 1 and harder. Adding brassica-ex meant appending four strings to a `members`
array and six rows to `[workspace.dependencies]` in a file the whole repository owns; adding an
iOS app means a row in `build/visibility/BUILD.bazel`'s `package_group`. A blind file write
destroys the other entries. willikins' `Ensured`/observation model is built around a provider you
can GET and compare and has no obvious analogue for "read this TOML, add this element if absent,
write it back idempotently, concurrently safe". Since three of the operator's five upcoming
projects are monorepo-shaped, this is not a later problem.

### Rank 3 — **an Apple provider**. Unblocks #4, #5, #8; three of the five upcoming projects.

App IDs, the 28-member `CapabilityType` enum, provisioning profiles (whose *name* must match in
three places), ASC API keys, TestFlight. `docs/research/2026-09-16-app-store-connect.md` already has
the API shape, and `todos/2026-09-20-ios-scaffolding-step-and-entitlements-as-inputs.md` has the
typing: entitlements are ordinary inputs, one value feeding both the capability ensure and the
scaffolded entitlements file, which makes "enabled on the identifier but missing from the app"
inexpressible. Evidence this is worth automating: pessimal's first release took eight attempts, two
of the failures from Apple setup, one of them two identically-named certificates differing by an
accent.

### Rank 4 — **`doppler.project_member.ensure`**. Unblocks every entry that touches a secret (#2,#3,#4,#5,#8,#9).

willikins' own fixture header admits it is manual. Under the post-2026-09-14 shared-service-account
pattern it is the *only* Doppler auth step that matters, and it is what replaces
`github.actions_secret.ensure` in a Buildkite world: Actions wants secrets pushed *into* the repo,
Buildkite-plus-Doppler has the agent *pull* them.

### Rank 5 — **naming v2 rows, and one type-grammar bug**. Unblocks #2,#3,#4,#5,#6.

`naming::v1` is frozen by invariant, so this is a v2 or explicit workflow inputs, never an edit.
Needed: an org-prefixed Doppler project (`lightless-labs-<slug>`) alongside the bare form; a
purpose config (`prd_<purpose>`); a Rust crate name (`<org>_<app>_<role>`); a Swift module prefix; a
bundle identifier; a Bazel package path; a release-tag prefix. **Separately and urgently:**
`DopplerConfigName`'s grammar rejects `prd_app-ios`, a config that exists in production. That is a
bug in the type, fixable without touching `naming::v1`.

### Rank 6 — **`buildkite.pipeline.ensure` is too thin**. Unblocks #3,#4,#5,#7.

It takes `org`, `slug`, `repo`, `cluster`. It cannot set `build_pull_request_forks` (pessimal lists
that under **Cautions** — a public repo on a cluster holding signing certificates), `build_tags`,
the `filter_condition` regex that is the *only* path filter a monorepo pipeline has, a non-root
pipeline file path (this monorepo puts them at `apps/<slug>/.buildkite/`), the agent queue, or the
concurrency group. A pipeline created today is pointed at a repo and otherwise inert.

### Rank 7 — **secret sinks, split carefully**. Unblocks #8, #9.

Three different things, with different answers:
- **`doppler.secret.set` as a sink for a willikins-*produced* secret output.** phil-connors' nested
  token — a `prd_app-ios` service token written as `DOPPLER_TOKEN_APP_IOS` into `prd_deployment` —
  is a secret output binding to a secret-accepting input, exactly what the invariants permit and
  `check` enforces. This is buildable. So is `GH_CLONE_TOKEN` if willikins ever mints it.
- **Placing operator-held blobs** (Apple p12s, ASC `.p8`, third-party API keys). Correctly manual
  and should stay so.
- **Asserting that a set of secret *names* exists in a config.** A name is not a secret. Its
  absence cost pessimal a whole release attempt (*"Doppler returned no value for
  APPLE_TEAM_ID"*). Read-only, invariant-safe, cheap, and it is descartes'
  `check-homebrew-tap-token.sh` instinct generalised: **verify the grant before the moment you need
  it, never by exercising it.** willikins has read tools but no notion of a precondition a workflow
  runs and reports on. Strongly recommended.

### Rank 8 — **a workflow catalogue surface**. Unblocks the operator's stated ask directly.

`workflows/` is three hand-written YAML files with no machine-readable technology tags, no listing
surface and no composition primitive. The organisation already does this by hand and says so in
comments: pessimal's `MODULE.bazel` copies *"the set proven in kumbaya, phil-connors,
bande-a-bonnot"* with one justified deviation; its pipeline copies *"the shape of Descartes'"*. A
catalogue is those two comments mechanised. Per
`todos/2026-09-20-agent-authored-workflows.md`, agent *authorship* is a future problem and reuses
the existing approval machinery; naming and listing is not.

### Rank 9 — **a Railway provider**. Unblocks #9 only, but blocks one named upcoming project.

`infra/railway/provision.sh` already shows the exact shape: check-before-act, read-only by default,
able to express a topology `railway.toml` cannot (a database service, a volume, a cross-service
variable reference). Note the live-fire hazard it records: a cross-service reference can store
successfully and resolve to an **empty string** when the service name carries stray whitespace.

### Rank 10 — **multi-credential routing**. Hit by the first real project surveyed.

phil-connors clones through `elfitz-github.com` (a second GitHub identity), lives in
Bande-a-Bonnot, and depends on a pinned plugin in Lightless-Labs. One run legitimately needs
credentials for two orgs. willikins holds one `WILLIKINS_GITHUB_TOKEN`. Already a design item in
memory; now it has a concrete instance.

### Not willikins' problem, and saying so is part of the answer

- **Agent fleet and VM images.** Queues, the token in the agent environment, the git credential for
  the private plugin repo, baked Tart images. Shared infrastructure, not per-project. Favourably
  for Walter: `ci-macos-rust-bazel-ios-20260910-v2` is literally named for its stack and exists.
- **Apple objects the API cannot create.** App records (website only, gated behind an agreement),
  app groups and iCloud containers (absent from the API), the distribution certificate (its request
  needs a private key on a developer's machine, and a team gets one of each type — a workflow that
  mints a second is *actively harmful*). All three are in the ASC research with evidence.
- **Regenerating lockfiles.** `Cargo.lock` after a `members` change, `MODULE.bazel.lock` (checked
  in, with `--lockfile_mode=error` in CI) require executing a toolchain. The "no shell command as a
  tool input" invariant is a deliberate wall. Either a narrow typed tool, or the workflow ends by
  handing an agent a checkout to finish.
- **Decommissioning.** Idempotent-ensure is the right primitive for creation and says nothing about
  removal. willikins cannot delete `.github/workflows/danksworth-beta.yml`, and should not try.
- **Minting third-party API keys** (Anthropic, ElevenLabs, Mapbox, SigNoz, …). Nine relationships
  for phil-connors alone. What willikins *can* do is assert the names exist (Rank 7).
- **The first green build.** Nothing expresses "this workflow is done when the first build passes",
  which is the only real proof any of it worked. Arguably a brief, not a tool.

### What should come out as a written artefact instead of silence

descartes' answer to the un-automatable remainder is `docs/operator/` for humans and
`*-validation-brief.md` for an agent on a real host. willikins has no concept of emitting a runbook,
a brief or a preflight for the steps its graph cannot reach. Given how much of the Apple chain is
permanently manual, that emission is a feature, not a consolation prize.

---

## 7. The operator's five upcoming projects

Three iOS/watchOS apps (one with a backend) and two open-source self-hostable tools for AI agents
(likely a CLI plus an OAuth remote MCP server, perhaps a web app).

**1. Walter — iOS/watchOS health app, Rust core + SwiftUI, Bazel, local-only, no backend.**
`~/Projects/bande-a-bonnot/apps/walter` is an empty directory; it does not even have the
name-reservation `BUILD.bazel` its siblings carry. → **Catalogue #5**, plus #1, plus #7, plus #8.
Missing: file-writing (Rank 1); **structured edits** to the root `Cargo.toml` members and
`build/visibility/BUILD.bazel` package_group (Rank 2); the Apple provider for the App ID,
**HealthKit** capability, the watch-app bundle and its App Group, and the profiles (Rank 3);
`project_member.ensure` (Rank 4); the naming v2 rows (Rank 5); the pipeline fields, especially the
`filter_condition` that keeps every commit to every sibling from building Walter (Rank 6).
Free for Walter: the CI image, the cluster, the team distribution certificate, the ASC API key, the
root `MODULE.bazel`/`.bazelrc` — all **ensure-and-share**, never create. Two non-obvious blockers:
`cog.toml`'s `tag_prefix = "v"` cannot serve nine apps in one tag namespace, and the monorepo pins
`rules_rust` 0.68.1 / Rust 1.88.0, so a crate with a higher MSRV silently demands a monorepo-wide
bump affecting nine apps — willikins has no notion of a pin a new project must live inside.
Also: danksworth is the right template for Walter's Apple half and the **wrong** one for its core
half (no Rust, no UniFFI). A composed workflow, not a copied one.

**2. Second iOS/watchOS app, no backend.** → identical to Walter: **#5 + #1 + #7 + #8**. Whether it
lands in the monorepo or stands alone decides #5 vs #4; that is a choice of document, not an input.
Its entitlement set differs, and entitlements are an input.

**3. Third iOS app, with a backend.** → **#5 (or #4) + #1 + #7 + #8 + #9**. Everything above plus
the Railway provider (Rank 9), plus a second and third Doppler config, because a backend's runtime
secrets are distinct from its build secrets — that is why phil-connors has three live configs and
not one. The Postgres service, the cross-service variable reference and the volume all have to be
expressible; `railway.toml` itself cannot express them, which is why a hand-written imperative
provisioner exists.

**4. A CLI tool for AI agents, open-source and self-hostable.** → **#3 + #1** (greenfield repo,
Cargo, Buildkite, release on tag, GitHub Releases, Homebrew tap). **descartes is the template** for
the distribution half — it is the org's only worked example of shipping a CLI to end users, and it
does it without ever publishing to a registry: a git tarball plus a git-backed tap. Missing:
file-writing (Rank 1), the tap token scoped to two repositories (Rank 10-adjacent — nothing models
a GitHub token's scope), `project_member.ensure` (Rank 4). Note the open-source-first memory item:
the tap and GitHub Releases are the only distribution channel in the sample, and neither requires
the *user* to hold an account. Also steal brassica-ex's `docs/agent-cli-protocol.md` shape — one
process per move, one JSON document on stdout, empty stderr, distinct exit codes — as the agent
interface convention.

**5. An OAuth remote MCP server, perhaps with a web app.** → **#3 + #1 + #9**, and here
**willikins is its own template — for the product shape, not the CI shape.** willikins already
*is* a self-hostable Rust OAuth remote MCP server with a CLI: AGPL-3.0-or-later with the LICENSE
actually present, a Dockerfile, Railway deployment (`.railway/`, memory: project Willikins /
service willikins / production, GitHub auto-deploy via the Dockerfile builder), its own
authorisation server in Rust with no vendor IdP, and OAuth 2.1 on MCP scheduled for milestone 2c.
That is the product template and it is a good one. But `ls -A` shows willikins has **no
`.buildkite` and no `.github`** — no CI at all, in a repository whose `CLAUDE.md` names four gates
and says to run all four before every commit. So for the CI half, descartes is the template and
willikins is not. Missing for this project: file-writing, the Railway provider (Rank 9), and — if
the web app gets a public domain — a domain/DNS surface nothing in this sample exercises.

**The honest tally.** Of the work these five need, willikins can express today: a Doppler project,
its root configs, a service token, reading a cluster, and a pipeline shell. Roughly one step of
twelve for Walter. Everything else is either files (Rank 1–2), a provider that does not exist
(Rank 3, 9), a grant (Rank 4), or correctly manual.

---

## 8. Sequencing, and the blunt version

Say it plainly: **this is not mostly a provisioning problem.** Counting the "would have needed"
lists across five projects, provider API calls are a minority of every one and are *zero* in
brassica-ex. What the operator forgets is not "create the Doppler project" — it is the LICENSE
(four times out of five), the linter (descartes' Node CLI), the gate that the docs prescribe and
CI does not run, the `core.hooksPath` that leaves gitleaks inert, the per-app Doppler project
(danksworth borrowed a sibling's), and the key that ended up inside a shipped bundle. Every one of
those is a file, or the absence of one.

Three consequences for sequencing:

1. **Rank 1 and Rank 2 come before every provider.** A new Apple provider on top of a willikins
   that cannot write `.buildkite/pipeline.yml` produces a pipeline that cannot run and an
   entitlement enabled on an identifier the app does not claim. The catalogue is nine documents
   with a shared prerequisite and there is no useful order that puts it second.
2. **The cheapest first cut of Rank 1 is a template repository, not a templating engine.** The hard
   part — making Bazel, `rules_rust`, UniFFI and `rules_apple` build together — is authored once as
   a real repository that can be tested, and willikins stamps it and edits the derived values. That
   is one GitHub call. Full templating with layered profiles is a milestone of its own.
3. **Catalogue entry #1 (`project-docs-scaffold`) is the highest value per unit of risk in the whole
   list.** It touches no provider, spends no credential, cannot leak anything, and it is precisely
   the category the operator says they keep forgetting. It is also the only entry that is *already*
   unblocked the moment Rank 1 lands.

And one pattern to steal outright, from `scripts/check-homebrew-tap-token.sh`: it exists because a
release once needed a credential that turned out not to have write access. It proves — read-only,
before the tag, printing no value — that the token can read the target file and that the provider
reports push permission. That is a provisioning butler's instinct already written down in the
operator's own estate, and willikins has no equivalent.
