# Milestone 2c: authorization — OAuth 2.1 on `/mcp`, a browser login on `/approvals`

**Created:** 2026-09-16
**Reviewed:** 2026-09-16 (via document-review workflow: coherence, feasibility, security-lens, scope-guardian, adversarial; findings folded in below)
**Addendum:** 2026-09-16 (operator) — three decisions the operator made on reading the review: the
identity provider must be **self-hostable open source**, because willikins is an open-source tool
and an open-source tool may not require a third-party SaaS account to run ("Willikins is *open
source*, *first and foremost*. Since when do you make open source tools dependent on third-party
SaaS?"); the public host is **`willikins.bandeabonnot.com`**; and no identifier is permanent ("But
no, nothing is forever"), so the audience gets a documented migration path instead of a promise of
permanence. Decisions 10, 12, 19 and the go-live sequence carry them.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (the 2026-09-15 addendum and milestone
2c in the milestone list)
**Previous:** `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`
**Research:** `docs/research/2026-09-16-m2c-authorization.md` (six parallel passes, every fact
quoted from a source fetched on 2026-09-16), plus
`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md` for the exposure hand-overs.

Every section below that rests on something the research note could not settle says so in place
and names the verify item it waits on. Nothing from the note's section 7 is written into frozen
code here; where a decision touches one, the decision is stated as conditional and the condition
is named.

## Goal

The service gets a public domain. An MCP client that has never met this deployment discovers
where to get a token, gets one from the operator's identity provider bound to this server as its
audience, and calls the eight tools; willikins validates that token itself, on every request,
and never forwards it anywhere. The operator approves a plan by logging in with the same
identity in a browser instead of typing a Basic-auth password the browser then stores. The static
bearer hashes and `hash-token` are gone: no shared secret authenticates a *caller* to willikins
any more (trust boundary 5 names the one static secret that remains and says why it is not one).

The milestone is done when every acceptance test below passes, including adversarial pass 3
against the in-process fake authorization server, and one opt-in live token test against the
provider the operator chooses has validated a real token end to end.

## Out of scope

- **willikins hosting its own authorization server.** The specification puts the authorization
  server's implementation out of scope and allows it co-hosted or separate (research §1.2), and
  every authorization-server MUST — exact `redirect_uri` matching, refresh-token rotation,
  `code_challenge_methods_supported`, RFC 7591/8414 endpoints — lands only on that branch
  (research §2.7). Delegating is one configuration block; hosting is a milestone.
- **A per-user upstream OAuth flow to GitHub or Doppler.** This is the fork research §8 says the
  plan must decide before task 1, and it is decided here for the first branch: operator-
  provisioned credentials still reach GitHub and Doppler. willikins therefore is not an "MCP
  Proxy Server" in the specification's defined sense and the confused-deputy MUSTs (a per-user
  registry of approved `client_id`s consulted before forwarding upstream, an MCP-owned consent
  page) do not apply. Credential routing across several GitHub organizations and Doppler
  workplaces is milestone 3's work, which is the same reason.
- **Token introspection (RFC 7662) and opaque access tokens.** RFC 7662 was not fetched at all
  (research §7.3), and an introspection round trip sits inside a handler with a 30 s budget on
  every single request. This milestone validates RFC 9068 JWTs offline against a cached JWKS.
  "The provider issues JWT access tokens for the registered resource" is therefore a
  precondition on the operator's provider choice, checked by the live token test (test 18), not
  a third must-have and not a code path.
- **Sender-constrained tokens.** RFC 9449 (DPoP) and RFC 8705 (mutual-TLS) were named by fetched
  text but not fetched (research §7.3). Bearer tokens over the edge's TLS are what the
  specification's own transport section describes (research §1.2).
- **Dynamic client registration or Client ID Metadata Documents implemented by willikins.** Both
  are client and authorization-server obligations at every revision (research §1.2, §1.3). What
  willikins does is require that the chosen provider offers one of them; see decision 10.
- **Multi-tenancy.** One server still serves one GitHub organization and one Doppler workplace
  until milestone 3's credential routing. The public deployment changes who can reach the
  server, not how many tenants it serves.
- **The pass-2 hand-overs this milestone does not take**, named one by one so the split is not a
  judgement call later (`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`, "Handed to
  milestone 3 (or 2c, the OAuth milestone)"). Items **1, 2, 3 and 9** are *in* scope and are tasks
  9a to 9d below. Item **4** — revisit the concurrency bound with a real measurement, and make it
  configurable only if a deployment needs it — stays milestone 3's: 9d moves the existing permit
  and changes no bound. Items **5 to 8** — validating tool outputs against declared port types and
  the distinguishable redaction marker, measuring the journal fold before caching it, a
  `journal_version` field, and `willikins journal repair` — stay milestone 3's, with one carve-out:
  the fold measurement (item 6) is pulled into adversarial pass 3's scope by review resolution 2
  below, because a public listener makes the fold's cost attacker-reachable. Item **10** — tighten the
  authenticated foreign-`Host` assertion to the status rmcp actually returns — goes into pass 3.
- Push notifications, chat integration, MCP elicitation, composition (2b), templates (3).

## Trust boundaries

Milestone 2's five boundaries still hold, with one amendment: boundary 5's sentence "INFO is the
ceiling: the binary installs no `EnvFilter` and does not read `RUST_LOG`" is replaced by task 9c
below, which gives the operator a level and sweeps what rmcp emits at it. This milestone adds
five boundaries. Each is normative for the crates below and has an acceptance test.

1. **willikins validates tokens and issues none.** At `/mcp` it is an OAuth 2.1 resource server
   and nothing else. It holds no signing key, mints no access token, runs no `/authorize`,
   `/token` or `/register` endpoint, and stores no refresh token. Its only cryptographic input
   is a JWKS of public keys fetched from the configured authorization server. Tests 1, 2, 3.
2. **The inbound token is a credential for this server and for nothing else.** It is never
   attached to a GitHub or Doppler request, never exchanged for an upstream token, never written
   to the journal, a log line, or an error message, and never reaches a tool handler: the auth
   middleware removes `Authorization`, `Cookie` and `Proxy-Authorization` before `next.run`,
   because rmcp verifiably inserts the complete request `Parts` — headers included — into every
   handler's extensions (research §3.4). The **`Credential` methods in `willikins-providers-http`
   are the only sites in the process that put a credential on an outgoing wire**, and the inbound
   token is never an input to any of them: the approvals login's client secret joins them as a
   `Credential` (decision 4), so the set grows by one named method and not by one new site.
   `clippy.toml`'s `disallowed-methods` entry and
   `crates/willikins-core/tests/expose_secret_guard.rs` are what keep that checkable. Test 6.
3. **Two surfaces, two credential kinds, two configured allowlists.** `/mcp` accepts an
   `Authorization: Bearer` access token and never a session cookie. `/approvals` accepts a session
   cookie and never a bearer token — it does not so much as look at an `Authorization` header.
   Neither surface treats a scope as authority on its own. On `/mcp` a validated token acts only
   if its `sub` is listed in `WILLIKINS_AGENT_SUBJECTS`, and the scopes it carries then decide
   which tools it may call. On `/approvals` a session approves only if its `sub` is listed in
   `WILLIKINS_APPROVER_SUBJECTS` **and** its token carried `willikins:approve`. Milestone 2's "the
   approver hash is not an agent hash" refusal has no analogue — the same human may legitimately
   hold both an MCP client token and a browser session, and may be listed in both variables — so
   the separation becomes structural rather than a startup comparison. Tests 7, 9, 11.
4. **Every principal is derived from a validated token, never supplied.** No request field, no
   header and no tool parameter can name the caller. The principal is computed from claims that
   survived validation, and the claims it is computed from travel with it to the journal. Test 8.
5. **No static shared secret authenticates a caller to willikins.** The agent token hashes, the
   approver token hash and the `hash-token` subcommand are removed, not deprecated, and a
   deployment that still sets either variable refuses to start rather than ignoring it. The design
   doc's own words for this milestone are "short-lived credentials, no static bearer tokens". One
   static secret remains in the process and is not a counter-example: the approvals login's
   `WILLIKINS_OAUTH_CLIENT_SECRET`, which authenticates *willikins to the provider's token
   endpoint* and never authenticates anyone to willikins. Its blast radius is bounded to
   "completes a login as willikins' approvals client", which the login-cookie binding of decision 4
   and the two subject allowlists make useless on its own: holding it grants no session, no
   principal, and no approval. Test 12.

## Decisions

Ten decisions the milestone cannot start without, each with its reasoning and the acceptance
test that pins it, then eight smaller ones the research note left unresolved and the plan may
not leave silent.

### 1. Resource server only

willikins validates access tokens at `/mcp`, publishes an RFC 9728 protected-resource-metadata
document, and answers an unauthenticated or badly authenticated request with a 401 that points
at that document. It issues nothing.

**The validation set**, in the order it runs, for a JWT-format access token. RFC 9068 §4 is the
list; the MCP profile is what makes the audience check a server-side MUST (research §2.4, §2.5):

0. The credential parses as a compact JWS at all: three base64url segments with a decodable JOSE
   header. Decision 1's remaining checks all read a parsed header, so "not a JWT" is its own
   refusal (`AuthFailedReason::Unparseable`) rather than an unnamed fall-through.
1. `typ` header is in the frozen accepted set: `at+jwt` or `application/at+jwt` (decision 14).
2. `alg` header is in the configured allowlist, which may contain only asymmetric families
   (decision 15). `none` never validates, and `jsonwebtoken` refuses a verifier whose key family
   differs from an allowed algorithm's family (research §4.2).
3. Signature verifies against the JWKS key whose `kid` matches the header's (decision 16).
4. `iss` matches `WILLIKINS_OAUTH_ISSUER` exactly, as a string.
5. `aud` contains this server's resource identifier, `<WILLIKINS_PUBLIC_URL>/mcp` — derived, never
   configured (decision 12) — or, while a migration is in progress, one of the identifiers listed
   in `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`, which are accepted but never published. RFC 9068 says
   "contains", so an `aud` array carrying an accepted resource among others is accepted.
6. `exp` is in the future, within `WILLIKINS_OAUTH_LEEWAY_SECONDS` of clock skew (decision 17).
   `nbf`, when present, is validated too — `jsonwebtoken`'s `validate_nbf` defaults to false and
   is overridden.
7. `exp`, `aud`, `iss` and `sub` are required claims. `jsonwebtoken`'s `required_spec_claims`
   defaults to `{"exp"}` only, so a token with no `aud` at all would otherwise pass its audience
   check (research §4.2); `set_required_spec_claims(&["exp", "aud", "iss", "sub"])` is what closes
   that. `sub` is on the list because decision 2 derives the principal from it and decision 3's two
   allowlists compare against it: a token with no `sub` has no identity this server can journal or
   authorize, so it is refused with `AuthFailedReason::MissingSubject` rather than given a
   principal derived from an empty string. The fake authorization server mints a `sub`-less token
   as one of its flaws so this is a test and not an assumption.

Every failure in that list answers **401** with `error="invalid_token"`, which is RFC 9068 §4's
own instruction, and is journaled as `AuthFailed` with a reason that says which check failed.

**After a token validates, two more refusals run before any tool does**, and both are 403 rather
than 401 because the credential was good and the authority was not: a `sub` that is not in
`WILLIKINS_AGENT_SUBJECTS` is `AuthFailedReason::UnlistedSubject`, and a scope set that does not
admit the tool named in the body is `AuthFailedReason::InsufficientScope { needed }` with
`error="insufficient_scope"` (decision 3). The specification reserves **400** for a malformed
authorization request. Whether a non-`Bearer` `Authorization` header counts as malformed (400) or
simply absent (401) is verify item 13 — RFC 6750 §3.1 was not fetched — so today's behaviour, 401
`MissingCredential`, is kept until it is. The status table itself is the specification's, verbatim
(research §1.2).

**Every check, and the `AuthFailedReason` it journals.** The variants are additive to
`willikins_journal::AuthFailedReason` (`crates/willikins-journal/src/event.rs`), which today holds
`MissingCredential`, `InvalidCredential`, `WrongRole`, `InvalidNonce`, `ForeignOrigin` and
`MalformedUsername`.

| Check | Status | Reason variant | State |
| --- | --- | --- | --- |
| No `Authorization` header, or a non-`Bearer` one, or a token only in the query string | 401 | `MissingCredential` | retained |
| Not a parseable compact JWS | 401 | `Unparseable` | new |
| `typ` outside the frozen set | 401 | `InvalidType` | new |
| `alg` outside the allowlist, or `none`, or a family mismatch | 401 | `InvalidAlgorithm` | new |
| No JWKS key for the `kid`, or a header with no `kid` | 401 | `UnknownKey` | new |
| Signature does not verify | 401 | `InvalidSignature` | new |
| `iss` missing or not the configured issuer | 401 | `InvalidIssuer` | new |
| `aud` missing or not containing this resource | 401 | `InvalidAudience` | new |
| `exp` past the leeway, or `nbf` in the future | 401 | `TokenExpired` | new |
| `sub` absent | 401 | `MissingSubject` | new |
| `sub` not in `WILLIKINS_AGENT_SUBJECTS` | 403 | `UnlistedSubject` | new |
| The token's scopes do not admit the tool named | 403 | `InsufficientScope { needed }` | new |
| A session whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS`, or whose token lacked `willikins:approve` | 403 | `WrongRole` | retained |
| A `POST /approvals/{plan_id}` with a missing, spent, foreign or expired nonce | 403 | `InvalidNonce` | retained |
| A `POST /approvals/{plan_id}` from a foreign `Origin`/`Referer` | 403 | `ForeignOrigin` | retained |
| — | — | `InvalidCredential`, `MalformedUsername` | **retained on the wire, never produced after 2c** |

`InvalidCredential` and `MalformedUsername` are the two variants the hash and Basic credentials
produced. Removing a variant would break the journal's own additive-only discipline and would stop
`pre-pass-2-every-event.jsonl`, which carries both, from replaying — so they stay in the enum, stop
being emitted, and their doc comments say when they were last written. `AuthFailed` additionally
gains an **optional** `subject` field, written only for a refusal of a token that validated
(`UnlistedSubject`, `InsufficientScope`, `WrongRole`, `InvalidNonce`): before validation there is
no subject to name, and writing attacker-supplied text for an unvalidated credential is exactly
what trust boundary 2 forbids.

**Journaling a refusal is itself attacker-reachable, so it is rate-limited by reason.** Once the
listener is public, anyone can send a garbage `Authorization` header, and every one of those
refusals is a journal append plus a line the approvals page's fold has to read forever. The
refusals of a *credential-less or unverifiable* request — `MissingCredential`, `Unparseable`, and
every `invalid_token` case that happens before a validated principal exists — therefore pass
through a **per-reason token bucket, 10 per minute per reason, in memory**, the same shape as the
existing per-principal buckets. Refusals beyond the bucket are counted, not dropped silently: one
`AuthFailed` line per reason per window is written carrying a new **optional** `suppressed: <count>`
field (additive, like `subject`), so the record says "this happened 4,812 more times" rather than
losing it. Refusals of a token that *did* validate — `UnlistedSubject`, `InsufficientScope`,
`WrongRole`, `InvalidNonce` — are journaled one to one and are not bucketed: a validated principal
exists, it is already rate-limited as a principal, and those are the lines an operator most needs
whole. Pass 3 measures both halves: a flood of garbage tokens against journal growth and approvals-
page latency, and the fold cost itself.

**The document it serves.** One JSON object with `resource` (RFC 9728's only REQUIRED field),
`authorization_servers` with at least one entry (the MCP profile's MUST), `scopes_supported`
(RECOMMENDED), `bearer_methods_supported: ["header"]`, and `resource_name`. Parameters with zero
values are omitted, the response is 200 `application/json`, and the route is served outside the
bearer middleware and outside rmcp's host check exactly as `/healthz` is today — an
unauthenticated client must reach it before it holds a token, and `/healthz` is the existing
precedent for a root-level exemption (research §2.2, §3.3).

**The 401 shape.** `WWW-Authenticate: Bearer resource_metadata="<PRM URL>",
scope="willikins:read"`, plus `error="invalid_token"` when a token was presented and rejected. The
scope is **fixed**, not the scope the refused call needed, because the token is validated before
the body is read at all (decision 3) — so at 401 time the server does not know which tool was
asked for, and naming a minimal read scope is what the specification recommends for an initial
challenge anyway. The 403 of decision 3 is where the exact scope is named, and there the body has
been parsed. The specification's MUST is to implement *one of* the header or the well-known URI;
clients prefer the header, so willikins does both (research §1.2, §2.2).

**The passthrough prohibition, stated for a server that itself calls GitHub and Doppler.** The
specification says it twice: an MCP server must not pass through the token it received, and must
not accept a token not issued for it (research §1.2). willikins satisfies the first structurally
already, because no caller token has ever reached an upstream API and the `Credential` methods in
`willikins-providers-http` are the only outgoing-credential sites. This milestone's job is to keep
it that way while a real OAuth token is in the request: the inbound token is consumed by the
middleware, the header is stripped before rmcp sees it, and nothing downstream can read it.

*Pinned by tests 1, 2, 3 and 6.*

### 2. Principal identity

Today a principal is `agent-<12 hex of the token hash>`. After 2c it is
`oauth-<12 hex of sha256(iss ‖ 0x00 ‖ sub)>`, and the claims it was derived from are recorded
beside it.

**Why not the subject itself.** `PrincipalId`'s grammar is
`^[A-Za-z0-9][A-Za-z0-9._@-]{0,127}$`, declared as `PRINCIPAL_ID_PATTERN` at
`crates/willikins-core/src/apply/principal.rs:13` and re-exported through `willikins_journal`.
A raw `sub` need not fit it — a provider-qualified subject carrying a `|` is a common shape — and
the research note fetched no statement about any surveyed provider's subject format. A derived,
in-grammar, deterministic id keeps the existing discipline (the same identity always derives the
same principal, distinct identities never collide) and keeps provider-shaped text out of an
identifier that is compared, indexed and printed. The `iss` is folded in because a `sub` is only
unique within its issuer.

**What the journal records, and how the claims get there.** The claims themselves, as new
**optional** fields on every event that already carries a principal: `subject`, `issuer`,
`client_id`. They are authorization-server-supplied text, so each is bounded and escaped the way
`willikins_types::quoted` bounds a rejected literal before it is written, and each is omitted
when the claim is absent. `client_id` in particular must tolerate absence: RFC 9068 §2.2's claim
list was not fetched (only §4 was), so "a JWT access token always carries `client_id`" is a
verify item, not a fact this plan may rely on.

**The claims cannot reach the journal unless they cross the transport boundary, and today nothing
carries them.** `PrincipalId` is a single string; the middleware inserts it into the request
extensions and `mcp.rs`'s `principal_for` reads it back out. A derived id carries no `sub`, no
`iss` and no `client_id` by construction — that is the whole point of deriving it — so "the claims
are journaled beside the principal" is a signature change, not a field addition. The plan's answer:

```rust
pub struct Principal { pub id: PrincipalId, pub claims: Option<Claims> }
```

`Principal` is what the middleware inserts into the extensions, what `principal_for` returns, and
what **every `Butler` method that writes an event takes** in place of today's `PrincipalId`. That
is ten methods in `crates/willikins-server/src/butler.rs` — `list_workflows`, `validate`,
`describe`, `list_tools`, `propose_slug`, `plan`, `approve`, `reject`, `apply` — plus
`record_auth_failure`, which gains the parameter it does not have today (it takes only a
`Transport` and an `AuthFailedReason`). `claims` is `None` for every caller that is not an OAuth
caller: `willikins-cli`'s `--principal`, `serve --stdio`'s local principal, and the existing test
helpers. Task 5 lists every changed signature: the ten `Butler` methods, the eight `#[tool]`
methods in `crates/willikins-server/src/mcp.rs` and `principal_for` beside them, the approve and
reject handlers in `crates/willikins-server/src/http/approvals.rs`, `willikins-cli`'s commands in
`crates/willikins-cli/src/commands.rs`, and the shared helpers under
`crates/willikins-server/tests/common/`.

The claims repeat on every event that carries a principal rather than appearing once per session.
That is deliberate: the journal's fold does not join across events, so a line that does not carry
its own subject is a line an operator cannot read on its own, and "self-describing per line" is the
property every other field in the journal already has.

**The discipline.** This is an additive wire-format change, the only kind a journal task is
allowed, and it follows pass 2's own procedure exactly (milestone 2 plan, the 2026-09-15 task 13
addendum: `PlanRecorded.principal` was added optionally, three `AuthFailedReason` variants were
added, a journal frozen before the change replays through both readers, and the new shapes were
frozen as the next pass's baseline). So: a fixture frozen **before** this change
(`crates/willikins-journal/tests/fixtures/pre-2c-every-event.jsonl`, cut from the current binary
as task 2's first commit, before task 3 changes anything) replays through both readers after it,
and pass 3 freezes `post-2c-new-shapes.jsonl` as milestone 3's baseline. Both existing fixtures —
`pre-pass-2-every-event.jsonl` and pass 2's own `post-pass-2-new-shapes.jsonl` baseline — keep
replaying.

*Pinned by test 8.*

### 3. Scopes

Four server-defined scopes, in a hierarchy, mapped onto the eight MCP tools. The specification
defines only the machinery — `scopes_supported`, the `scope` challenge parameter, and a client
selection rule — names no registry and no reserved scope, and calls a wildcard or omnibus scope
a common mistake (research §1.2).

| Scope | Tools it admits |
| --- | --- |
| `willikins:read` | `validate`, `describe`, `list_workflows`, `list_tools`, `propose_slug`, `run_status` |
| `willikins:plan` | the above, plus `plan` |
| `willikins:apply` | the above, plus `apply` |
| `willikins:approve` | no MCP tool; the approvals page only |

`plan` is separated from the read tools because it is the only read-side tool that calls live
providers and spends the organization's GitHub and Doppler budget — which is why it already has
its own rate-limit bucket. `apply` is separated because it is the only tool that writes.
`willikins:approve` grants no tool at all: it is the browser-side intent, and decision 4 says
where the authority behind it lives.

**Which claim carries the scopes.** Both shapes are accepted: the RFC 6749 `scope` claim, a
single space-separated string, and an `scp` array of strings. A token carrying neither is treated
as carrying the empty set and is refused on every tool, never as carrying everything. Both are
accepted because no fetched sentence says which one the provider the operator has not yet chosen
emits; which it actually is, is a verify item test 18 prints along with the rest of the claim
names.

**A scope is not authority. The subject allowlists are.** This is the argument this decision used
to make for approval alone, applied now to every scope this server defines, because the property
that drives it is the same one: decision 10 *requires* a provider with an open registration path
(Client ID Metadata Documents or open Dynamic Client Registration), so on that provider the set of
clients that may *ask* for `willikins:apply` is open by design. A deployment whose only gate is
"the token says `willikins:apply`" is a deployment whose apply authority is whatever the provider's
consent screen hands out.

So: a new required variable **`WILLIKINS_AGENT_SUBJECTS`**, comma-separated `sub` values allowed to
hold any MCP scope at all. It is required in http mode; empty or unset is a startup refusal
(`HttpConfigError::EmptyAllowlist`), never a permissive default, so a deployment cannot become open
by omission. A token that passes every check in decision 1 but whose `sub` is not listed answers
**403** with `AuthFailedReason::UnlistedSubject`, and **no tool runs** — the check happens in the
middleware, before rmcp sees the request at all. Only within that allowlist do the token's scopes
decide read, plan or apply.

The result is that "who may call `apply`" is legible in the deployment's own configuration as
`WILLIKINS_AGENT_SUBJECTS` ∩ the tokens carrying `willikins:apply`, rather than in a provider
console an auditor may not have. `WILLIKINS_APPROVER_SUBJECTS` keeps exactly the same role for
approval, and a subject may legitimately be in both lists: the same human may run an agent and
approve its plans, which is the flow milestone 2 built for.

**Entries are bare `sub` values, not issuer-qualified ones**, because a deployment has exactly one
issuer: `WILLIKINS_OAUTH_ISSUER` is a single value and decision 1 compares `iss` against it by
exact string match, so a listed `sub` is unambiguous by construction. Two consequences are recorded
rather than designed around. Migrating to a different provider rewrites **both** lists, because
every subject is reissued; that is a go-live step, not a silent breakage, and the `WrongRole` page
of decision 4 is where an operator reads the new values. And if a deployment ever needs to trust
two issuers at once, the answer is issuer-qualified entries and a multi-issuer configuration block,
which is milestone 3's work — not a shape guessed at here for a case that does not exist.

**Where the check runs, and how the challenge knows what to name.** The tool name lives in the
JSON-RPC body, which rmcp parses *after* the middleware, and rmcp exposes no way for a handler to
set an authorization status: `jsonrpc_http_status` in
`rmcp-3.3.0/src/transport/streamable_http_server/tower.rs:626` maps a handler's error to exactly
**400, 404 or 200** and nothing else, so 401 and 403 are unreachable from inside a handler at the
version the lock holds. The scope check therefore runs **in the middleware**, in this order:

1. **Validate the token first, before reading one byte of the body.** An unauthenticated request
   answers 401 carrying the fixed `scope="willikins:read"` — legal, because the specification says
   the challenged scope set MAY be a subset, a superset or neither, and names a minimal read scope
   as the recommended initial set. A challenge that named the real scope would require parsing an
   unauthenticated body, which is the thing this ordering exists to avoid.
2. **Only for an authenticated request, buffer and parse the body.** The middleware is *not*
   downstream of a body cap: rmcp's 1 MiB cap is applied inside its own service
   (`expect_json(body, self.config.max_request_body_bytes)`, `tower.rs:1704`), and axum's
   `DefaultBodyLimit` is layered only on the approvals router
   (`crates/willikins-server/src/http/mod.rs`). So the middleware buffers under **the same
   `max_body_bytes` value**, answers **413** on overflow, and re-attaches the request body from the
   bytes it read.
3. **Parse with rmcp's own `ClientJsonRpcMessage`**, not a hand-rolled envelope, so there is one
   parser of record and the middleware cannot disagree with the handler about what a body says. A
   body that type cannot parse is refused **400 by the middleware**, before rmcp — fail closed. The
   type has four variants (`Request`, `Response`, `Notification`, `Error`; `rmcp-3.3.0/src/model.rs:714`)
   and **no batch variant**, so there is no "several `tools/call`s in one body" bypass to cover at
   3.3.0; a future rmcp that adds one makes this a scope check per element, and the plan says so
   here so the next reader looks.
4. **Scope-check only a parsed `tools/call`**, against the tool named in `params.name`: 403
   `insufficient_scope` with the exact scope that call needed. Every other parsed message —
   `initialize`, `ping`, `tools/list`, notifications, responses — passes once authenticated and is
   forwarded unchanged. A `tools/call` with no `name`, or a name no tool has, is **403**: an
   unknown name maps to no scope, and refusing it on authority rather than forwarding it keeps the
   middleware from having to know rmcp's tool table to stay closed.
5. **`GET` and `DELETE` at `/mcp` still require a token**, and then meet rmcp's own **405** with
   `Allow: POST` (`tower.rs:1502`, reached because `legacy_session_mode` is false), which is a
   transport answer this milestone does not change.

*Verify item 12 is settled for rmcp 3.3.0* by the `jsonrpc_http_status` reading above: there is no
handler-set status and the check stays in the middleware. It stays open only for 3.4.0, and only as
an opportunity — nothing here waits on it.

The hierarchy is a resource-server obligation at 2026-07-28 — "Servers **MUST** account for
scope hierarchies, where a broader scope implies narrower ones, when deciding whether a token is
sufficient for an operation" (research §1.3) — so a token carrying only `willikins:apply`
satisfies a `describe` call. `scopes_supported` lists exactly these four and never
`offline_access`, which that revision tells protected resources not to advertise. The 403's
`scope` attribute names only what the refused call needed, which is 2026-07-28's rule and the
reverse of 2025-11-25's; naming less is safe under both.

**Why an allowlist and not a claim or a group.** The claim name that carries roles or groups is
per-provider, and the research note fetched no group-claim name for any of the six providers, so
building on one would freeze a provider choice the operator has not made (decision 10). Two
comma-separated variables cost one deployment step each and are readable by anyone with access to
the deployment's configuration.

*Pinned by tests 7 and 11.*

### 4. The approvals page

Basic auth is removed. `GET /approvals` without a valid session answers **302 to
`/approvals/login`**; `/approvals/login` mints a pending login and **302**s to the provider's
authorization endpoint with `response_type=code`, `code_challenge_method=S256`, a fresh
`code_verifier`, a `state` value and the `resource` indicator; the callback exchanges the code at
the token endpoint with willikins' own client credentials and sets a session cookie.

**`/approvals` never inspects `Authorization`, at all, in any code path.** One rule, and it is the
whole of trust boundary 3's browser half: a request without a valid session gets the same response
whatever headers it carries. A `GET` is 302 to `/approvals/login` with a bearer header, with a
Basic header, and with neither. A `POST` is **403** in all three cases. There is no
`WWW-Authenticate` on any of them, and in particular no `WWW-Authenticate: Basic`, which would
re-summon the browser password prompt this milestone exists to delete. Tests 9 and 10 assert the
responses are byte-identical with and without the headers, because "identical" is the property, and
a surface that answers differently for a bearer token is a surface that reads one.

**willikins is a confidential client here.** A server-rendered page whose client credentials and
tokens stay on the server is draft-16 §2.1's "web application", which is a confidential client;
§9 "Browser-Based Apps" is still a TODO placeholder in draft-16 and cannot be relied on
(research §2.6). So the client secret lives in the process, reaches it through Doppler like the
two provider credentials, and is a `Credential` — the same `secrecy`-backed newtype, with the same
redacted `Debug` and no `Display` or `Serialize`.

**How it authenticates at the token endpoint: `client_secret_basic`.** That is RFC 6749's
mandatory-to-support method for a confidential client, so it is the one method a provider the
operator has not chosen yet is most likely to accept. It is sent through a **new
`Credential::authorize_basic(client_id)`** in `willikins-providers-http`, beside the existing
`authorize`. Two details of that crate's existing discipline are kept rather than bent:
`Credential::authorize` is `pub(crate)` on purpose — a returned builder carries the credential in a
header any caller could read back with `headers_ref()` — so `authorize_basic` is `pub(crate)` too,
and the token exchange reaches it through a new form-encoded `pub` entry point on
`willikins_providers_http::Http`, which sends the header and never hands it back. The invariant
after 2c reads: **the `Credential` methods in `willikins-providers-http` are the only sites that
put a credential on an outgoing wire, and the inbound token is never an input to any of them.**
`clippy.toml`'s `disallowed-methods` list and
`crates/willikins-core/tests/expose_secret_guard.rs` gain that one named method as an allowed
`expose_secret` site; nothing else changes about either guard.

*Rejected: a public client with PKCE and no secret at all.* It would remove one secret from the
deployment, and it would also remove client authentication at the token endpoint entirely, which is
the check that stops anyone who steals an authorization code from redeeming it. The secret's blast
radius is small and bounded in the other direction: holding it lets someone complete a login *as
willikins' approvals client*, which the login-cookie binding below and the two subject allowlists
make useless on its own — no session lands in anyone else's browser, and no unlisted `sub` approves
anything. A bounded secret beats an unauthenticated token endpoint.

**What the login asks for, and where the subject comes from.** The authorization request carries
`scope=openid willikins:approve` and `resource=<the derived audience>` — the same resource
indicator an MCP client sends — so the access token the callback receives is an access token for
*this* server. The `resource` parameter is sent on the **token request as well as the authorization
request**: RFC 8707 defines it on both, a provider may honour only the second, and sending it twice
costs nothing and closes the case where the authorization-time indicator is ignored. The fake
authorization server's `/token` endpoint *enforces* its presence, so "we send it on both" is a test
rather than a claim. The callback then runs the returned access token through `oauth::validate`,
the same function and the same configuration the `/mcp` middleware uses: one validation path, not
two. The session's `sub`, `iss` and granted scopes are the claims that validation returned, which
is what gives test 11's "the token lacked `willikins:approve`" a concrete meaning.

**The exchange is bounded in both directions.** `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS` (default 5)
caps how long the callback waits, so a hung token endpoint cannot hold a request or a runtime
worker, and the response body is read under a **fixed byte cap**: an oversized response fails the
callback rather than being buffered, because the token endpoint is a host willikins trusts for a
JWT and not for an unbounded body. The refresh token and id token in the token response are
**dropped unread** — willikins stores neither, and a session that cannot be silently refreshed is a
session whose TTL means what it says. A provider
that will not issue such a token for a browser login fails decision 10's audience must-have, and the
live test is where that shows.

**PKCE is used even though this is a confidential client**, because draft-16 §4.1.1 makes
`code_challenge` REQUIRED (the carve-out it points at, §7.5.1, was not fetched — research §7.3)
and `plain` is prohibited. `state` is a random value held **server-side**, in the pending-login
store, paired with its `code_verifier`, single-use, expiring in minutes; it is checked on the
callback, and so is the `iss` parameter when the authorization response carries one (2026-07-28's
rule for clients; research §1.3). A callback whose `state` is unknown, already used, or expired is
**400**, never a redirect.

**The pending login is bound to the browser that started it.** `/approvals/login` sets a
short-lived `__Host-willikins-login` cookie (`Secure`, `HttpOnly`, `SameSite=Lax`, 300 s) holding
the pending-login id, and the callback requires it to be present and to match the record the
`state` resolves to; a mismatch or an absent cookie is **400**. This replaces an earlier draft of
this decision that deliberately set no pre-login cookie on the argument that a login-CSRF "gains
nothing, because approval authority is the subject allowlist and not the session". That argument is
wrong in the two-approver case the adversarial reviewer described: **both** parties can be listed
in `WILLIKINS_APPROVER_SUBJECTS`. An approver-attacker starts a login, withholds their own callback
URL, and gets a second approver's browser to load it; the victim's browser now holds a session
minted from the *attacker's* authorization code, and every decision the victim makes is journaled
under the attacker's `sub` — an audit trail that names the wrong human, which is worse than a
refusal. The login cookie closes it: the callback only completes in the browser that began the
login.

**The session cookie**: name `__Host-willikins-session`, `Secure`, `HttpOnly`,
`SameSite=Lax`, `Path=/`, no `Domain`. Signed through `axum-extra`'s `SignedCookieJar` and
carrying an opaque session id only; the session record itself (subject, issuer, granted scopes,
expiry) is server-side and in memory. **`Lax`, not `Strict`, is required** and the reason is the
callback: it answers 302 to `/approvals`, and the browser must send the just-set session cookie on
that landing navigation, which arrives at the end of a cross-site redirect chain that began at the
provider — exactly the navigation `Strict` withholds cookies from. `Strict` is `tower-sessions`'
default and would break that hop (research §4.4). The corollary is stated rather than assumed:
`Lax` also withholds cookies on a cross-site **POST**, so a forged approval form submitted from
another site arrives with no session and is 403 before the nonce is even read — which makes the
per-plan nonce **defence in depth here, not the only defence**. The `__Host-` prefix, `Secure`,
`HttpOnly`, `SameSite` and "signed or server-side" are the confused-deputy consent-cookie checklist,
which is a MUST only for an MCP proxy server's consent page and is adopted here as the closest
applicable analogue (research §1.2, §8). `X-Frame-Options: DENY` and a CSP carrying
`frame-ancestors 'none'` and `form-action 'self'` are set on the page — the second so that a
markup-injection bug on the approvals page cannot retarget the approve form at another origin. The
signing `Key` is generated at startup and never configured: a session surviving a redeploy is not
wanted (see below), so there is nothing to keep stable, and one fewer secret is one fewer secret.

**Both stores are bounded, and say so when they fill.** The pending-login store: TTL 300 s, pruned
on insert, capped at **1,024** entries, answering **503** when full rather than evicting a login
someone is in the middle of. The session store: the same shape, capped at **1,024**, TTL
`WILLIKINS_SESSION_TTL_SECONDS`, pruned on insert. Both caps exist because both stores are filled
by unauthenticated requests — anyone who can reach `/approvals/login` can mint a pending login —
and an unbounded in-memory map behind an anonymous endpoint is a memory-exhaustion path. Pass 3
floods each of them.

**Logging out.** `POST /approvals/logout` drops the session record and clears the cookie. It is
named here because it is the only revocation there is: see Risks.

**The `WrongRole` page tells the operator their own identity.** A session whose `sub` is not in
`WILLIKINS_APPROVER_SUBJECTS` gets a 403 page that displays that session's own `sub` and `iss` —
the values the provider actually issued, escaped the way every other authorization-server-supplied
string on that page is. Without it the operator has no way to learn the value to put in the
variable: the subject is a provider-generated identifier that appears in no console willikins
controls, and "log in, read it off the refusal, put it in the allowlist" is the bootstrap. The same
values are what a provider migration rewrites (decision 3).

**Everything milestone 2 built on the page stays.** The single-use per-plan nonce, the
Origin/Referer check (made IPv6-aware by task 9b), the body cap, and the journaling of every
refusal with the `AuthFailedReason` that is literally true of it.

**Lifetime.** The session's lifetime is `WILLIKINS_SESSION_TTL_SECONDS`, default 3600, and it is
**capped below the approval window** (default 86400): a session must expire well inside the
window a plan can wait in, so that a plan pending overnight cannot be approved by a browser
nobody has re-authenticated in front of since. The session is in memory and is therefore lost on
a redeploy, exactly as the nonces already are; the README gains a line saying so, next to the
existing recovery notes.

*Pinned by tests 9, 10 and 11.*

### 5. Which listeners require OAuth

- `serve --stdio` keeps the local principal (`--principal`, default `local`) and gains nothing.
  The specification is explicit: implementations using a STDIO transport **SHOULD NOT** follow
  the authorization specification and should take credentials from the environment (research
  §1.2). Whoever runs it holds the machine that holds the provider credentials.
- `serve --http` requires OAuth **on every bind, loopback included**. There is no dual mode.

**The argument for losing the pre-shared hashes on loopback too.** A dual mode is a second
authentication path that must be attacked, configured and kept correct forever, and every
adversarial pass from here on would have to run its matrix twice. It is also the path that
fails open: a deployment that binds `0.0.0.0` while an operator believes it is on loopback would
silently keep accepting a static token. The local use cases the loopback listener served are
already covered — `serve --stdio` for a local agent, and the CLI for the operator's own
`plan`/`apply`/`approve` loop, which is the flow milestone 2 built for exactly this reason.
Against that, the design doc's own sentence for this milestone is "short-lived credentials, no
static bearer tokens", and a loopback exemption is precisely a static bearer token that survives.

**The cost, stated plainly, because it is a real one.** After 2c there is **no dev-only mode**:
driving the HTTP transport by hand needs a real token from a real provider. The fake authorization
server of decision 7 mints tokens in-process, but it is test-support code — it starts inside a
`cargo test` process and is not reachable from a hand-run `serve --http`. So a developer's loop is
one of exactly two things: write a test (which is where the fake is, and where every attack in
pass 3 lives), or point a locally run server at the operator's real provider and paste a
short-lived token. Anything else — a `--dev-token` flag, an "insecure mode", a fake server the
binary can start — is a second authentication path, which is the thing this decision exists to
refuse. The CLI needs none of this and stays the fast loop for everything that is not the transport
itself.

*Pinned by test 12.*

### 6. What happens to `hash-token` and the two hash variables

**Removed, and refused at startup on every bind** — not kept for loopback, not ignored.

`hash-token` is deleted from both binaries, along with its tests, and the README's section on it
goes with it. `WILLIKINS_AGENT_TOKEN_HASHES` and `WILLIKINS_APPROVER_TOKEN_HASH` become *retired*
variable names: if either is set, `serve --http` refuses to start with
`HttpConfigError::RetiredVariable { name }`, naming the variable and pointing at the OAuth
configuration that replaces it.

**Which error type the new refusals live on.** `StartupError`
(`crates/willikins-server/src/startup.rs:43`) has seven variants and every one of them is about
the **trusted workflow directory** or the `ServerStarted` write — `Directory`, `Symlink`,
`InvalidName`, `NameMismatch`, `Document`, `Check`, `Journal`. None is a configuration refusal, so
this milestone adds none to it. Configuration refusals go where the existing ones are:
`HttpConfigError` (`crates/willikins-server/src/http/config.rs:212`, today `NoAgentHash`,
`ApproverAmongAgentHashes`, `EmptyAllowedHosts`) gains **`RetiredVariable`**,
**`SymmetricAlgorithm`**, **`SessionOutlivesApprovalWindow`**, **`InsecureUrl`** and
**`EmptyAllowlist`**; a required variable that is simply unset keeps reusing
`ConfigError::Missing { variable }` (`crates/willikins-server/src/config.rs:33`), which is the
shape `build_http_config` already reaches for. One refusal cannot live on either: **`JwksUnavailable`**,
because `HttpConfigError` is `Copy` and is produced by the pure `HttpConfig::build`, which runs
*before* the tokio runtime exists (see the note in task 3), while a failed JWKS fetch happens
inside it. It goes on `cli.rs`'s own `StartError`, beside `Bind` and `NoBindOrPort`.

The reasoning for retiring loudly is the one `WILLIKINS_FAKE_CATALOG` already established in this
codebase: a variable that used to decide how the server authenticates must never quietly become a
no-op. An operator who upgrades the image and keeps the old variables set would otherwise have a
service that looks configured and is in fact open to anyone the provider will issue a token to,
which is the opposite of what those variables meant. Refusing is loud, happens before the listener
binds, and costs one deployment step in the go-live sequence (step 5 of decision 9: the variable
change lands *before* the deploy that needs it, never after).

**No commit on `main` may hold the no-op state this decision forbids**, which is a sequencing rule
on the tasks and not only on the shipped binary. So each half of the retirement lands in the same
task that removes the machinery it fed: task 3 removes `HttpConfig::build`'s agent-hash rules and
retires `WILLIKINS_AGENT_TOKEN_HASHES` in the same commit series that lands the OAuth middleware;
task 7 removes `basic_auth`, `TokenHash`, `matches_any` and the constant-time compare and retires
`WILLIKINS_APPROVER_TOKEN_HASH` in the same commit series that lands the login. Task 8 is then only
`hash-token` and the README. See the task table for why the split falls this way.

*Pinned by test 12, whose last case is that `hash-token` no longer exists on either binary.*

### 7. The fake authorization server, and adversarial pass 3

Every attack in this milestone needs a token with a chosen flaw, and no external provider can be
asked for one. So the test harness grows an **in-process fake authorization server**:

- An axum router on a loopback port with an OS-assigned port number, started per test, in the
  same process, on the same runtime — the shape `crates/willikins-server/tests/http_smoke.rs`
  already uses for the server itself. It also offers a **blocking `start()`**, which owns a runtime
  on its own thread and returns the base URL: `tests/binary_startup.rs` and its siblings spawn the
  real binary from a synchronous test, and a binary that fetches a JWKS at startup needs a fake
  that is already serving one from outside its own runtime.
- It holds a test key pair generated in the test and serves `/jwks.json` (with a `kid`), plus
  `/authorize` and `/token` endpoints good enough to drive the approvals login end to end —
  including a real PKCE `code_verifier` check (so test 10 proves willikins sends one), a
  `client_secret_basic` check (so it proves willikins authenticates), and a `resource`-parameter
  check on `/token` (so it proves willikins sends the indicator on both requests).
- **It serves no metadata documents.** An earlier draft had it serve an RFC 8414
  authorization-server metadata document, an OIDC discovery document, and a "lying metadata" mode
  whose `issuer` disagreed with the URL it was fetched from. willikins consumes none of the three:
  decision 13 is that it never fetches authorization-server metadata at all, so a fake that serves
  it is fixture surface with no consumer, and a lying-metadata attack has nothing to land on.
  What survives is the half that does have a consumer: the fake's `/authorize` can redirect back
  with a **wrong `iss` parameter**, which the callback's `iss` check refuses (decision 4). Test 18
  still fetches the *real* provider's discovery document, and only to print it.
- `mint(flaws)` returns a signed token with any combination of: wrong `aud`, wrong `iss`,
  `exp` in the past, `exp` inside the leeway, `alg: none`, a symmetric `alg`, an `alg` outside
  the allowlist, a `kid` that is not in the JWKS, no `kid`, a `typ` other than `at+jwt`, missing
  `aud`, missing `iss`, **missing `sub`**, a scope set carried as `scope` or as `scp`, no scope
  claim at all, an `aud` array containing this resource among others, a `sub` that is in neither
  allowlist, and a `sub` listed as an agent but not as an approver.
- It can be told to rotate its key (serving a new `kid`) and to hang a JWKS response for longer
  than willikins' JWKS timeout.
- It ships with an **environment-block helper**, because every test that configures a server
  against it sets the same dozen `WILLIKINS_OAUTH_*` variables and a hand-written block in twelve
  files is twelve places to forget `WILLIKINS_AGENT_SUBJECTS`.

This is what makes pass 3 deterministic rather than a story, and it is why task 2 comes before
task 3: the middleware is written test-first against it.

**The fake is plain HTTP on loopback, and the configuration rule has to permit that without
permitting anything else.** willikins' provider URLs — the issuer, the JWKS URI, the authorization
endpoint, the token endpoint — must be `https://`, or the token and the code travel in clear. But
an in-process fake on `127.0.0.1` with an OS-assigned port cannot hold a certificate, and giving
`JwkCache` and the token exchange a TLS trust hook so tests can inject one is a production code
path that exists only for tests. So the rule is: **every provider URL must be `https://` unless its
host is a loopback literal — `127.0.0.1`, `[::1]` or `localhost` — in which case `http://` is
accepted and startup logs a named warning saying which URL it is.** A non-loopback `http://` URL is
refused with `HttpConfigError::InsecureUrl`. This is permanent and is **not** gated on a test
build: a `cfg(test)` gate would mean the shipped binary runs a rule no test exercises, which is the
shape that rots. Both halves are pinned by test 12 — a loopback `http://` URL starts with a
warning, a public `http://` URL refuses — and `JwkCache` and the token exchange need no trust hook
at all.

**Pass 3's attacks**, each with the status and reason the test asserts:

| Attack | Expected |
| --- | --- |
| Wrong audience | 401, `error="invalid_token"`, `AuthFailedReason::InvalidAudience` |
| Wrong issuer | 401, `invalid_token`, `AuthFailedReason::InvalidIssuer` |
| Expired beyond the leeway | 401, `invalid_token`, `AuthFailedReason::TokenExpired` |
| `alg: none`, and a symmetric `alg` signed with a guessed secret | 401, `invalid_token`, `AuthFailedReason::InvalidAlgorithm`; the symmetric case also fails startup validation if it is ever configured |
| A key rotated out of the JWKS | 401, `invalid_token`, `AuthFailedReason::UnknownKey`, and at most one refetch |
| A token minted for another resource (valid signature, valid issuer, other `aud`) | 401, `invalid_token`, `InvalidAudience` — the token-passthrough MUST, from the receiving side |
| A token for a **previous** audience, with `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES` listing it and then not listing it | **200** while listed, 401 `InvalidAudience` the moment it is not; and the PRM document and every challenge name only the current identifier in both cases (decision 12's migration path) |
| A valid token whose `sub` is in neither allowlist, at `/mcp` | **403**, `AuthFailedReason::UnlistedSubject`, and no tool runs — asserted by the journal carrying no `ToolCalled` for it |
| A token with no `sub` claim | 401, `invalid_token`, `AuthFailedReason::MissingSubject` |
| An expired session cookie, and a session cookie replayed after `POST /approvals/logout` | **302 to `/approvals/login`** on a GET and **403** on a POST, in both cases, with the plan's nonce not burned |
| The passthrough case: a tool call whose provider request would carry the inbound token | the mock provider sees `Authorization` equal to the operator credential and nothing else; the handler sees no `Authorization`, `Cookie` or `Proxy-Authorization` header at all |
| An authorization response whose `iss` parameter is not the configured issuer | the callback refuses with 400. (willikins never fetches authorization-server metadata — decision 13 — so a lying metadata document has no consumer to attack; this is where the attack actually lands) |
| A JWKS endpoint that hangs | the request answers 401 `invalid_token` within `WILLIKINS_JWKS_TIMEOUT_SECONDS`, the blocking pool does not fill, `/healthz` keeps answering |
| A token endpoint that hangs, during a callback | the callback answers within `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS`, no session is created, no runtime worker is held |
| A token in the query string (`?access_token=...`) with no header | 401 with `MissingCredential`: OAuth 2.1 §5.1 is normative that resource servers MUST ignore an access token in a URI query parameter (research §2.6) |
| An `aud` array carrying this resource plus others | **accepted** — RFC 9068 says `aud` must *contain* a resource indicator for this server (research §2.5) |
| A bearer token presented at `/approvals`, and a session cookie presented at `/mcp` | at `/approvals`, **byte-identical** to the same request with no header at all (302 on a GET, 403 on a POST); at `/mcp`, 401 `MissingCredential`. Neither surface ever reads the other's credential |
| An **oversized body from an authenticated caller** at `/mcp` | **413** from the middleware, which buffers under the same `max_body_bytes` rmcp enforces (decision 3) |
| A `tools/call` with no `name`, and one naming a tool that does not exist | **403** `insufficient_scope` in both cases: an unknown name maps to no scope (decision 3) |
| A body `ClientJsonRpcMessage` cannot parse, from an authenticated caller | **400** from the middleware, before rmcp |
| A `notifications/initialized` from an authenticated caller | forwarded unchanged; no scope is required of it |
| A flood of garbage tokens at `/mcp` | journal growth stays bounded by the per-reason bucket (decision 1), the coalesced lines carry `suppressed`, and the approvals page's fold latency is **measured** and recorded — pass-2 hand-over item 6, pulled forward because a public listener makes the fold attacker-reachable |
| A flood of `GET /approvals/login`, and a flood of callbacks | the pending-login store stops at its cap with **503**; the session store likewise; neither grows without bound |
| A callback replayed a second time, and an authorize URL from one browser's login loaded in a second browser | **400** in both cases: `state` is single-use, and the `__Host-willikins-login` cookie does not match (decision 4) |
| `state` mismatch, `state` expired, and a `redirect_uri` the deployment did not register | 400 on the first two; the third is the authorization server's refusal, asserted against the fake |
| A valid token whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS`, presented after login | 403 on the approve POST, `AuthFailedReason::WrongRole`, the decision not journaled as a grant, and the 403 page showing that session's own `sub` and `iss`, escaped |
| A valid `willikins:read` token calling `apply` | 403, `error="insufficient_scope"`, `scope="willikins:apply"`, `resource_metadata` present |
| An authenticated request carrying a foreign `Host` | the status rmcp actually returns, measured rather than inferred — pass-2 hand-over item 10 |

Every bypass pass 3 finds becomes a fixture plus a test, as in passes 1 and 2, and the pass is
recorded under `docs/research/` with the same shape.

**One opt-in live login test, never in the workspace gate.** Interactive browser login cannot be
automated against a provider whose grant details this plan has not fetched, so the live test is
shaped like the existing live smoke test: behind a `live-tests` feature plus
`WILLIKINS_LIVE_TESTS=1`, `#[ignore]`d, reading an operator-obtained short-lived access token
from an environment variable. `willikins-server` has **no `[features]` table today**, so task 11
adds one — `live-tests = []` — plus the `[[test]]` entry with `required-features = ["live-tests"]`
that keeps the test out of `--all-targets`. It fetches the provider's real discovery document and
JWKS, starts a real server configured against the real issuer, calls `list_tools` with the real
token and asserts 200, then asserts the four refusals that need no minting: no token, a truncated
token, a token with a tampered signature, and the same token against a server started with a
**different `WILLIKINS_PUBLIC_URL`** — which is how the audience is varied now that it is derived
rather than configured (decision 12). The browser half of the login stays in "Verify with a
browser". *Test 18.*

### 8. The exposure work pass 2 handed over

Four items, as tasks rather than notes, from `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`,
"Handed to milestone 3 (or 2c, the OAuth milestone)", items 1, 2, 3 and 9. They are here because
each one is a consequence of the same decision — no public domain until authentication is
stronger — and that decision changes in this milestone. The other six items and where each one
went are in the out-of-scope list above.

- **Bound the header read** (its item 1, finding 8). `axum::serve` does not expose hyper's
  `header_read_timeout`, so this replaces `axum::serve` with a hand-written accept loop carrying
  its own graceful shutdown and connection accounting. Three implementation facts that the plan
  fixes now rather than leaving to discovery:
  - The builder is **`hyper::server::conn::http1::Builder`**, not
    `hyper_util::server::conn::auto::Builder`. The `auto` builder needs hyper-util's `server-auto`
    feature, which pulls HTTP/2 into the release binary — a protocol this service does not speak
    and a dependency surface it does not need. `hyper` becomes a direct dependency
    (`["server", "http1"]`; the lock already holds 1.11.1 through axum), and hyper-util supplies
    `rt::TokioIo`, `rt::TokioTimer` and `service::TowerToHyperService`.
  - hyper 1.11 **panics if `header_read_timeout` is set with no timer installed**, so the loop
    calls `.timer(hyper_util::rt::TokioTimer::new())` on the builder *before* setting the timeout.
    This is the single most likely way the task ships a binary that dies on its first connection.
  - Graceful shutdown is hand-rolled over the connection-accounting set the task introduces, so
    hyper-util's `server-graceful` feature is not enabled either; the existing `RUN_DRAIN_BOUND`
    keeps its meaning.

  The pass left an `#[ignore]`d slowloris test written so that it fails once the header read is
  bounded; this task flips it into the assertion. **It runs after adversarial pass 3, not before**,
  with its own short pass-3 addendum — see the task table. *Task 9a, test 13.*
- **Make the origin check IPv6-aware** (its item 2). The approvals Origin/Referer parser is not
  bracket-aware, which is pointless to fix until an origin can be a bracketed literal and
  necessary once one can. *Task 9b, test 14.*
- **The log level story, with a sweep of rmcp's own debug output** (its item 3, finding 10).
  Milestone 2's trust boundary 5 made INFO the ceiling because no level could be raised; this
  task gives the operator `WILLIKINS_LOG` (a `tracing_subscriber` filter directive, defaulting to
  the current INFO behaviour) and then sweeps what the whole process emits at DEBUG and TRACE —
  which the pass could not do, because the level could not be raised. The sweep is the
  acceptance test: at every level willikins can be configured to emit, no secret byte, no
  document text, no header value and no JSON-RPC body reaches stderr, or the level that does is
  refused. *Task 9c, test 15.*
- **Move the concurrency permit into the blocking closure** (its item 9). One line, no behaviour
  change; it stops the 64-call bound's correctness from depending on rmcp running the stateless
  handler on its own task. *Task 9d, test 16.*

### 9. The go-live sequence

Ordered, and the order matters: the resource identifier, the audience, the PRM document's
`resource` value and the registered `redirect_uri` all freeze the host the moment they are
published (research §2.2, §3.3), so the host is chosen before any of them is written anywhere.

1. **The host is `willikins.bandeabonnot.com`** (operator, 2026-09-16), a custom domain on a
   name the operator owns rather than a generated `*.up.railway.app` one, so the identifier the
   deployment publishes belongs to the operator and survives a move off Railway. Therefore:
   `WILLIKINS_PUBLIC_URL=https://willikins.bandeabonnot.com`, the resource identifier and audience
   are `https://willikins.bandeabonnot.com/mcp`, the PRM document is at
   `https://willikins.bandeabonnot.com/.well-known/oauth-protected-resource/mcp`, and the
   registered redirect URI is `https://willikins.bandeabonnot.com/approvals/callback`. A domain is
   not automatic on Railway (research §5.3): the operator adds it in the dashboard or with the
   CLI, and a custom domain needs **both** a CNAME and a TXT ownership record — with only the
   CNAME it answers 404 even after DNS resolves — after which Railway issues the certificate
   itself, giving up after 72 hours. These four values are what every later step and the
   provider's client registration are configured against, and they are the only values in this
   milestone that a change of host would move (decision 12 says how to move them).
2. **Declare it.** `.railway/railway.ts` can declare a custom domain (`domains: ["…"]`, or
   `{ domain, port }`) and **cannot** declare a generated one — that is documented, and it is the
   primary source behind the file's own header comment (research §5.4). So: a custom domain goes
   in the file; a generated one is created in the dashboard or by the CLI and the file's header
   comment is updated to say which. **Conditional in both directions on a verify item**: whether
   an omitted `domains` key deletes an already-attached *custom* domain is not stated anywhere —
   the documented "omit means delete" exemption covers only generated domains — so the file is
   never applied against a live custom domain until a read-only `railway config plan` has shown
   what the apply would do. Never settle this by applying.
3. **Measure what the edge sends, and what it already bounds.** Before any allowed-hosts value is
   frozen, deploy a build that echoes the inbound `Host`, `X-Forwarded-Host` and `X-Railway-Edge`
   on `/healthz` and read them from a real public request. What `Host` reads at the service is
   documented nowhere on `docs.railway.com` (research §5.2, §7.3) and it decides both
   `WILLIKINS_ALLOWED_HOSTS` and rmcp's `allowed_hosts`. In the same visit, **measure what the edge
   does to a trickled header stream**: open a connection, send headers one byte at a time, and
   record when and how it is closed. Railway's own documented limits are a "Max 32 KB combined
   header size" and "idle HTTP/1.1 connections are closed after 60 seconds between requests"
   (research §5.2), so the edge already bounds part of this shape and task 9a is **defence in depth
   behind it** rather than the only bound. The measurement is what turns that from an inference
   into a number, and it can only be taken against a live public deployment.
4. **Learn every `sub`, and set both allowlists.** Each approver and each agent operator logs in
   once and reads their own `sub` and `iss` off the `WrongRole` page (decision 4), which is the
   only place willikins surfaces them. `WILLIKINS_APPROVER_SUBJECTS` and `WILLIKINS_AGENT_SUBJECTS`
   are set from those values **before the domain goes public**. Both are required in http mode and
   an empty one refuses startup, so there is no window in which a reachable service has an unset
   allowlist.
5. **One variable change, before the deploy that needs it, with `.railway/railway.ts` in the same
   change.** In a single staged edit: set the whole OAuth block including both subject allowlists,
   give `WILLIKINS_ALLOWED_HOSTS` the measured public host alongside the private-domain reference,
   and delete `WILLIKINS_AGENT_TOKEN_HASHES` and `WILLIKINS_APPROVER_TOKEN_HASH`. Doing the
   deletion and the OAuth block as two changes around the deploy leaves a window where the service
   refuses to start — either `RetiredVariable` or `Missing`, depending on which half landed first —
   so they are one change. (`healthcheck.railway.app` needs no entry: `/healthz` is served outside
   the host check.)

   `.railway/railway.ts` must move with it. Its `env` block names five variables through
   `preserve()` today, two of which are `WILLIKINS_AGENT_TOKEN_HASHES` and
   `WILLIKINS_APPROVER_TOKEN_HASH`, and **the file's own header states that an omitted resource or
   field is an instruction to delete it, not to leave it alone**. So the file drops those two rows
   and gains a `preserve()` row for **every** new `serve --http` variable; without that, the next
   apply deletes the deployment's whole OAuth configuration and the service stops starting.
   **Require a read-only `railway config plan` showing no variable delete before any apply.** Two
   questions it must also settle, because neither is documented: what `preserve()` does for a
   variable that does not yet exist live, and whether a push-triggered deploy picks up variable
   edits staged on Railway's variables page, which stages edits until a deploy. The cutover order,
   and why:

   1. Stage the whole variable change in Railway.
   2. Edit `.railway/railway.ts`, then read `railway config plan`.
   3. Push the 2c commit, whose auto-deploy is what picks the staged variables up.

   If staged edits turn out **not** to be picked up by a push-triggered deploy, the fallback is
   that the previous healthy deployment stays active behind the healthcheck while the new one fails
   to start: one failed deployment, no outage, and then a deploy triggered by hand. Confirming that
   is a step of test 19, and both questions are in the verify list.
6. **Connect Doppler's Railway integration for the credentials** — now three secrets, not two:
   `WILLIKINS_GITHUB_TOKEN`, `WILLIKINS_DOPPLER_TOKEN` and the approvals login's
   `WILLIKINS_OAUTH_CLIENT_SECRET`. Doppler's integration is the only path a real credential
   takes onto the service, as it has been since milestone 2's task 12.
7. **Remove `WILLIKINS_FAKE_CATALOG`.** Until this step the deployed service serves the fake,
   in-memory catalog; after it, the real one.
8. **Record the tenancy.** The live service serves **one GitHub organization and one Doppler
   workplace** until milestone 3's credential routing lands. The operator runs several of each
   (milestone 2 plan, "Notes for milestone 3"), and nothing in 2c changes that: a public domain
   changes who may reach the server, not how many organizations its two credentials cover.

Steps 6 and 7 are milestone 2's own remaining operator steps. Read each as **"confirm it is
already done (a milestone 2 follow-up), or do it now"**: if the milestone 2 go-live finished them,
they are a check rather than work.

*Pinned by test 19, which is a checklist run by hand with the operator, not a cargo test.*

### 10. The identity provider is the operator's decision, and it is self-hosted

The plan is provider-agnostic. Any OIDC authorization server passes if it satisfies **three
must-haves**. The first is the operator's own constraint, stated on 2026-09-16; the other two are
the research note's (research §6):

1. **Self-hostable, under an open-source licence.** willikins is an AGPL-3.0-or-later tool that
   anyone may run, and a tool nobody can run without opening an account with a particular company
   is not one of those. Every deployment must be able to stand up its own authorization server
   from source, so a hosted tier is a convenience an operator may choose, never a dependency the
   software carries. This rules out Auth0 outright (SaaS only, no self-hosted edition) and rules
   out the hosted tier of every other candidate as a *requirement*, though not as a choice.
2. **Audience or resource-indicator binding.** The authorization server must be able to issue an
   access token whose audience is willikins' resource identifier. Honouring RFC 8707's `resource`
   parameter is the interoperable form, because that is the parameter a stock MCP client is
   required to send; a provider that binds the audience through a proprietary parameter instead
   still works for a client the operator configures, and does not work for one they do not.
3. **A registration path for MCP clients.** Client ID Metadata Documents, or an open Dynamic
   Client Registration endpoint, or pre-registration if the operator is content to register each
   client by hand before it can ever connect.

A fourth condition falls out of the out-of-scope list rather than the specification: **the
provider must issue JWT access tokens** for the registered resource, since introspection is out
of scope.

**The licence table**, from the research note's own section 6, because must-have 1 is now a filter
and not a footnote: Logto MPL-2.0 (self-host or cloud), Keycloak Apache-2.0 (self-host only),
authentik MIT with enterprise carve-outs (self-host), Zitadel AGPL-3.0-only with Apache-2.0 and MIT
carve-outs (self-host or cloud), Ory Hydra Apache-2.0 (self-host; no login UI, so the operator
writes the login and consent app), Auth0 proprietary SaaS. Only the last fails must-have 1. What
still eliminates the others is must-have 2: Keycloak "cannot recognize" the `resource` parameter,
Zitadel accepts and ignores it, authentik rejects it, and Ory Hydra has no user store to log in
against. That leaves Logto, self-hosted, which is what the Open decisions section recommends —
recommends, because must-have 1 is a property of the deployment and the operator may prefer to
carry the cost of one of the others.

**A willikins that needs no second service at all** is the honest end state for a self-hosted
open-source tool, and it is not this milestone: hosting an authorization server means exact
`redirect_uri` matching, refresh-token rotation, a client registry, RFC 7591 and RFC 8414
endpoints and a user store, which the out-of-scope list already prices as a milestone of its own.
It is recorded in "Notes for milestone 3" as the thing that would make a single-operator
deployment self-contained, and this milestone's job is to make the seam clean enough that it can
be filled later without changing the resource-server half at all.

Everything provider-specific lives in **one configuration block** — issuer URL, JWKS URI, the
approvals client's id and secret, the authorization and token endpoints, the algorithm allowlist
(the audience is no longer in it: decision 12 derives it) — and **one live test target** (test 18).
No provider name appears anywhere in `crates/`, in a fixture, or in an error message. Swapping
providers is an environment change, two rewritten subject allowlists (decision 3), and one live
test run.

**"Provider-agnostic" is about the code, not about the operator's shortlist.** The Open decisions
section below recommends self-hosted Logto as a deployment default. That is a recommendation for a human to
accept or reject, and accepting it changes zero lines in `crates/` and zero names in the
configuration shape — which is exactly the distinction this decision draws. A recommendation is not
a code dependency, and nothing in this milestone becomes harder if the operator picks another
provider that passes the three must-haves.

*Pinned by test 3, which runs the whole validation matrix against the fake authorization server
and therefore against no provider at all; by test 18 against the real one; and by test 19's
discovery step, which is the only place the goal's "a client that has never met this deployment"
sentence is actually exercised — a stock MCP client, with no pre-registration, completing
discovery, registration (CIMD or DCR), login and a `list_tools` call against the public domain.*

### Eight smaller decisions the research note left open

**11. The revision anchor is 2026-07-28's resource-server obligations.** willikins already
advertises `V_2026_07_28` in `get_info` while rmcp 3.3.0 negotiates `2025-11-25` (research §1.1);
targeting the later revision's server-side rules satisfies both, because all three deltas are a
tightening: the scope-hierarchy MUST, the `offline_access` SHOULD NOT, and a 403 `scope` that
names only what is needed (2025-11-25 merely recommended naming more). *Conditional*: the
2026-07-28 sub-pages `/authorization-server-discovery`, `/client-registration` and
`/security-considerations` were not fetched, and the index says the last covers "mix-up" attacks,
a term absent from the page that was fetched — so a further resource-server obligation may exist.
**Task 4's first step** is to fetch all three, plus the `modelcontextprotocol/ext-auth` extensions,
and to record any new resource-server obligation in this plan before the metadata and challenge
surface are frozen. Naming the owner here is the point: an unowned "fetch this first" is a verify
item nobody runs.

**12. The resource identifier is `<public base URL>/mcp`, not the bare origin.** Both are legal
and picking one freezes the other (research §2.2). `/mcp` is picked because RFC 9728 §3.3 says
that when the client reached the document through the `resource_metadata` challenge, the
returned `resource` must be identical to the URL the client used to request the resource — and
that URL is `/mcp`. The origin also hosts `/approvals` and `/healthz`, which are not the
protected resource. The PRM document is therefore served at
`/.well-known/oauth-protected-resource/mcp`, built by **inserting** the well-known string between
host and path, never by appending it to the path — the research note calls this the single most
likely implementation mistake in the milestone.

**It is derived from `WILLIKINS_PUBLIC_URL` and is not separately configurable.** An earlier draft
made the audience its own optional variable defaulting to `<public URL>/mcp`, and the redirect URI
another defaulting to `<public URL>/approvals/callback`. Both are removed. The audience is *always*
`<WILLIKINS_PUBLIC_URL>/mcp` and the redirect URI is *always*
`<WILLIKINS_PUBLIC_URL>/approvals/callback`. The reason is test 1's own rule: RFC 9728 §3.3
requires the PRM document's `resource` to be byte-identical to the URL the client used, and two
values that can be set independently are two values that can disagree — an overridable audience is
a misconfiguration that produces a server whose published metadata does not describe the resource
it validates for. Deriving both makes that rule hold **by construction** rather than by a startup
comparison nobody wrote. Startup refuses a `WILLIKINS_PUBLIC_URL` that carries a path, a query or a
fragment, or that is not `https://` (decision 7's loopback carve-out excepted), because each of
those makes the derivation ambiguous or the identifier insecure.

**Stable is not permanent, and the plan says how to move.** An earlier draft of this section, and
of the Risks entry below, said the audience is frozen "forever after". That is wrong as a design
statement: RFC 9728 §3.3 constrains what a *published* document may say at a given moment, not how
long a deployment must keep the same hostname, and a tool whose identifier can never change is a
tool that can never move off a host. So there is a migration path, and it is one optional variable:
`WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`, a comma-separated list of resource identifiers this server
**also accepts** in `aud` during a move. It is never published — the PRM document's `resource` is
always the one derived value, so test 1's byte-identical rule still holds by construction — and it
is never what a challenge names. While it is non-empty, startup logs a named warning saying which
identifiers are being accepted beyond the current one, so a migration that was left half-finished is
visible in the first log line rather than a year later. The documented move is therefore: publish
the new host, set the previous identifier here, let clients re-discover through the 401 challenge
they already follow, then remove it. Pass 3 attacks it: a token for a previous audience is accepted
only while the variable lists it, and rejected the moment it does not.

**13. Introspection, opaque tokens and AS-metadata discovery are all absent.** See the
out-of-scope list for the first two. For the third: willikins does not fetch the authorization
server's metadata document at all. The issuer and the JWKS URI are two configuration values the
operator already has, providers disagree on whether RFC 8414 is even served (Zitadel publishes
OIDC discovery and no RFC 8414 endpoint — research §6), and a startup-time fetch of a document
that is only used to learn two strings is a dependency with no gain. The consequence is stated
in pass 3's table: "a metadata document that lies about the issuer" cannot attack the resource
server, only the approvals login's callback, where the `iss` check catches it.

**14. The accepted `typ` set is frozen to `at+jwt` and `application/at+jwt`, with no knob.** RFC
9068 §4 requires rejecting any other value (research §2.5). An earlier draft made this
configuration — `WILLIKINS_OAUTH_ACCEPTED_TYP`, widenable by the operator with a startup warning —
on the argument that no fetched sentence says any of the six surveyed providers actually sets `typ`
to `at+jwt`, so freezing it would freeze an assumption about a provider not yet chosen. That
argument is backwards: the variable does not resolve the uncertainty, it *ships* it, as a knob whose
only use is to relax an RFC MUST at 3 a.m. against a provider nobody has tested. The unknown is
handled where unknowns belong — **"the provider emits `at+jwt`" is one of decision 10's provider
preconditions, checked by test 18**, which prints the whole JOSE header. A provider that does not
emit it is a decision to take then, with the real value in hand, not a configuration surface to
carry until then.

**15. The algorithm allowlist is configuration with no default, may hold only asymmetric families,
and is intersected per key.** `WILLIKINS_OAUTH_ALGORITHMS` is required in http mode. Startup
refuses a symmetric family (`HS*`) with `HttpConfigError::SymmetricAlgorithm`: a resource server
verifying an HMAC would have to hold the signing secret, which would make it able to mint its own
tokens and break trust boundary 1. No default algorithm is written into the plan because the
research note fetched no statement of any provider's signing algorithm.

**The allowlist is applied per key, never per token header.** For each JWKS key, the `Validation`
the verification runs under carries **the allowlist intersected with that key's own family** —
so an RSA key is only ever asked to verify an `RS*`/`PS*` algorithm and an EC key only an `ES*` one.
The token's `alg` header selects nothing: it is compared against that intersection and refused if it
is outside. This matters the moment the allowlist spans two families, which a real deployment's
does during a migration. `jsonwebtoken` already refuses a verifier whose key family differs from an
allowed algorithm's family (research §4.2), which means a single `Validation` carrying a mixed
allowlist fails *every* verification rather than choosing correctly — so the intersection is what
makes a two-family allowlist work at all, and a two-family case is one of test 3's rows.

**16. The JWKS is fetched at startup, refreshed in the background, and refetched on an unknown
`kid` at most once per window.** Startup fetches it and **refuses to start** if the fetch fails
(`StartError::JwksUnavailable`, on `cli.rs`'s own error type beside `Bind` — see decision 6 for why
it cannot live on `HttpConfigError`), which fails closed the way every other startup refusal does.
The fetch is `ureq` inside `tokio::task::spawn_blocking` — the tree has no async TLS
client at all and `ureq` 3.4.2 is the only HTTP client in it (research, "Two corrections") —
with its own timeout (`WILLIKINS_JWKS_TIMEOUT_SECONDS`, default 5, far inside the 30 s request
timeout). A token whose `kid` is not in the cache triggers at most one refetch per
`WILLIKINS_JWKS_MIN_REFETCH_SECONDS` (default 60), so a stream of forged `kid`s cannot make
willikins hammer the provider; a token whose header carries no `kid` never matches, because
`JwkSet::find` only matches keys that have one (research §4.2) — which is a refusal, so verify
item 6 covers it too: if the chosen provider ever signs without a `kid`, every token is refused
and this decision needs a single-key fallback. Test 18 prints the whole JOSE header, so the
question is answered by running it. A token that cannot be verified
for any of these reasons is 401 `invalid_token`: the specification's status table has no code for
"the authorization server is unreachable", and a token willikins cannot verify is not a token it
may honour.

**A failed refresh keeps the previous key set; keys are never dropped on failure.** The background
refresh replaces the cached `JwkSet` only on a *successful* fetch. A failure logs a warning at
every failed interval — every interval, not once, so a provider that has been unreachable for six
hours says so six times rather than once — and the previous keys keep validating. The alternative,
emptying the cache on a failed refresh, turns one unreachable provider into a total outage of a
service whose keys are still perfectly good. The accepted cost is stated rather than discovered: a
key the provider **removed** stays trusted for at most one refresh interval past the first
successful fetch that no longer carries it (`WILLIKINS_JWKS_REFRESH_SECONDS`, default 3600), and
while the provider is unreachable, indefinitely. That is the staleness bound this decision accepts;
a deployment that needs a tighter one lowers the interval.

**17. Clock skew is bounded at `WILLIKINS_OAUTH_LEEWAY_SECONDS`, default 60.** That is
`jsonwebtoken`'s own default `leeway` (research §4.2), and it is made explicit rather than
inherited so that a future version changing its default cannot change willikins' behaviour
silently. `validate_nbf` is turned on, `required_spec_claims` is set to `exp`, `aud`, `iss`, `sub`.

**18. The rmcp 3.4.0 bump is its own task and is conditional.** 3.4.0 does not move any
resource-server work into rmcp (research §3.5), so nothing here needs it. **`allowed_origins`
itself is not the reason to bump**: it already exists at 3.3.0, as a
`StreamableHttpServerConfig` field with a `with_allowed_origins` builder
(`rmcp-3.3.0/src/transport/streamable_http_server/tower.rs:119` and `:212`), so **task 4 sets it
unconditionally** at the version the lock holds. What 3.4.0 adds is only
`enforce_origin_validation()`: at 3.3.0 an **empty** allowlist skips origin validation entirely
(`tower.rs:884`), and that method makes an empty list enforce rather than exempt. Since task 4 sets
a non-empty list, the delta is a defence against a future empty one, which is worth having and is
not worth blocking on. It is not a lock bump on its own: `ServerInfo` is deprecated at 3.4.0 and
used at three sites in `mcp.rs` under `-D warnings`. *Conditional*: no cargo command was permitted
during the research, so whether `cargo update -p rmcp --precise 3.4.0` resolves and the four gates
pass is unverified, as is the effect of 3.4.0's cancellation and pre-init changes on willikins'
stateless configuration.

**If task 1b is skipped, nothing changes.** No task and no acceptance test depends on it: the
origin value is set at 3.3.0, the scope check's placement is settled at 3.3.0 (decision 3), and the
milestone ships on the lock as it stands. Recording why it was skipped is the whole of the
obligation.

## Pinned dependencies (new)

Caret requirements on the major; `Cargo.lock` pins the rest. Every version, MSRV and licence cell
below is quoted in the research note from a source fetched on 2026-09-16, and every one was
measured against the workspace's **declared** MSRV of 1.88 (`Cargo.toml:21`), not against the
1.97 toolchain the Dockerfile builds with.

| Crate | Requirement | Why |
| --- | --- | --- |
| `jsonwebtoken` | `{ version = "11", default-features = false, features = ["<backend>"] }` (11.0.0, MIT, MSRV 1.88, edition 2024) | validates the access token; parses the JWKS and selects by `kid` natively. Its defaults are not enough on their own — see decisions 15 and 17 |
| `axum-extra` | `{ version = "0.12", default-features = false, features = ["cookie-signed"] }` (0.12.6, MIT) | the approvals session cookie. Requires `axum ^0.8.9` and `axum-core ^0.5.2`; the lock holds 0.8.9 and 0.5.6, so no axum bump |
| `hyper` | `{ version = "1", default-features = false, features = ["server", "http1"] }` (1.11.1 is already in the lock, pulled by axum) | task 9a's accept loop builds on `hyper::server::conn::http1::Builder` directly; it becomes a **direct** dependency of `willikins-server` rather than a transitive one |
| `hyper-util` | `{ version = "0.1", default-features = false, features = ["server", "http1", "tokio", "service"] }` (0.1.20 is already in the lock) | `rt::TokioIo`, `rt::TokioTimer` (decision 8: hyper 1.11 panics without a timer) and `service::TowerToHyperService`. **Not `server-auto`**, which would add HTTP/2 to a release binary that speaks only HTTP/1.1 |
| `tracing-subscriber` | already a workspace dependency at `0.3`; task 9c **adds the `env-filter` feature** to the existing `features = ["json"]` | `WILLIKINS_LOG` is a filter directive, and `EnvFilter` is behind that feature |
| `ureq` | already a workspace dependency at `3`; for `willikins-server` it **moves from `[dev-dependencies]` to `[dependencies]`** | the JWKS fetch is production code now, not test support; today `willikins-server` lists `ureq` only under `[dev-dependencies]` |
| JWKS fetch and cache | no crate — hand-rolled over the tree's `ureq` 3.4.2 inside `spawn_blocking`, parsed into `jsonwebtoken::jwk::JwkSet` | `jsonwebtoken` has no HTTP client, and the tree has no async TLS client at all |

Rejected, each for a fetched reason (research §4.1, §4.5): `reqwest` (a second async HTTP+TLS
stack in an image that has none), `oauth2` 5.0.0 (every bundled client mismatches this tree),
`openidconnect` 4.0.1 (23 non-optional dependencies duplicating four majors, and it pulls `rsa`),
`josekit` (non-optional `openssl`), `jwt-simple` (BoringSSL by default, audience unchecked by
default), `jwks-client` (one release, 2020), `tower-sessions` (needs a session store this server
does not have; its cookie defaults are copied instead).

`oauth2`'s rejection no longer rests on a version-unification claim. An earlier draft said its
`base64 >=0.21, <0.23` bound "cannot unify with the tree's 0.23.1"; that is not how cargo works —
two semver-incompatible majors coexist in one build, which this tree already demonstrates
(`Cargo.lock` holds `rand` 0.9.5 **and** 0.10.2, and three majors of `getrandom`). The reason to
reject it is the one that survives: every bundled HTTP client mismatches this tree. What duplicate
majors actually cost is build time and audit surface, and that is recorded under Risks.

**The one open dependency question is a Dockerfile question.** Since 10.0.0 `jsonwebtoken`
requires exactly one crypto backend. `rust_crypto` is pure Rust and builds in the current image
unchanged but pulls `rsa` 0.9, which carries RUSTSEC-2023-0071 with `patched = []` — a private-key
timing leak, which does not apply to a resource server that only verifies with public keys, but
which any future `cargo-audit` or `cargo-deny` gate would flag forever. The open question about
`aws_lc_rs` is **not** "the image has no C toolchain": the builder stage already installs `gcc` and
`libc6-dev`, for `ring`, and the Dockerfile says so in its own comment. What is open is whether
`aws-lc-sys` needs **`cmake` and `bindgen`** on this target, which it does wherever it ships no
prebuilt bindings, and whether adopting it would falsify the Dockerfile's own "no `aws-lc-sys`,
`openssl-sys`, or `cmake` anywhere in the tree" comment. **No cargo command was permitted during
the research, so neither backend was test-built.** Task 1 builds both in the image and picks;
whichever wins, the choice is recorded in the Dockerfile with its reason, and `rust_crypto`
additionally gets a documented ignore entry ready for the day an audit gate exists.

## Crate contracts

### willikins-server (the bulk of the milestone)

**New module `src/http/oauth/`**, replacing the body of `http::auth::bearer_auth` at the seam
that already exists. The order stays: timeout → tracing → the auth middleware → rmcp's
`StreamableHttpService` → rmcp's host check → dispatch. rmcp offers no server-side authorization
hook in 3.3.0 or 3.4.0 and needs none; `StreamableHttpServerConfig` has ten fields and not one of
them is an authorization point (research §3.3).

- `oauth::config::OAuthConfig` — the single provider-agnostic configuration block of decision 10,
  built by `ServerConfig::from_vars` and validated at startup.
- `oauth::jwks::JwkCache` — `Arc`-shared, `ureq` in `spawn_blocking`, startup fetch, background
  refresh, rate-limited refetch on an unknown `kid` (decision 16).
- `oauth::validate::validate(token, &OAuthConfig, &JwkCache) -> Result<Claims, TokenRejection>` —
  checks 0 to 7 of decision 1, in order, each mapping to one `TokenRejection` variant and one
  `AuthFailedReason`. The function is pure over its inputs plus the cache, so the whole validation
  matrix is a unit test as well as an end-to-end one.
- `oauth::middleware::require_token` — extracts the header (and nothing else: a query parameter
  is not read), validates, checks the `sub` against `WILLIKINS_AGENT_SUBJECTS`, derives the
  principal (decision 2), inserts the `Principal` (id plus claims) and the granted scope set into
  the request extensions, **removes the `Authorization`, `Cookie` and `Proxy-Authorization`
  headers**, and calls `next.run`. Only then does it buffer and parse the body for the scope check
  (decision 3). On refusal it journals through `Butler::record_auth_failure` and answers the
  challenge of decision 1.
- `oauth::scope` — the four scopes, the hierarchy, the `scope`/`scp` claim readers, and the
  tool-to-scope table of decision 3. The check runs in the middleware, on the parsed
  `ClientJsonRpcMessage`, for the reason decision 3 gives; the granted set is also carried in the
  request extensions so a handler can assert it.
- `oauth::metadata` — the RFC 9728 document and its route, registered on the root router outside
  both the auth middleware and rmcp's nest, exactly as `/healthz` is.
- `http::approvals` gains the login: `GET /approvals/login`, `GET /approvals/callback`,
  `POST /approvals/logout`, an in-memory `SessionStore` and `PendingLoginStore` (both capped and
  TTL-pruned per decision 4), and the `SignedCookieJar` of decision 4. The nonce store, the
  Origin/Referer check and the body cap are untouched apart from task 9b.
- `http::auth` loses `bearer_auth`'s hash comparison and `basic_auth` entirely. `TokenHash`,
  `matches_any` and the constant-time compare go with them; nothing else in the crate uses them.
  They leave in two pieces, in the tasks that replace what each half fed — see decision 6.
- `http::config` gains the `HttpConfigError` variants of decisions 6, 7 and 15, and `cli.rs`'s
  `StartError` gains `JwksUnavailable` (decision 16). `startup::StartupError` is **not** touched:
  its seven variants are the trusted-directory refusals and none of this milestone's refusals is
  one.

**`mcp.rs`.** `principal_for` reads the `Principal` the OAuth middleware inserted — id plus claims,
not a bare `PrincipalId` — and every `#[tool]` method passes it on to the `Butler` method it calls,
which is the signature change decision 2 describes. It does **not** gain a scope refusal, because
an authorization refusal is a transport-level status in the specification's table rather than a
domain error, and a tool result cannot carry one (decision 3).

### willikins-journal

The new `AuthFailedReason` variants land in **task 3**, with the middleware that produces them,
because tests 3 to 5 assert them. The claim fields land in task 5. Both are additive.

Three new **optional** fields — `subject`, `issuer`, `client_id` — on every event that carries a
`principal`, each bounded and escaped before it is written (decision 2), plus two more optional
fields on `AuthFailed` alone: `subject`, written only for the refusal of a token that validated,
and `suppressed`, written only on a coalesced line (decision 1). No existing field changes type,
**no variant is removed** — `InvalidCredential` and `MalformedUsername` stay in the enum and stop
being produced, because `pre-pass-2-every-event.jsonl` carries both and removing a variant would
break the replay the whole discipline exists to guarantee — and no field becomes required. The new
`AuthFailedReason` variants are the ones in decision 1's check table.

Three fixtures replay unchanged through `Journal` and through `willikins_journal::replay`: both
existing ones and the new `pre-2c-every-event.jsonl`. `post-pass-2-new-shapes.jsonl`, which pass 2
froze as "the next pass's baseline", is that baseline; `pre-2c-every-event.jsonl` supplements it
with the events this milestone touches, cut from the current binary before task 3 changes anything.

**How the new fixture is produced.** Both existing fixtures came from an `#[ignore]`d generator
test in `willikins-journal` — `regenerate_the_frozen_fixture`, one in
`crates/willikins-journal/tests/pre_pass_2_replay.rs:291` and one in
`crates/willikins-journal/tests/post_pass_2_shapes.rs:217`, each writing to
`$WILLIKINS_FIXTURE_OUT` and each kept in the tree as the fixture's provenance rather than as a way
to refresh it. `pre-2c-every-event.jsonl` is generated the same way, by the same shape of test, and
carries the same "never point this at the committed file" warning.

### willikins-cli

`hash-token` is removed. `serve --http` reaches the same configuration path as the binary, so it
inherits every new refusal. Its commands in `crates/willikins-cli/src/commands.rs` pass
`Principal { id, claims: None }` where they pass a bare `PrincipalId` today — the signature change
of decision 2 reaches them even though nothing about the CLI's own authentication changes. The
CLI's own flow (`plan`, `apply`, `approve`, `reject`, `runs`,
`run` against a journal file) is unauthenticated and unchanged: it is the operator's local flow,
on the machine that holds the credentials, and decision 5's argument for removing loopback bearer
tokens rests on it staying that way.

### Dockerfile and deployment

One new build input (the `jsonwebtoken` backend of the dependency section) and one new secret
(`WILLIKINS_OAUTH_CLIENT_SECRET`) through Doppler's Railway integration. `.railway/railway.ts`
changes twice, both in task 12: it gains a `domains` entry on the custom-domain branch of decision
9, step 2, and its `env` block drops the two retired `preserve()` rows and gains one per new
`serve --http` variable at step 5. Both only after a read-only `railway config plan` that shows no
delete, because the file's own rule is that an omitted field is an instruction to delete.

## Environment variables

**Eighteen added**, ten of them required in http mode and eight optional. Every tunable names the component that
reads it, so a value with no consumer cannot survive review. Two values that look like they should
be here are not, and deliberately: the **audience** and the **redirect URI** are derived from
`WILLIKINS_PUBLIC_URL` and are not configurable at all (decision 12), and the accepted `typ` set is
frozen in code (decision 14).

| Variable | Default | Required | Value and who reads it |
| --- | --- | --- | --- |
| `WILLIKINS_PUBLIC_URL` | none | `serve --http` | The canonical `https://` origin the service is reached at; no path, no query, no fragment, refused otherwise. The audience (`<it>/mcp`) and the redirect URI (`<it>/approvals/callback`) are derived from it. Read by `oauth::config`, `oauth::metadata` and the approvals login. |
| `WILLIKINS_OAUTH_ISSUER` | none | `serve --http` | The authorization server's issuer identifier, compared to `iss` by exact string match. Read by `oauth::validate` and by the callback's `iss` check. |
| `WILLIKINS_OAUTH_JWKS_URI` | none | `serve --http` | An `https://` URL (loopback `http://` excepted, decision 7). Not discovered; see decision 13. Read by `oauth::jwks::JwkCache`. |
| `WILLIKINS_OAUTH_ALGORITHMS` | none | `serve --http` | Comma-separated JWA names. Asymmetric families only; intersected per key (decision 15). Read by `oauth::validate`. |
| `WILLIKINS_OAUTH_LEEWAY_SECONDS` | `60` | optional | Whole seconds of clock skew. Read by `oauth::validate`. |
| `WILLIKINS_JWKS_TIMEOUT_SECONDS` | `5` | optional | Whole seconds; well under the 30 s request timeout. Read by `JwkCache`'s fetch. |
| `WILLIKINS_JWKS_REFRESH_SECONDS` | `3600` | optional | Background refresh interval, and therefore the accepted staleness bound of decision 16. Read by `JwkCache`'s refresh task. |
| `WILLIKINS_JWKS_MIN_REFETCH_SECONDS` | `60` | optional | Floor between unknown-`kid` refetches. Read by `JwkCache`. |
| `WILLIKINS_AGENT_SUBJECTS` | none | `serve --http` | Comma-separated `sub` values allowed to hold any MCP scope. Empty or unset refuses startup (decision 3). Read by `oauth::middleware::require_token`. |
| `WILLIKINS_APPROVER_SUBJECTS` | none | `serve --http` | Comma-separated `sub` values allowed to approve. Empty or unset refuses startup. Read by the approvals decision handler. |
| `WILLIKINS_OAUTH_CLIENT_ID` | none | `serve --http` | The approvals page's own OAuth client id. Read by the login and the token exchange. |
| `WILLIKINS_OAUTH_CLIENT_SECRET` | none | `serve --http` | Confidential-client secret; a `Credential`, never logged, Doppler only. Read by the token exchange, through `Credential::authorize_basic` (decision 4). |
| `WILLIKINS_OAUTH_AUTHORIZE_URL` | none | `serve --http` | The provider's authorization endpoint. Read by `GET /approvals/login`. |
| `WILLIKINS_OAUTH_TOKEN_URL` | none | `serve --http` | The provider's token endpoint. Read by `GET /approvals/callback`. |
| `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS` | `5` | optional | Whole seconds bounding the callback's token exchange, so a hung token endpoint cannot hold a request or a worker. Read by the token exchange. |
| `WILLIKINS_SESSION_TTL_SECONDS` | `3600` | optional | Capped below `WILLIKINS_APPROVAL_WINDOW_SECONDS`; startup refuses a larger value. Read by the session store. |
| `WILLIKINS_LOG` | the current INFO behaviour | optional | A `tracing_subscriber` filter directive (task 9c). Read once, when the subscriber is built. |
| `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES` | empty | optional | Comma-separated resource identifiers this server also accepts in `aud` during a move to a new host (decision 12). Never published, never named in a challenge; a non-empty value logs a named startup warning. Read by `oauth::validate`. |

Retired, and refused at startup: `WILLIKINS_AGENT_TOKEN_HASHES`, `WILLIKINS_APPROVER_TOKEN_HASH`.

Unchanged: `WILLIKINS_WORKFLOWS_DIR`, `WILLIKINS_JOURNAL_PATH`, `WILLIKINS_ALLOWED_HOSTS` (which
gains the measured public host at go-live), `WILLIKINS_PLAN_TTL_SECONDS`,
`WILLIKINS_APPROVAL_WINDOW_SECONDS`, `WILLIKINS_PLAN_RATE_PER_MINUTE`,
`WILLIKINS_READ_RATE_PER_MINUTE`, `WILLIKINS_GITHUB_TOKEN`, `WILLIKINS_DOPPLER_TOKEN`,
`WILLIKINS_FAKE_CATALOG`, `PORT`. The README's variable reference is updated in the same task as
the code that reads each one.

## Acceptance tests

The milestone cannot ship without every one of these. Each names the exact status or error kind
it expects.

1. **The protected-resource-metadata document.** `GET /.well-known/oauth-protected-resource/mcp`
   with no credential answers **200** and `content-type: application/json`; `resource` is
   byte-identical to the derived audience `<WILLIKINS_PUBLIC_URL>/mcp` — which holds by
   construction now that the audience is not separately configurable (decision 12);
   `authorization_servers` has at least one entry; `scopes_supported` is exactly the four of
   decision 3 and never contains `offline_access`; `bearer_methods_supported` is `["header"]`; no
   parameter with zero values is present. The route answers with an unknown `Host` (it is outside
   rmcp's host check) and a bearer token is not required and not read. A companion test asserts the
   URL is built by insertion: a public URL of `https://h` publishes at
   `/.well-known/oauth-protected-resource/mcp`, and `GET /mcp/.well-known/oauth-protected-resource`
   answers **401**. Not 404 and never 200: that path sits *inside* the `/mcp` nest, so it meets the
   bearer middleware first, and the assertion is that it is refused as an unauthenticated MCP
   request rather than serving the document at the wrong URL.
2. **The 401 challenge.** No `Authorization` header → **401**, `WWW-Authenticate` containing
   `Bearer`, `resource_metadata="<the exact URL from test 1>"` and the fixed
   `scope="willikins:read"` — fixed because the token is validated before the body is read at all
   (decision 3), so an unauthenticated challenge cannot know what the call would have needed —
   journaled `AuthFailedReason::MissingCredential`. A header that is not a `Bearer` credential
   (`Basic …`, say) → **401** `MissingCredential`, which is what ships today and what
   `crates/willikins-server/tests/adversarial_10b.rs` pins; whether RFC 6750 §3.1 calls that
   malformed, and therefore **400**, is verify item 13, and the task that ever flips it names
   every pinned test it flips. A well-formed but rejected token → **401** with the same header
   plus `error="invalid_token"`. `?access_token=` in the query string with no header → **401**
   `MissingCredential` (the parameter is never read). An **authenticated** request with a body over
   `max_body_bytes` → **413** from the middleware; an authenticated request whose body
   `ClientJsonRpcMessage` cannot parse → **400** from the middleware, before rmcp; an authenticated
   `GET` and `DELETE` at `/mcp` → **405** from rmcp with `Allow: POST`.
3. **The validation matrix**, against the fake authorization server, one case per check of
   decision 1: not a parseable JWS, wrong `aud`, missing `aud`, wrong `iss`, missing `iss`, missing
   `sub`, `exp` in the past, `alg: none`, a symmetric `alg`, an `alg` outside the allowlist, a
   `typ` outside the frozen set, a `kid` absent from the JWKS, a header with no `kid`, a signature
   from a foreign key. Every one is **401** `error="invalid_token"` with the matching
   `AuthFailedReason` from decision 1's table; a token whose `aud` is an array containing this
   resource among others is **200**. One further case for decision 15: with an allowlist spanning
   **two key families** (an `RS*` and an `ES*` entry) and a JWKS holding one key of each, a token
   signed by either verifies, and a token whose `alg` names the other key's family is refused —
   which is what proves the per-key intersection rather than a single mixed `Validation`. Each case
   also runs as a unit test against `oauth::validate` so the reason is asserted, not just the
   status.
4. **Clock skew.** A token expired by less than `WILLIKINS_OAUTH_LEEWAY_SECONDS` is accepted; one
   expired by more is **401** `AuthFailedReason::TokenExpired`. A token whose `nbf` is in the
   future is **401** — `jsonwebtoken`'s `validate_nbf` defaults to false, so this test is what
   proves the override is in place.
5. **JWKS cache, rollover and hang.** After the fake rotates its key, the first token signed with
   the new `kid` triggers exactly one refetch and is accepted; a second unknown `kid` inside
   `WILLIKINS_JWKS_MIN_REFETCH_SECONDS` triggers no refetch and is **401**
   `AuthFailedReason::UnknownKey`. With the JWKS endpoint hanging, a request with an unknown
   `kid` answers **401** within `WILLIKINS_JWKS_TIMEOUT_SECONDS`, `/healthz` still answers **200**
   throughout, and the blocking pool is not exhausted. A background refresh that **fails** leaves
   the previous key set validating and logs a warning at every failed interval (decision 16).
   Startup with an unreachable JWKS refuses with `StartError::JwksUnavailable`.
6. **The inbound token neither leaves nor lands.** A probe that reads the headers rmcp hands a
   handler sees **no** `Authorization`, `Cookie` or `Proxy-Authorization` header. The probe is an
   **axum layer the test wraps around `router()`'s output**, not a test-only MCP tool and not a
   crate feature: an integration test under `tests/` cannot see a `#[cfg(test)]` library item, so a
   "test-only tool" would have to be compiled into the shipped binary or hidden behind a feature
   that nothing else needs. A layer sees exactly what rmcp would receive and costs the production
   build nothing. A tool call that makes a provider request against the mock HTTP server asserts
   the outgoing `Authorization` equals the operator credential and contains no substring of the
   inbound token. The `expose_secret` guard
   (`crates/willikins-core/tests/expose_secret_guard.rs`) and the `SinkToken::new` guard are
   extended to cover the new module and the new `Credential::authorize_basic` exemption, and a
   sweep over the journal, every log line and every error body produced during a full
   plan-approve-apply cycle asserts the token's bytes appear nowhere.
7. **Subjects, scopes and the 403.** A valid token whose `sub` is **not in
   `WILLIKINS_AGENT_SUBJECTS`** → **403** `AuthFailedReason::UnlistedSubject` on every tool
   including the read ones, with the journal carrying no `ToolCalled` for it — the allowlist is
   checked before anything runs. Within the allowlist: a `willikins:read` token calling `apply` →
   **403** with `error="insufficient_scope"`, `scope="willikins:apply"` and `resource_metadata`
   present; calling `plan` → **403** `scope="willikins:plan"`. A `willikins:apply` token calling
   `describe` → **200** (the hierarchy). A token with no scope claim at all → **403** on every
   tool, and the same token with its scopes in `scp` rather than `scope` behaves identically.
   `willikins:approve` alone → **403** on every MCP tool. A `tools/call` with no `name`, and one
   naming a tool that does not exist → **403**. An `initialize`, a `tools/list`, a `ping` and a
   notification from a listed subject → forwarded, no scope required.
8. **Principal and journal.** The same `(iss, sub)` always derives the same
   `oauth-<12 hex>` principal and two different subjects never collide; the principal parses
   under `PrincipalId`'s grammar for a `sub` containing `|`, `:` and non-ASCII characters. Every
   event carrying a principal also carries `subject`, `issuer` and `client_id` when present, each
   bounded and escaped; a token with no `client_id` claim journals without it and does not fail.
   `pre-2c-every-event.jsonl` and both pass-2 fixtures replay through `Journal` and through
   `replay` unchanged.
9. **Transport separation, asserted as identity of response.** `GET /approvals` with a valid
    access token in `Authorization` produces a response **byte-identical** to the same request with
    no header at all: **302** to `/approvals/login`, no session created, no `WWW-Authenticate`. The
    same for a `Basic` header, and the same for `POST /approvals/{plan_id}`, which is **403** in
    all three cases. A valid session cookie presented at `/mcp` with no `Authorization` header →
    **401** `MissingCredential`. Neither surface reads the other's credential in any code path.
10. **The approvals login.** `GET /approvals` with no session → **302** to `/approvals/login`.
    `GET /approvals/login` → **302** to the configured authorization endpoint carrying
    `response_type=code`, `code_challenge_method=S256`, a `code_challenge` that is the base64url
    SHA-256 of the verifier the server kept, `state`, `resource` equal to the derived audience, and
    `redirect_uri` equal to `<WILLIKINS_PUBLIC_URL>/approvals/callback`; it also sets
    `__Host-willikins-login` with `Secure`, `HttpOnly`, `SameSite=Lax` and a 300 s lifetime. The
    callback with a good code sends `resource` on the token request too, authenticates with
    `client_secret_basic` (the fake's `/token` rejects a request missing either), and sets
    `__Host-willikins-session` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/` and no
    `Domain`; the response carries `X-Frame-Options: DENY` and a CSP with `frame-ancestors 'none'`
    and `form-action 'self'`. A callback with an unknown, replayed or expired `state` → **400**;
    with an `iss` parameter that is not the configured issuer → **400**; **with no
    `__Host-willikins-login` cookie, or one naming a different pending login → 400**, which is the
    two-approver login-CSRF of decision 4. A token endpoint that hangs → the callback answers
    within `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS` and creates no session. A handler that fails to
    return the jar sets no cookie — asserted, because `axum-extra` documents that footgun and a
    silent failure here is an open approvals page. A session older than
    `WILLIKINS_SESSION_TTL_SECONDS`, and a session used after `POST /approvals/logout`, → **302**
    to `/approvals/login` on a GET and **403** on a POST. Basic credentials presented → the
    identical response to none, with no `WWW-Authenticate: Basic` anywhere. The per-plan nonce is
    still required: a `POST` without it is **403** and journals
    `AuthFailedReason::InvalidNonce`. Filling the pending-login store → **503**; filling the
    session store → **503**.
11. **The approver role.** A session whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS` → the
    approve and reject POSTs are **403** `AuthFailedReason::WrongRole` and nothing is journaled
    as a grant; the pending list is not rendered; and the 403 page **displays that session's own
    `sub` and `iss`**, escaped, so the operator can copy them into the allowlist. A session whose
    token lacked `willikins:approve` but whose `sub` is listed → **403** as well. Both together →
    the decision is journaled with the derived principal and the approver's `subject`.
12. **Startup refusals and the retired variables.** `WILLIKINS_AGENT_TOKEN_HASHES` set (even
    empty-valued) → `HttpConfigError::RetiredVariable { name }`, and the same for
    `WILLIKINS_APPROVER_TOKEN_HASH`; the message names the OAuth variable that replaces it. Each
    missing required OAuth variable → `ConfigError::Missing` naming it. An empty or unset
    `WILLIKINS_AGENT_SUBJECTS` or `WILLIKINS_APPROVER_SUBJECTS` →
    `HttpConfigError::EmptyAllowlist` naming which. A symmetric algorithm in the allowlist →
    `HttpConfigError::SymmetricAlgorithm`. `WILLIKINS_SESSION_TTL_SECONDS` larger than the approval
    window → `HttpConfigError::SessionOutlivesApprovalWindow`. A `WILLIKINS_PUBLIC_URL` carrying a
    path, a query or a fragment → refused. A provider URL that is `http://` on a **non-loopback**
    host → `HttpConfigError::InsecureUrl`; the same URL on `127.0.0.1`, `[::1]` or `localhost` →
    **starts**, with a startup warning naming the URL (decision 7, both readings). An unreachable
    JWKS at startup → `StartError::JwksUnavailable`. A loopback `--bind` with no OAuth
    configuration → the same refusals as any other bind (decision 5). `hash-token` is gone from
    both binaries: invoking it exits non-zero with a usage error.
13. **Bounded header read.** The pass-2 slowloris test, un-`#[ignore]`d: a client that trickles
    headers is disconnected within the configured header-read timeout, the server keeps serving
    other connections, and graceful shutdown still drains an in-flight run.
14. **IPv6-aware origin check.** An `Origin` of `https://[::1]:8080` against an allowed host of
    `[::1]:8080` is accepted; `https://[::2]` is **403** `AuthFailedReason::ForeignOrigin`; a
    bracketed literal never parses as a hostname containing a colon.
15. **Log level and the rmcp sweep.** With `WILLIKINS_LOG` raised to DEBUG and to TRACE, a full
    cycle (login, plan, approve, apply, a provider error, an auth failure) emits no secret byte,
    no document text, no header value and no JSON-RPC request or response body on stderr. The
    sweep records what rmcp itself emits at each level; anything that would print a body makes
    the level refused rather than the test relaxed.
16. **The concurrency permit.** The 64-call bound still refuses with the kind-tagged `Busy`, and
    the permit is held by the blocking closure — asserted by a test that would have passed before
    only if rmcp ran the handler on its own task. This is about where the permit is held and
    nothing else; it does not depend on any rmcp status mapping.
17. **Adversarial pass 3.** Every attack in decision 7's table, recorded under `docs/research/`
    as passes 1 and 2 were, with every bypass becoming a fixture plus a test.
18. **The live token test.** Opt-in, `#[ignore]`d, behind a new `live-tests` feature on
    `willikins-server` plus `WILLIKINS_LIVE_TESTS=1`, never in the workspace gate. It fetches the
    real provider's discovery document and JWKS, runs a real server against the real issuer, calls
    `list_tools` with an operator-supplied token and asserts **200**, then asserts **401** for no
    token, a truncated token, a tampered signature, and the same token against a server started
    with a **different `WILLIKINS_PUBLIC_URL`**, which is how the audience varies now that it is
    derived. It prints the token's complete JOSE header (`typ`, `alg`, `kid`, whatever else) and
    its claim names, so verify item 6 — decision 14's frozen `typ` set as a **provider
    precondition**, which claim carries the scopes (`scope` or `scp`, decision 3), decision 2's
    tolerance of a missing `client_id`, and decision 16's `kid` assumption — is answered by
    running it.
19. **Go-live checks**, run by hand with the operator, in decision 9's order: the measured `Host`
    and `X-Forwarded-Host`; what the edge does to a trickled header stream; the PRM document
    fetched over the public domain by a client with no credential; a 401 whose `resource_metadata`
    URL resolves; `railway config plan` showing no variable delete; whether a push-triggered deploy
    picks up staged variable edits, and if it does not, that the previous deployment stays healthy
    behind the healthcheck; the old variables gone before the deploy; `WILLIKINS_FAKE_CATALOG`
    gone after it; and one real plan-approve-apply cycle. Plus the step that is the goal's own
    sentence and that nothing else in this list tests: **a stock MCP client that has never met
    this deployment completes discovery, registration (CIMD or DCR), login and a `list_tools` call
    against the public domain and the chosen provider**, by hand with the operator. Every other
    test in this plan configures its client; this is the only one that does not, and the goal is
    written about a client that was never configured.

## Gates

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

All four before every commit, bare `cargo`, in the background with a 600,000 ms timeout, reading
the log body rather than a captured exit code. `-j 2` and `RUST_TEST_THREADS=2` are load-bearing
on this host; never run two cargo commands at once. The live token test (18) and the go-live
checks (19) are `#[ignore]`d or by hand and are never part of the gate. Every Workflow `CONTEXT`
string names these four commands and no wrapper.

## Tasks

**Implementation starts only after the milestone 2 plan carries `Completed`.** Go-live steps 6 and
7 are milestone 2's own remaining operator work, and starting 2c's code before that plan is closed
means two open milestones competing for the same deployment.

Dependency order. **Everything runs sequentially on `main`**, one lane at a time: this host has
11 GB of RAM shared with other sessions, and the milestone 2 experiment with parallel worktree
lanes took six hours and was OOM-killed. Agents stage only their own paths and the coordinator
commits by path.

| # | Task | Depends on | Delegate to |
| --- | --- | --- | --- |
| 0 | Research: `docs/research/2026-09-16-m2c-authorization.md`. **Done 2026-09-16** | | six parallel passes |
| 1 | Dependency pins: `jsonwebtoken` 11 with a backend chosen by building **both** in the Dockerfile image, `axum-extra` 0.12 with `cookie-signed`; record the backend choice and its reason in the Dockerfile; amend the "no `aws-lc-sys`, `openssl-sys`, or `cmake`" comment if `aws_lc_rs` wins | 0 | sonnet, verified by opus |
| 1b | rmcp 3.4.0 (conditional, decision 18): `cargo update -p rmcp --precise 3.4.0`, the `ServerInfo` → `ServerConfig` rename at three `mcp.rs` sites, `enforce_origin_validation()`, all four gates. Lands it or records why not; nothing depends on it, and the `allowed_origins` value itself is task 4's, at 3.3.0 | 1 | sonnet, verified by opus |
| 2 | **First commit:** freeze `pre-2c-every-event.jsonl` from the current binary, before any 2c change (decision 2), through an `#[ignore]`d generator in the shape of `willikins-journal`'s two existing ones. Then the in-process fake authorization server (decision 7): JWKS with `kid`, `/authorize` and `/token` with real PKCE, `client_secret_basic` and `resource` checks, `mint(flaws)`, key rotation, a hanging JWKS, a wrong-`iss` authorization response, **a blocking `start()`** for tests that spawn the binary, and the shared environment-block helper. No metadata documents. Shared test support, used by tasks 3, 4, 6, 7 and 10 | 1 | sonnet, verified by opus |
| 3 | The resource-server middleware, test-first against task 2: `OAuthConfig`, `JwkCache`, `validate`, `require_token`, the header strip, the subject-allowlist check, the new `AuthFailedReason` variants, the refusals of decisions 6, 7, 15 and 16. **Also removes `HttpConfig::build`'s agent-hash rules, and migrates every `HttpConfig::build` test site.** Tests 3, 4, 5, 6 | 2 | sonnet, verified by opus |
| 4 | **First step: fetch verify item 3's four documents** and record any new obligation here. Then the metadata document and the 401/403 surface: the RFC 9728 route outside the auth middleware and rmcp's nest, the insertion-built URL, the challenge header, and rmcp's `allowed_origins` set unconditionally. Tests 1, 2 | 3 | sonnet, verified by opus |
| 5 | Principal and journal: `oauth-<12 hex>` derivation, the `Principal { id, claims }` signature change across every site decision 2 lists, the optional claim fields bounded and escaped (the `AuthFailedReason` variants landed in task 3). Test 8 | 3 | sonnet, verified by opus |
| 6 | Scopes: the four, the hierarchy, the `scope`/`scp` readers, the tool table, the body parse and the 403 with `insufficient_scope`, `scopes_supported`. Test 7 | 4, 5 | sonnet, verified by opus |
| 7 | The approvals login: authorization code with PKCE, `state`, the login cookie, the callback, `client_secret_basic` through `Credential::authorize_basic`, the signed `__Host-` session cookie, both capped stores, logout, `X-Frame-Options`/CSP, the `WrongRole` page's own `sub`, `WILLIKINS_APPROVER_SUBJECTS`. **Removes `basic_auth`, `TokenHash`, `matches_any` and the constant-time compare, and retires `WILLIKINS_APPROVER_TOKEN_HASH`**; the nonce and origin defences stay. Tests 9, 10, 11 | 6 | sonnet, verified by opus |
| 8 | Delete `hash-token` from both binaries and its README section; update the variable reference. Test 12's last case | 7 | sonnet |
| 9b | IPv6-aware origin check. Test 14 | 7 | sonnet |
| 9c | Two steps, in order: (1) `WILLIKINS_LOG` as a `tracing_subscriber` `EnvFilter` directive, which flips `crates/willikins-server/tests/adversarial_13.rs`'s `the_binary_ignores_rust_log_and_emits_nothing_below_info` (line 1249) into its successor; (2) the sweep of everything the process emits at DEBUG and TRACE, rmcp's own output included. Amend milestone 2's trust boundary 5 in that plan's own addendum style. Test 15 | 8 | sonnet, swept by opus |
| 9d | Move the concurrency permit into the blocking closure. Test 16 | 8 | sonnet |
| 10 | Adversarial pass 3 against the fake authorization server: decision 7's whole table, recorded under `docs/research/`; freeze `post-2c-new-shapes.jsonl` | 8, 9b, 9c, 9d | opus |
| 9a | Bound the header read: replace `axum::serve` with a `hyper::server::conn::http1` accept loop (timer first, decision 8) carrying graceful shutdown and connection accounting; flip the pass-2 slowloris test. Runs **after** pass 3, with a short pass-3 addendum recording that the transport changed under it. Test 13 | 10 | sonnet, verified by opus |
| 11 | The live token test (18): the `live-tests` feature and its `[[test]]` entry, written now and run when the operator supplies a token from the chosen provider | 9a | sonnet, run with the operator |
| 12 | Go-live (decision 9, test 19): domain, measured `Host` and edge header behaviour, allowed hosts, both subject allowlists, `.railway/railway.ts` and `railway config plan`, the retired variables removed in the same change, Doppler's three secrets, `WILLIKINS_FAKE_CATALOG` removed, the stock-client discovery run, one real cycle; mark this plan **Completed** | 11, operator | coordinator with the operator |

**Why 9a runs after pass 3 rather than before it.** Task 9a rewrites the transport: graceful
shutdown, connection accounting and the run-drain bound all move into hand-written code. Running it
before the adversarial pass means every finding in that pass is ambiguous between "the
authentication is wrong" and "the accept loop is wrong", which is exactly the confusion the Risks
section already asks to avoid. Pass 3 therefore runs against the transport milestone 2 shipped,
9a lands after it with test 13, and a short addendum records what changed underneath. The public
listener still gets the bound before it is public: task 12 depends on 11, which depends on 9a.

**Task 3's migration budget, counted rather than estimated.** `HttpConfig::build` takes the two
hash parameters that stop existing, so **every call site changes**. There are **12** in the tree:
one production call (`crates/willikins-server/src/cli.rs:314`) and **11 test sites across six
files** — four unit tests in `crates/willikins-server/src/http/config.rs` (lines 315, 328, 340,
352), three in `crates/willikins-server/tests/adversarial_10b.rs` (39, 357, 364), and one each in
`tests/blocking_pool_13.rs` (224), `tests/deploy_host_headers.rs` (65), `tests/http_server.rs` (97)
and `tests/http_smoke.rs` (36). A review count of 21 was high; the real number is 11, and it is 11
because several files reach `build` through a local `base_config()` helper rather than calling it
directly. Three more files set the hash **environment variables** and move with them:
`tests/binary_startup.rs`, `tests/adversarial_13.rs` and `tests/serve_http_deploy_pins.rs`. Task 2's
environment-block helper is what keeps that migration from being twelve hand-written OAuth blocks.

**Task 3 also has an ordering constraint inside `cli.rs`.** `cmd_serve_http` calls
`build_http_config` (line 253) *before* `build_runtime` (line 267), so the startup JWKS fetch cannot
live in `build_http_config`: there is no tokio runtime yet, and `spawn_blocking` has nothing to
spawn onto. The fetch happens after the runtime exists, inside it, which is also why
`JwksUnavailable` lives on `StartError` rather than on the `Copy` `HttpConfigError` (decision 6).

**Task 7 flips two pins in `crates/willikins-server/tests/adversarial_10b.rs`**, and it names them
rather than discovering them: `the_approver_token_presented_as_a_bearer_on_approvals_is_401`
(line 289) asserts a **401** carrying `WWW-Authenticate: Basic realm="willikins"` for a bearer
header at `GET /approvals`, which becomes the 302 of decision 4 with no challenge header at all,
and `an_empty_basic_username_is_403` and `an_approver_username_spelling_an_agent_principal_is_403`
(lines 272 and 253) assert Basic-username behaviour that stops existing along with
`MalformedUsername`.

Every implementation task is test-first: the failing test lands in the same commit series before
the behaviour, and every adversarial pass is opus.

## Verify with a browser

The research note's own verify list (its section 7) carries forward in full. These are the items
this plan actually depends on, in the order the tasks need them. Nothing below may be written
into frozen code before it is settled.

1. **Task 1: which `jsonwebtoken` crypto backend builds in the repository's Dockerfile.** No
   cargo command was permitted during the research, so neither `rust_crypto` nor `aws_lc_rs` was
   test-built; `aws_lc_rs` wants a C/C++ compiler the image deliberately omits.
2. **Task 1b: whether `cargo update -p rmcp --precise 3.4.0` resolves and the four gates pass.**
   The `ServerInfo` rename at three sites is the only breakage identifiable statically; 3.4.0's
   cancellation and pre-init changes were not verified against willikins' stateless
   configuration.
3. **Task 4's first step: the three 2026-07-28 sub-pages** — `/authorization-server-discovery`,
   `/client-registration`, `/security-considerations` — and the `modelcontextprotocol/ext-auth`
   extensions, none of which were fetched. The index says `security-considerations` covers
   "mix-up" attacks, a term absent from the page that was fetched, so a further resource-server
   obligation may exist. Task 4 fetches all four and records what it finds in this plan before
   freezing the metadata and challenge surface; this item has an owner rather than being a
   standing note.
4. **Task 3: every OAuth 2.1 section number.** draft-ietf-oauth-v2-1-16 is an Internet-Draft
   (rev 16, 2026-09-03, expires 2027-03-07) whose Status of This Memo forbids citing it other
   than as work in progress, and which already carries one unreconciled tension (refresh tokens:
   non-normative §10 "must" versus normative §4.3 "SHOULD"). Which draft each MCP section number
   resolves to is verified stable only for §5.2 — 2025-11-25 cites draft-13 eighteen times,
   2026-07-28 cites draft-13 nine times and draft-14 once, and the current revision is 16.
5. **Task 3: RFC 9700 and RFC 7662 were not fetched at all**, nor were draft-16 §4.3.1 and
   §7.5.1, nor RFC 9449 (DPoP), RFC 8705 (mTLS-bound), RFC 9126 (PAR) or RFC 9207 (`iss`). RFC
   9700 is the authority behind every OAuth 2.1 removal; §7.5.1 is the exact carve-out behind
   `code_challenge` being "REQUIRED unless".
6. **Task 3 and test 18: what `typ`, `alg` and claims the chosen provider actually emits**, which
   claim carries the scopes (`scope` as a space-separated string, or `scp` as an array — decision 3
   accepts both), and in particular whether its access token carries `client_id` — RFC 9068 §2.2's
   claim list was not fetched, only §4. Decision 14 turns the `typ` question into a **provider
   precondition** rather than a configurable default, so a provider that does not emit `at+jwt` is
   a decision to take with this answer in hand; decision 2's tolerance of an absent `client_id`
   depends on this too.
7. **Task 12: what `Host` header the service receives on public Railway traffic.** Not documented
   anywhere on `docs.railway.com`; it decides `WILLIKINS_ALLOWED_HOSTS` and rmcp's
   `allowed_hosts`. Measure against a live deployment.
8. **Task 12: whether Railway's edge overwrites a client-supplied `X-Real-IP`,
   `X-Forwarded-Host` or `X-Forwarded-Proto`.** Stated for none of the three, so none is
   spoof-proof; `X-Forwarded-For` appears on none of the seventeen fetched pages, which is not
   proof it is absent on a real request. Nothing in this plan reads a forwarded header, and
   nothing may start to before this is measured.
9. **Task 12: the service's public hostname.** It has none at present — the operator deleted the
   generated domain on 2026-09-15 — and a generated one cannot be declared in
   `.railway/railway.ts`. Read it from the dashboard or the CLI when it exists.
10. **Task 12: whether an omitted `domains` key deletes an already-attached custom domain**, what
    `preserve()` does for a variable that does not yet exist live, and whether a push-triggered
    deploy picks up variable edits staged on Railway's variables page. The documented "omit means
    delete" exemption covers only generated domains. Settle all three with a read-only
    `railway config plan` and one observed deploy; never by applying. If staged edits are not
    picked up by a push, the accepted outcome is one failed deployment behind a healthcheck that
    keeps the previous one live, which is itself a step of test 19.
11. **Decision 10 and test 18: the chosen provider's own claims.** For Logto specifically: its
    CIMD pages were fetched from the docs repository's `master` branch rather than the v1.43.0
    tag, so the feature may postdate the current release, and whether a token requested without a
    `resource` parameter is a JWT was not established. For any other provider the note's own
    caveats apply: Keycloak's token format is not stated outright and its RFC 8707 support is
    "planning"; Zitadel's default token format is unestablished and it accepts `resource` and
    ignores it; authentik's behaviour on `resource` at `/authorize` (as opposed to the
    token-exchange endpoint) is undocumented; Ory Network's free tier is unquotable; GitHub
    publishes no discovery document, which is a live-probe absence rather than a statement.

12. **Settled for rmcp 3.3.0; open only for 3.4.0: whether a tool handler can set the HTTP
    response status.** At 3.3.0 it cannot. `jsonrpc_http_status`
    (`rmcp-3.3.0/src/transport/streamable_http_server/tower.rs:626`) maps a handler's error to
    **400** (`UNSUPPORTED_PROTOCOL_VERSION`, `MISSING_REQUIRED_CLIENT_CAPABILITY`,
    `INVALID_PARAMS`), **404** (`METHOD_NOT_FOUND`) or **200**, and nothing else — so 401 and 403
    are unreachable from a handler and the scope check stays in the middleware (decision 3). Task 6
    needs nothing further. Whether 3.4.0 adds such a mechanism, behind its "map handler-generated
    HeaderMismatch to HTTP 400" release line, is unverified and is an opportunity rather than a
    dependency.
13. **Task 3: whether a non-`Bearer` `Authorization` header is malformed (400) or absent (401).**
    RFC 6750 §3.1 was not fetched. Today's behaviour is 401 `MissingCredential` and
    `adversarial_10b.rs` pins it; it is kept until the RFC is read.

## Risks

- **The audience is stable while it is published, and moving it costs a migration.** RFC 9728 §3.3
  makes the published `resource` byte-identical to the URL a client used, and RFC 8707 §3 warns
  that a multi-tenant resource needs the tenant in its URI. Milestone 3 routes credentials across
  several GitHub organizations and Doppler workplaces, so the identifier chosen here constrains how
  per-organization resources can later be expressed. Mitigation, in two halves: the host
  (`willikins.bandeabonnot.com`, go-live step 1) is one the operator owns, so a move off Railway
  does not move the identifier at all; and a move of the identifier itself is a migration with a
  documented window rather than a breakage, through decision 12's
  `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`. It is still client-visible: a client that never re-reads
  the challenge keeps sending the old resource and stops working when the window closes.
- **A provider that does not honour `resource`** still works for a client the operator
  configures and fails for one they do not. Decision 10's audience must-have is what keeps this
  visible; test 18 is what makes it concrete before anything depends on it.
- **The `rsa` advisory, if `rust_crypto` wins task 1.** RUSTSEC-2023-0071 has `patched = []`
  deliberately. The leak is of a *private* key through signing or decryption timing and a
  resource server only verifies with public keys from a JWKS, so it does not apply here — but any
  future `cargo-audit` or `cargo-deny` gate flags it forever and will need a documented ignore.
- **A plain-HTTP POST to the public domain is silently converted to a GET** at Railway's edge.
  An MCP client or an approvals form that reaches `http://` does not fail loudly; it gets a
  method it did not send. Mitigation: `WILLIKINS_PUBLIC_URL` is `https://` and the README says so;
  a GET at `/mcp` is already a 405 from rmcp, which is the visible symptom.
- **There is no per-IP rate limit at Railway's edge**, and the defence that remains is narrower
  than "the token buckets". `WILLIKINS_AGENT_SUBJECTS` bounds the set of principals that can reach
  a bucket at all (decision 3), so the shape an earlier draft worried about — an attacker minting
  cheap new subjects on an open-registration provider and getting a fresh bucket for each — does
  not exist: an unlisted subject is refused before any bucket is consulted. What remains
  attacker-reachable is the work done **before** authentication: signature verification is CPU an
  anonymous request can spend, one verification per forged token, and the edge rate-limits none of
  it. The per-reason journal bucket of decision 1 bounds the *write* amplification of that traffic
  but not its CPU. A per-IP limiter cannot be built until verify item 8 settles whether any
  forwarded header is trustworthy, and pass 3 measures what the flood actually costs.
- **A stolen session cookie is valid until its TTL expires or the browser logs out.** There is no
  operator-side revocation: the session store is in-process and has no "kill this session" surface,
  so an operator who believes a cookie leaked has exactly two options — wait out
  `WILLIKINS_SESSION_TTL_SECONDS` (default one hour), or redeploy, which empties the store. That is
  why the TTL is capped below the approval window and why it is short by default. Removing the
  subject from `WILLIKINS_APPROVER_SUBJECTS` and redeploying is the complete revocation.
- **Sessions, pending logins and the cookie signing key are process memory, and the deployment is
  one replica.** `.railway/railway.ts` pins `replicas: { "europe-west4-drams3a": 1 }`, which is
  what makes an in-memory session store correct at all: a second replica would serve a browser a
  cookie signed by a key it does not hold and a session id it has never seen, so every other
  request would bounce to the login. Scaling out needs a shared store and a configured signing key
  first, and that is a milestone 3 decision, not a dial to turn.
- **Sessions and nonces die on a redeploy.** Approving after a redeploy means logging in again.
  The journal is durable and a pending plan is unaffected; this is the same property milestone 2
  recorded for nonces, extended to sessions. The README gains the line.
- **Replacing `axum::serve` (task 9a) is a transport rewrite, not a line.** Graceful shutdown,
  connection accounting and the run-drain bound all move into hand-written code, and hyper 1.11
  panics outright if `header_read_timeout` is set without a timer. It is sequenced after
  adversarial pass 3 deliberately, so a regression there cannot be confused with an authentication
  finding.
- **Duplicate dependency majors, as build time and audit surface.** `Cargo.lock` today holds one
  `base64` (0.23.1) and one `sha2` (0.11.0), and already two `rand` majors (0.9.5, 0.10.2) and
  three of `getrandom`. `jsonwebtoken` 11 and `axum-extra`'s signed-cookie feature both want
  `base64` ^0.22 and `sha2` ^0.10, and the crypto backends pull `rand` 0.8 and `hmac`; if
  `rust_crypto` wins task 1 it adds `rsa`, `p256`, `p384` and `ed25519-dalek`, none of which is in
  the tree today. Cargo compiles all of them, which is fine and is not a correctness problem — it
  is minutes of cold build on an 11 GB host and a wider surface for the day an audit gate exists.
- **Build time.** `jsonwebtoken` plus a crypto backend adds minutes to a cold build on this host.
  Every agent runs cargo in the background with the 600,000 ms timeout, one at a time.

## Notes for milestone 3

- **A willikins that needs no authorization server beside it.** Must-have 1 of decision 10 says a
  deployment must be able to self-host its identity provider; it does not make that pleasant. A
  single-operator deployment today means running Logto (or another of the four) next to willikins
  for the sake of one human and a handful of agent clients. The end state for an open-source tool
  is a built-in, minimal authorization server: one operator account, client registration through
  CIMD, short-lived JWTs signed by a key the deployment generates, and nothing else — with the
  delegating path kept for anyone who already runs an identity provider. This milestone's shape is
  deliberately compatible with that: willikins is a resource server that trusts an issuer and a
  JWKS URI, so an internal issuer is a configuration value, not a rewrite. The costs the
  out-of-scope list names (exact `redirect_uri` matching, refresh-token rotation, a client
  registry, RFC 7591 and RFC 8414 endpoints, a user store) are what make it a milestone rather
  than a task.
- **Credential routing is unchanged by this milestone.** One GitHub organization, one Doppler
  workplace, per the go-live sequence's step 8. The proposed shape stays what milestone 2's notes
  recorded: route by `GitHubOrg` to `WILLIKINS_GITHUB_TOKEN_<ORG>`, add a `DopplerWorkplace`
  input for the Doppler half, and prefer a GitHub App minting short-lived installation tokens.
  The OAuth principal this milestone introduces is what a per-organization authorization decision
  would eventually key off, which is a reason to keep `subject` and `issuer` in the journal now.
- **A multi-tenant resource identifier.** If milestone 3 ever exposes per-organization resources,
  RFC 8707 §3 says the tenant belongs in the resource URI. That is a second audience, not a
  second server, and it is easier if the identifier chosen here has a path already.
- **Token introspection**, if a provider the operator later prefers issues opaque tokens. RFC
  7662 was never fetched; the cost is a network round trip per MCP request inside a 30 s budget,
  so it needs a cache with its own invalidation story.
- **Sender-constrained tokens** (DPoP, mTLS) if the threat model ever includes a stolen bearer
  token. Neither RFC was fetched.
- The remaining pass-2 hand-overs: validating tool outputs against declared port types and the
  distinguishable redaction marker (item 5), a `journal_version` field (7), `willikins journal
  repair` (8), and revisiting the concurrency bound with a real measurement (4). Item 6, measuring
  the journal fold, is **not** on this list any more: pass 3 measures it (decision 1), so what
  milestone 3 inherits is the decision about caching, with a number already in hand.
- **Edge rules** could fence `/approvals` by IP before a request reaches the service, but Client
  IP matching is IPv4-only and they need a plan allowance. Under Attack Mode cannot be used while
  `/mcp` and `/approvals` share a domain, because it turns away every non-browser request.

## Open decisions

One, the operator's. The other two this section carried are decided, and the header addendum records
them: the identity provider must be self-hostable open source (decision 10, must-have 1), and the
public host is `willikins.bandeabonnot.com` (go-live step 1).

**Which self-hosted provider. Recommended default: Logto, self-hosted (MPL-2.0).** The one reason:
of the six surveyed it is the only one that passes all three must-haves at once — it is open source
and self-hostable, it honours RFC 8707's `resource` parameter by name, and its Client ID Metadata
Documents give a client with no prior relationship a registration path (research §6). Each of the
others fails must-have 2 or 3 rather than the licence: Keycloak (Apache-2.0) "cannot recognize" the
`resource` parameter and its CIMD support is experimental; Zitadel (AGPL-3.0-only) accepts
`resource` and ignores it; authentik (MIT) rejects it and its dynamic registration needs a bearer
token; Ory Hydra (Apache-2.0) has no user store or login UI, so the operator would write the login
and consent app themselves; Auth0 is SaaS-only and fails must-have 1 outright. *Conditional* on
verify item 11: Logto's CIMD pages were read from the docs repository's `master` branch rather than
the v1.43.0 tag, and whether a token requested without a `resource` parameter is a JWT was not
established. **The question for the operator: accept self-hosted Logto, or name another
self-hostable provider to test against?** Nothing in `crates/` changes either way; what changes is
which issuer, JWKS URI and client the deployment is configured with, and which one test 18 runs
against. If the answer is "none of them, willikins should issue its own", that is the milestone-3
note above, and 2c ships against whichever of these is easiest to stand up in the meantime.

## Review resolutions

How each finding of the 2026-09-16 document review was resolved. Reviewers: coherence,
feasibility, security-lens, scope-guardian, adversarial. Findings from two or more reviewers on
the same point are merged; the reviewers in brackets are who raised it. Seventy-five findings
merged into the fifty-seven entries below.

1. **A scope was the only authority for every MCP tool** (adversarial P0, security P1). Decision 3
   argued that on an open-registration provider a scope alone is not authority — and applied that
   argument to `willikins:approve` only, leaving `willikins:apply` gated by a scope the provider's
   consent screen hands out. Extended to the whole MCP surface: a new required
   `WILLIKINS_AGENT_SUBJECTS`, empty or unset refusing startup, checked in the middleware before
   any tool runs, with a validated but unlisted `sub` answering 403
   `AuthFailedReason::UnlistedSubject`. "Who may call `apply`" now reads as
   `WILLIKINS_AGENT_SUBJECTS` ∩ the tokens carrying `willikins:apply`. Entries are bare `sub`
   values because a deployment has one issuer; a provider migration rewrites both lists, and
   issuer-qualified entries are recorded as milestone 3's answer. Trust boundary 3, decisions 1
   and 3, test 7, a pass-3 row, and go-live step 4 all changed.
2. **Unauthenticated refusals were unbounded journal writes** (security P1, adversarial P1). Once
   the listener is public, a garbage `Authorization` header is a journal append anyone can trigger.
   Credential-less and unverifiable refusals now pass through a per-reason token bucket (10 per
   minute, in memory); refusals beyond it are coalesced into one line per reason per window
   carrying a new optional `suppressed` field. Refusals of a *validated* token stay one to one,
   since a principal exists to rate-limit. Pass 3 gains a flood row, and the journal-fold
   measurement (pass-2 hand-over item 6) moves from milestone 3 into pass 3's scope, because a
   public listener makes fold cost attacker-reachable.
3. **The middleware was described as reading an "already-body-capped request"** (adversarial P1,
   security P2, feasibility P2 ×2, adversarial P2). It is not: rmcp's 1 MiB cap is applied inside
   its own service (`tower.rs:1704`) and axum's `DefaultBodyLimit` is layered only on the approvals
   router. Decision 3's "Where the check runs" is rewritten as five ordered steps: validate first,
   answer an unauthenticated 401 with a fixed `scope="willikins:read"` and no body read; only then
   buffer under the same `max_body_bytes` (413 on overflow) and parse with rmcp's own
   `ClientJsonRpcMessage`, so there is one parser of record; scope-check only a parsed `tools/call`;
   refuse an unparseable body 400 before rmcp; `GET` and `DELETE` require a token and then meet
   rmcp's own 405. Tests 2 and 7 amended and pass-3 rows added. Also recorded: `ClientJsonRpcMessage`
   has no batch variant at 3.3.0, so there is no multi-call bypass.
4. **https-only configuration versus a plain-HTTP loopback fake** (feasibility P1, security P2).
   The fake authorization server cannot hold a certificate, and a TLS trust hook for tests is
   production code with no production caller. Decision 7 and the variable table now say: every
   provider URL must be `https://` unless its host is a loopback literal, in which case `http://`
   is accepted with a named startup warning; a non-loopback `http://` URL is
   `HttpConfigError::InsecureUrl`. Permanent, not test-gated. Test 12 pins both readings, and
   neither `JwkCache` nor the token exchange needs a trust hook.
5. **The token endpoint's client authentication was unnamed, and the single outgoing-credential
   site was about to gain a second** (feasibility P1, adversarial P2). Decision 4 now names
   `client_secret_basic`, RFC 6749's mandatory-to-support method, sent through a new
   `Credential::authorize_basic(client_id)` so `willikins-providers-http`'s `Credential` methods
   stay the only sites that put a credential on the wire; `clippy.toml` and
   `crates/willikins-core/tests/expose_secret_guard.rs` gain that one named method. Trust boundary
   2 is reworded accordingly. The exchange gains its own timeout
   (`WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS`, default 5) and a response-size cap, and drops the
   refresh token and id token unread; test 10 gains a hang case. The rejected alternative — a
   public client with PKCE and no secret — is recorded with its reason. *Not applied as written in
   one respect:* `Credential::authorize` is `pub(crate)` by design, so `authorize_basic` is too,
   and the exchange reaches it through a new form-encoded entry point on
   `willikins_providers_http::Http`. Making it `pub` would hand callers a builder they can read the
   credential back out of, which is the footgun that crate's own doc comment names.
6. **The approvals page read `Authorization`, and the pending login was not bound to a browser**
   (adversarial P1 ×2, feasibility P2, security P2 ×2, security P3, adversarial P2 ×2, adversarial
   P3). Seven findings, one rule: `/approvals` never inspects `Authorization`, and a request
   without a valid session gets the identical response whatever its headers — GET 302 to
   `/approvals/login`, POST 403. The plan's internal inconsistency about which path redirects is
   settled (`GET /approvals` → 302 → `/approvals/login`, which mints the pending login and 302s to
   the provider), and "401 and a fresh redirect" is gone. The pending login is bound to the browser
   by a short-lived `__Host-willikins-login` cookie the callback must match, which closes the
   two-approver login-CSRF where an approver-attacker gets a second approver's browser to complete
   their callback and every decision the victim makes is journaled under the attacker's principal;
   decision 4's "no pre-login cookie, deliberately" paragraph is replaced by that reasoning. Both
   stores gain a TTL, prune-on-insert, a 1,024 cap and a 503, with pass-3 flood rows. The
   `SameSite` sentences are corrected: `Lax` is *required* because the callback's 302 arrives
   through a cross-site redirect chain `Strict` withholds cookies from, and `Lax` also blocks
   cross-site POSTs, which makes the nonce defence in depth rather than the only defence. The
   pass-3 row "a session cookie replayed from another browser" is dropped — an opaque session id
   with no client binding cannot distinguish browsers — and replaced by expired-session and
   post-logout rows; `POST /approvals/logout` is named in test 10. The `WrongRole` page shows the
   session's own `sub` and `iss`. CSP gains `form-action 'self'`. Risks records that a stolen
   session cookie is valid until TTL or logout with no operator-side revocation.
7. **Claims could not reach the journal with `PrincipalId` unchanged** (adversarial P1, feasibility
   P1). A derived principal carries no `sub`, `iss` or `client_id` by construction, so "the claims
   are journaled beside it" was a signature change nobody had budgeted. Decision 2 and the crate
   contracts now carry `Principal { id, claims }` across the transport boundary, and every `Butler`
   method that writes an event takes it — the ten that take a `PrincipalId` today plus
   `record_auth_failure`, which gains the parameter. Task 5 lists every changed signature. "`mcp.rs`
   changes in one place" is deleted. The claims repeat per event on purpose: the fold does not join
   across events, so a self-describing line is the property worth keeping.
8. **`.railway/railway.ts` was not part of the cutover** (adversarial P1 ×2, feasibility P1). The
   file `preserve()`s the two retired variables and its own rule is that an omitted field is an
   instruction to delete — so the next apply would have deleted the whole OAuth configuration.
   Go-live step 5 now edits the file in the same change: drop the two rows, add one per new
   `serve --http` variable, and require a read-only `railway config plan` showing no variable
   delete before any apply. The cutover order is stated (stage in Railway, edit the file, push the
   2c commit whose auto-deploy picks the staged variables up) with the fallback if a push-triggered
   deploy does not pick them up, and both open questions — that, and what `preserve()` does for a
   variable that does not exist live — are verify item 10 and test 19 steps.
9. **New refusals were attributed to the wrong error type** (feasibility P2). `StartupError`'s
   seven variants are the trusted-directory refusals and the `ServerStarted` write; none is a
   configuration refusal, so "matches the seven existing startup refusals" was wrong. The new
   refusals are attributed to `HttpConfigError::{RetiredVariable, SymmetricAlgorithm,
   SessionOutlivesApprovalWindow, InsecureUrl, EmptyAllowlist}` and `ConfigError::Missing`, and
   decision 6 says so explicitly. *Not applied as written for one variant:* `JwksUnavailable`
   cannot live on `HttpConfigError`, which is `Copy` and is produced by the pure `HttpConfig::build`
   *before* the tokio runtime exists. It goes on `cli.rs`'s `StartError`, beside `Bind`, produced
   inside the runtime — which is the same fact resolution 53 records about ordering.
10. **hyper's `header_read_timeout` panics without a timer** (feasibility P2). hyper 1.11 (1.11.1
    in the lock) panics if the timeout is set with no timer installed, which would kill the binary
    on its first connection. Task 9a and decision 8 now call
    `.timer(hyper_util::rt::TokioTimer::new())` on the builder first.
11. **`hyper-util`'s `server-auto` feature adds HTTP/2 to the release binary** (feasibility P3).
    Pinned to `["server", "http1", "tokio", "service"]` instead. *Extended beyond the resolution as
    written:* `server-auto` is what provides `hyper_util::server::conn::auto::Builder`, the builder
    decision 8 named, so dropping the feature required naming a replacement. Decision 8 and task 9a
    now build on `hyper::server::conn::http1::Builder` with `hyper` as a direct dependency
    (`["server", "http1"]`, a new pinned-dependencies row), `hyper_util`'s `TokioIo`, `TokioTimer`
    and `TowerToHyperService`, and a hand-rolled graceful drain rather than `server-graceful`.
12. **`tracing-subscriber`'s `env-filter` feature was not pinned** (feasibility P3). `WILLIKINS_LOG`
    is an `EnvFilter` directive and the workspace enables only `json` today. The pinned-dependencies
    table gains the row, owned by task 9c.
13. **`ureq` is a dev-dependency of `willikins-server`** (feasibility P3). The JWKS fetch is
    production code, so the table's JWKS row now says `ureq` moves into `[dependencies]` for that
    crate.
14. **`PrincipalId`'s grammar was cited at the wrong path** (feasibility P3). It is
    `PRINCIPAL_ID_PATTERN` at `crates/willikins-core/src/apply/principal.rs:13`, re-exported through
    `willikins_journal`, not `http/auth.rs`. Decision 2 corrected, with the line number.
15. **`oauth2`'s rejection rested on a false unification claim** (feasibility P3). Cargo compiles
    two semver-incompatible majors side by side, which this tree already does (`rand` 0.9.5 and
    0.10.2; three `getrandom` majors). The "cannot unify with the tree's 0.23.1" sentence is dropped
    and the rejection now rests on the reason that survives — every bundled client mismatches this
    tree. What duplicate majors actually cost is recorded under Risks, against the lock: `base64`
    0.22 and `sha2` 0.10 alongside the tree's 0.23.1 and 0.11.0, `rand` 0.8 as a third major,
    `hmac`, and, if `rust_crypto` wins task 1, `rsa`, `p256`, `p384` and `ed25519-dalek`.
16. **Test 6's header probe was a test-only tool an integration test cannot see** (feasibility P3).
    A `#[cfg(test)]` library item is invisible to a test under `tests/`, so the probe would have had
    to ship in the binary or hide behind a feature. It is now an axum layer the test wraps around
    `router()`'s output: same visibility into what rmcp receives, no feature, no production cost.
17. **The new fixture's provenance was unstated** (feasibility P3). Both existing fixtures come from
    an `#[ignore]`d `regenerate_the_frozen_fixture` writing to `$WILLIKINS_FIXTURE_OUT`
    (`crates/willikins-journal/tests/pre_pass_2_replay.rs:291` and
    `tests/post_pass_2_shapes.rs:217`). The journal contract now says `pre-2c-every-event.jsonl` is
    generated the same way, with the same "never point it at the committed file" warning.
18. **The `aws_lc_rs` premise was wrong** (feasibility P2). The Dockerfile's builder already
    installs `gcc` and `libc6-dev`, for `ring`, and says so in its own comment — so "wants a C/C++
    compiler the image deliberately omits" is false. Reworded: the open question is `cmake` and
    `bindgen` on targets with no prebuilt bindings, plus whether adopting it falsifies the
    Dockerfile's "no `aws-lc-sys`, `openssl-sys`, or `cmake`" comment. Task 1's build-both decision
    is unchanged.
19. **`willikins-server` has no `[features]` table** (feasibility P2), so "behind the crate's
    `live-tests` feature" named something that does not exist. Task 11 now adds
    `live-tests = []` plus the `[[test]]` entry with `required-features`, and decision 7 and test 18
    say so.
20. **`allowed_origins` already exists at rmcp 3.3.0** (feasibility P3), as a
    `StreamableHttpServerConfig` field with a `with_allowed_origins` builder (`tower.rs:119`,
    `:212`) — so it was not a reason to bump. Task 4 sets it unconditionally at the locked version,
    and decision 18's only remaining delta is `enforce_origin_validation()`, which makes an *empty*
    allowlist enforce rather than exempt (`tower.rs:884`).
21. **Task 1b's contingency was unstated** (coherence P1, scope P3). Decision 18 now says plainly:
    if 1b is skipped nothing changes, no task and no acceptance test depends on it, and recording
    why it was skipped is the whole obligation.
22. **Verify item 3 had no owner** (coherence P1). A "fetch this before freezing" with no task
    attached is a note nobody runs. Task 4's first step now fetches the three 2026-07-28 sub-pages
    and the `ext-auth` extensions and records any new resource-server obligation in this plan before
    the metadata and challenge surface are frozen; decision 11 and verify item 3 both name the
    owner.
23. **Test 16 looked like it depended on verify item 12** (coherence P2). Dismissed as a
    dependency: test 16 is about where the concurrency permit is held, not about a handler-set
    status, and with item 12 settled for 3.3.0 (resolution 3) there is nothing to depend on. Test
    16's last clause is reworded so it cannot be read as one.
24. **The pinned-dependencies table was missing rows** (coherence P2). Added: `ureq` as a runtime
    dependency, `tracing-subscriber`'s `env-filter` feature, the `hyper-util` feature set, and
    `hyper` itself. The other half of the finding — that `tower-sessions` was missing — is
    **dismissed**: it is already in the rejected paragraph, with its reason.
25. **The "Required" column said "Never; optional"** (coherence P2). Values are now `serve --http`
    or `optional`; the "Never;" prefix is gone.
26. **The out-of-scope list and decision 8 disagreed about the pass-2 hand-overs** (coherence P2,
    scope P3). The out-of-scope bullet now names every item and where it went: 1, 2, 3 and 9 are
    tasks 9a to 9d; 4 stays milestone 3's; 5, 7 and 8 stay milestone 3's; 6 moves into pass 3
    (resolution 2); 10 goes into pass 3.
27. **Trust boundary 5's amendment wording** (coherence P2). **Dismissed.** The preamble already
    quotes the replaced sentence verbatim before naming task 9c as its replacement, which is the
    clarity the finding asked for.
28. **Task 9c conflated two changes in one cell** (coherence P3). Split into two numbered steps in
    one row: first `WILLIKINS_LOG`, then the sweep. The test the first step flips is named —
    `the_binary_ignores_rust_log_and_emits_nothing_below_info`,
    `crates/willikins-server/tests/adversarial_13.rs:1249`.
29. **"Decision 10 contradicts Open decisions"** (coherence, rated P0). **Dismissed.** Decision 10
    keeps provider names out of `crates/` and out of the configuration *shape*; Open decisions
    recommends a deployment default for a human to accept or reject. A recommendation is not a code
    dependency. Both places already said so; decision 10 gains one sentence making the distinction
    explicit rather than implied.
30. **The goal's discovery-and-registration flow was tested by nothing** (scope P1). Every test
    configures its client, while the goal is written about a client that has never met the
    deployment. Test 19 gains a step: a stock MCP client with no pre-registration completes
    discovery, registration (CIMD or DCR), login and a `list_tools` call against the public domain
    and the chosen provider, by hand with the operator. Decision 10's "pinned by" names it.
31. **The fake authorization server served metadata willikins never reads** (scope P2). Decision 13
    is that willikins fetches no authorization-server metadata at all, so the RFC 8414 document, the
    OIDC discovery document and the lying-metadata mode had no consumer to attack. All three removed
    from decision 7 and task 2. What survives is the half with a consumer: the fake's `/authorize`
    can return a wrong `iss`, which the callback refuses. Test 18 still fetches the real provider's
    discovery document, and only to print it.
32. **`WILLIKINS_OAUTH_ACCEPTED_TYP` shipped an uncertainty as a knob** (scope P2, security
    residual). The variable and decision 14's widening warning are removed; the accepted `typ` set
    is frozen to `at+jwt` and `application/at+jwt` per RFC 9068 §4. "The provider emits `at+jwt`"
    joins decision 10's provider preconditions, checked by test 18, and a provider that does not is
    a decision to take then rather than a relaxation to carry now. Decision 14 keeps its number: a
    frozen statement is still a decision, and renumbering 15 to 18 would break every
    cross-reference to them.
33. **The audience and redirect URI were separately configurable** (scope P2, adversarial P2,
    feasibility P2). Two values that can be set independently are two values that can disagree, and
    test 1 requires the PRM `resource` to be byte-identical to the URL a client used.
    `WILLIKINS_OAUTH_AUDIENCE` and `WILLIKINS_OAUTH_REDIRECT_URI` are removed: the audience is
    always `<WILLIKINS_PUBLIC_URL>/mcp` and the redirect URI always
    `<WILLIKINS_PUBLIC_URL>/approvals/callback`, derived. Startup refuses a `WILLIKINS_PUBLIC_URL`
    with a path, query or fragment, or a non-https scheme (loopback excepted, resolution 4). Test
    1's rule now holds by construction, and test 18 varies the audience by varying the public URL.
34. **Adversarial pass 3 was gated on the transport rewrite** (scope P2). Task 10 now depends on 8,
    9b, 9c and 9d; task 9a runs *after* pass 3 with test 13 and a short pass-3 addendum, so a
    transport-rewrite regression is never confused with an authentication finding — which is what
    the Risks section already asked for and the task order contradicted. Tasks 11 and 12 depend on
    9a in turn, so the public listener still gets the bound before it is public.
35. **The edge's own bounds were unmeasured while 9a was justified as the only defence** (scope
    P2). Go-live step 3 now also measures what the edge does to a trickled header stream, and
    decision 8 and step 3 both cite research §5.2's documented numbers: a 32 KB combined header cap
    and idle HTTP/1.1 connections closed after 60 seconds. 9a is stated as defence in depth behind
    those rather than as the only bound.
36. **"The README already tells the operator to reload after a redeploy"** (scope P3). It does not
    — `grep -i reload README.md` returns nothing. Decision 4 now says the README *gains* the line,
    and Risks repeats it.
37. **`sub` was not a required claim** (security P2). Decision 2 derives the principal from it and
    both allowlists compare against it, so a token without one has no identity to journal or
    authorize. `sub` is added to `required_spec_claims`, with `AuthFailedReason::MissingSubject`, a
    test-3 case and a `mint(flaws)` flaw.
38. **The rate-limit risk described a bypass that no longer exists** (security P2). Covered by
    resolution 1 and reworded: the subject allowlist bounds the set of principals that can reach a
    bucket at all, so the cheap-subject shape is gone. What remains attacker-reachable is
    pre-authentication signature-verification CPU, which the edge does not rate-limit and which
    pass 3 measures.
39. **Store caps and TTLs were unspecified** (security P2, adversarial P2). Covered by resolution
    6: both stores are TTL'd, pruned on insert, capped at 1,024 and answer 503 when full, with
    pass-3 flood rows for each.
40. **A mixed-family algorithm allowlist would have failed every verification** (security P2).
    `jsonwebtoken` refuses a verifier whose key family differs from any allowed algorithm's family,
    so one `Validation` carrying `RS256` and `ES256` verifies nothing. Decision 15 now says the
    `Validation` is built per key as the allowlist ∩ that key's family, the token header selects
    nothing, and test 3 gains a two-family case.
41. **`resource` was sent only on the authorization request** (security P2). Decision 4 now sends
    it on the token request too — RFC 8707 defines it on both and a provider may honour only the
    second — and the fake's `/token` enforces its presence so it is a test rather than a claim.
42. **The operator had no way to learn their own `sub`** (security P2). Covered by resolution 6
    (the `WrongRole` page displays the session's `sub` and `iss`, escaped) plus go-live step 4,
    which makes "everyone logs in once and reads it off the refusal" an ordered step before the
    domain goes public.
43. **The `SameSite` rationale was wrong in both directions** (security P3, adversarial P3).
    Covered by resolution 6: `Lax` is required for the callback's cross-site redirect chain, and
    `Lax` also withholds cookies on cross-site POSTs, which demotes the nonce from sole defence to
    defence in depth.
44. **The middleware stripped only `Authorization`** (security P3). It now removes `Cookie` and
    `Proxy-Authorization` as well before `next.run` — rmcp hands handlers the complete headers, and
    a session cookie in handler scope is the same class of leak as a bearer token. Trust boundary
    2, the crate contract and test 6 all extended.
45. **Login CSRF had no pass-3 coverage** (security P3, adversarial P2). Covered by resolution 6,
    plus two pass-3 rows: a callback replayed a second time, and an authorize URL from one
    browser's login loaded in a second browser.
46. **Test 1's companion asserted 404 under the bearer middleware** (adversarial P2).
    `/mcp/.well-known/oauth-protected-resource` sits *inside* the `/mcp` nest, so it meets the
    bearer middleware before any route matching and the honest assertion is **401** — never 200 and
    never the PRM document at the wrong URL. Test 1 corrected.
47. **Tasks 3 to 7 left dead hash rules and an unbudgeted test migration** (adversarial P2,
    feasibility P2, feasibility P1). Task 3 now also removes `HttpConfig::build`'s hash rules and
    adds `RetiredVariable`, and task 8 shrinks to `hash-token` plus its README section. Task 2 adds
    a blocking `start()` for the fake — sync tests that spawn the binary need a JWKS from a fake on
    its own runtime thread — and an environment-block helper. Task 7 names the `adversarial_10b.rs`
    pins that flip. The migration is counted rather than estimated: **11 `HttpConfig::build` test
    sites across six files** (not the 21 the reviewer counted, because several files reach `build`
    through a local helper), named with line numbers in the task section, plus three more files
    that set the hash environment variables. *Not applied as written:* the resolution put both
    retirements in task 3, which would leave `/approvals` unauthenticated for four tasks —
    `basic_auth` compares a `TokenHash`, so neither can go without the other, and task 7 is where
    the login that replaces it lands. Split instead: task 3 retires
    `WILLIKINS_AGENT_TOKEN_HASHES` with the bearer-hash machinery, task 7 retires
    `WILLIKINS_APPROVER_TOKEN_HASH` with `basic_auth` and `TokenHash`. Decision 6's rule — no commit
    on `main` holds a set-but-ignored variable — is satisfied either way, and decision 6 now states
    the split and its reason.
48. **Trust boundary 5 overclaimed** (adversarial P3). "No static shared secret authenticates
    anything on the HTTP transport" is false while the approvals client secret exists. Reworded to
    "No static shared secret authenticates **a caller to willikins**", with the client secret named
    in the same paragraph, its direction stated (it authenticates willikins *to the provider*), and
    its blast radius bounded by the login-cookie binding and the two allowlists.
49. **Scope and role refusals had no named reasons** (adversarial P3, feasibility P2). Decision 1
    gains a check-to-variant table covering every check plus `MissingSubject`, `UnlistedSubject`,
    `InsufficientScope { needed }`, `WrongRole`, `InvalidNonce` and `ForeignOrigin`, and says which
    variants are retained. `InvalidCredential` and `MalformedUsername` are **retained on the wire
    and never produced after 2c** rather than retired: `pre-pass-2-every-event.jsonl` carries both,
    and removing a variant breaks the replay the additive discipline exists to guarantee.
    `AuthFailed` gains an optional `subject`, written only for refusals of a validated token.
    `Unparseable` is added beyond the resolution as written, because decision 1's checks all assume
    a parsed JOSE header and "not a JWT" had no variant.
50. **The manual development loop was left implicit** (feasibility P2). Decision 5 now says plainly
    that after 2c there is no dev-only mode: the fake authorization server is test-support code and
    is not reachable from a hand-run `serve --http`, so manual testing of the HTTP transport either
    happens inside `cargo test` against the fake or against a real provider with a real token. A
    `--dev-token` flag is named and refused, because it is the second authentication path decision
    5 exists to prevent.
51. **JWKS refresh-failure policy was unstated** (security residual). Decision 16 now says a failed
    background refresh keeps the previous key set and logs a warning at every failed interval; keys
    are never dropped on failure, because emptying the cache turns an unreachable provider into a
    total outage. The accepted staleness bound is stated: a removed key stays trusted for at most
    one refresh interval past the first successful fetch without it, and indefinitely while the
    provider is unreachable. Test 5 gains the case.
52. **Which claim carries the scopes was deferred** (security, deferred). Decision 3 now accepts
    both the `scope` claim (space-separated string) and an `scp` array, and fails closed on
    neither — a token with no scope claim carries the empty set, never everything. Which one the
    chosen provider emits is verify item 6, printed by test 18.
53. **`cli.rs`'s ordering was not accounted for** (feasibility residual). `cmd_serve_http` calls
    `build_http_config` (`crates/willikins-server/src/cli.rs:253`) before `build_runtime` (line
    267), so the startup JWKS fetch cannot live in `build_http_config` — there is no runtime to
    `spawn_blocking` onto. Task 3 records the constraint, and it is the reason `JwksUnavailable`
    lives on `StartError` rather than on `HttpConfigError` (resolution 9).
54. **The single-replica assumption was unstated** (adversarial residual). Risks now records that
    sessions, pending logins and the cookie signing key are process memory and that
    `.railway/railway.ts` pins one replica — which is what makes an in-memory session store correct
    at all. A second replica needs a shared store and a configured signing key first, which is a
    milestone 3 decision rather than a dial.
55. **Milestone 2's remaining operator steps were indistinguishable from new work** (scope,
    deferred). Go-live steps 6 and 7 now read "confirm it is already done (a milestone 2
    follow-up), or do it now", and the Tasks section opens by saying implementation of this
    milestone starts only after the milestone 2 plan carries `Completed`.
56. **Go-live numbering skipped 5** (coordinator). The sequence ran 1, 2, 3, 4, 6, 7, 8. Renumbered
    contiguously to 1 through 8 after the two new steps landed, and the four cross-references
    outside decision 9 were checked and corrected — decision 6's "step 4" is now step 5; the
    deployment section's "step 2", Risks' "step 1" and Open decisions' "step 3" still point at the
    right steps.
57. **The environment-variable count had to be restated** (scope, residual). After removing
    `WILLIKINS_OAUTH_ACCEPTED_TYP`, `WILLIKINS_OAUTH_AUDIENCE` and `WILLIKINS_OAUTH_REDIRECT_URI`
    and adding `WILLIKINS_AGENT_SUBJECTS` and `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS`, the count
    goes from eighteen to **seventeen added, ten required in http mode and seven optional**, and
    every tunable's row names the component that reads it.
