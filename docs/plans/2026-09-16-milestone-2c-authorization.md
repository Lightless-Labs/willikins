# Milestone 2c: authorization — OAuth 2.1 on `/mcp`, a browser login on `/approvals`

**Created:** 2026-09-16
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
bearer hashes and `hash-token` are gone: no shared secret authenticates anything on the HTTP
transport any more.

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
- The pass-2 hand-overs that are not exposure items: validating tool outputs against declared
  port types, the distinguishable redaction marker, measuring the journal fold before caching it,
  a `journal_version` field, `willikins journal repair`. They stay milestone 3's
  (`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`, "Handed to milestone 3 (or 2c)",
  items 5 to 8).
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
   middleware removes the `Authorization` header before `next.run`, because rmcp verifiably
   inserts the complete request `Parts` — headers included — into every handler's extensions
   (research §3.4). `Credential::authorize` stays the single site in the process that sets an
   outgoing `Authorization` header, and the existing `expose_secret` guard test is what keeps
   that checkable. Test 6.
3. **Two surfaces, two credential kinds, one source of the approver role.** `/mcp` accepts an
   `Authorization: Bearer` access token and never a session cookie. `/approvals` accepts a
   session cookie and never a bearer token. The approver role is not a token scope alone: it is
   the `willikins:approve` scope **and** the token subject being listed in
   `WILLIKINS_APPROVER_SUBJECTS`. Milestone 2's "the approver hash is not an agent hash" refusal
   has no analogue — the same human may legitimately hold both an MCP client token and a browser
   session — so the separation becomes structural rather than a startup comparison. Tests 9, 11.
4. **Every principal is derived from a validated token, never supplied.** No request field, no
   header and no tool parameter can name the caller. The principal is computed from claims that
   survived validation, and the claims it is computed from are journaled beside it. Test 8.
5. **No static shared secret authenticates anything on the HTTP transport.** The agent token
   hashes, the approver token hash and the `hash-token` subcommand are removed, not deprecated,
   and a deployment that still sets either variable refuses to start rather than ignoring it.
   The design doc's own words for this milestone are "short-lived credentials, no static bearer
   tokens". Test 12.

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

1. `typ` header is in the accepted set (`at+jwt` or `application/at+jwt` by default; decision 14).
2. `alg` header is in the configured allowlist, which may contain only asymmetric families
   (decision 15). `none` never validates, and `jsonwebtoken` refuses a verifier whose key family
   differs from an allowed algorithm's family (research §4.2).
3. Signature verifies against the JWKS key whose `kid` matches the header's (decision 16).
4. `iss` matches `WILLIKINS_OAUTH_ISSUER` exactly, as a string.
5. `aud` contains `WILLIKINS_OAUTH_AUDIENCE`, which is this server's resource identifier. RFC
   9068 says "contains", so an `aud` array carrying this resource among others is accepted.
6. `exp` is in the future, within `WILLIKINS_OAUTH_LEEWAY_SECONDS` of clock skew (decision 17).
   `nbf`, when present, is validated too — `jsonwebtoken`'s `validate_nbf` defaults to false and
   is overridden.
7. `exp`, `aud` and `iss` are required claims. `jsonwebtoken`'s `required_spec_claims` defaults
   to `{"exp"}` only, so a token with no `aud` at all would otherwise pass its audience check
   (research §4.2); `set_required_spec_claims(&["exp", "aud", "iss"])` is what closes that.

Every failure in that list answers **401** with `error="invalid_token"`, which is RFC 9068 §4's
own instruction, and is journaled as `AuthFailed` with a reason that says which check failed.
Insufficient scope is **403** with `error="insufficient_scope"` (decision 3), and the
specification reserves **400** for a malformed authorization request. Whether a non-`Bearer`
`Authorization` header counts as malformed (400) or simply absent (401) is verify item 13 — RFC
6750 §3.1 was not fetched — so today's behaviour, 401 `MissingCredential`, is kept until it is.
The status table itself is the specification's, verbatim (research §1.2).

**The document it serves.** One JSON object with `resource` (RFC 9728's only REQUIRED field),
`authorization_servers` with at least one entry (the MCP profile's MUST), `scopes_supported`
(RECOMMENDED), `bearer_methods_supported: ["header"]`, and `resource_name`. Parameters with zero
values are omitted, the response is 200 `application/json`, and the route is served outside the
bearer middleware and outside rmcp's host check exactly as `/healthz` is today — an
unauthenticated client must reach it before it holds a token, and `/healthz` is the existing
precedent for a root-level exemption (research §2.2, §3.3).

**The 401 shape.** `WWW-Authenticate: Bearer resource_metadata="<PRM URL>", scope="<the scopes
this request needed>"`, plus `error="invalid_token"` when a token was presented and rejected.
The specification's MUST is to implement *one of* the header or the well-known URI; clients
prefer the header, so willikins does both (research §1.2, §2.2).

**The passthrough prohibition, stated for a server that itself calls GitHub and Doppler.** The
specification says it twice: an MCP server must not pass through the token it received, and must
not accept a token not issued for it (research §1.2). willikins satisfies the first structurally
already, because no caller token has ever reached an upstream API and `Credential::authorize` is
the single outgoing-`Authorization` site. This milestone's job is to keep it that way while a
real OAuth token is in the request: the inbound token is consumed by the middleware, the header
is stripped before rmcp sees it, and nothing downstream can read it.

*Pinned by tests 1, 2, 3 and 6.*

### 2. Principal identity

Today a principal is `agent-<12 hex of the token hash>`. After 2c it is
`oauth-<12 hex of sha256(iss ‖ 0x00 ‖ sub)>`, and the claims it was derived from are recorded
beside it.

**Why not the subject itself.** `PrincipalId`'s grammar is
`^[A-Za-z0-9][A-Za-z0-9._@-]{0,127}$` (`crates/willikins-server/src/http/auth.rs`). A raw `sub`
need not fit it — a provider-qualified subject carrying a `|` is a common shape — and the
research note fetched no statement about any surveyed provider's subject format. A derived,
in-grammar, deterministic id keeps the existing discipline (the same identity always derives the
same principal, distinct identities never collide) and keeps provider-shaped text out of an
identifier that is compared, indexed and printed. The `iss` is folded in because a `sub` is only
unique within its issuer.

**What the journal records.** The claims themselves, as new **optional** fields on every event
that already carries a principal: `subject`, `issuer`, `client_id`. They are
authorization-server-supplied text, so each is bounded and escaped the way
`willikins_types::quoted` bounds a rejected literal before it is written, and each is omitted
when the claim is absent. `client_id` in particular must tolerate absence: RFC 9068 §2.2's claim
list was not fetched (only §4 was), so "a JWT access token always carries `client_id`" is a
verify item, not a fact this plan may rely on.

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

**Where the check runs, and how the challenge knows what to name.** The tool name lives in the
JSON-RPC body, which rmcp parses *after* the middleware — and rmcp exposes no way for a handler
to set an HTTP status that this research verified (the note found zero `WWW-Authenticate` or
`UNAUTHORIZED` paths anywhere in rmcp's server transport, in 3.3.0 or 3.4.0; 3.4.0's
"map handler-generated HeaderMismatch to HTTP 400" hints at a narrow mechanism that was not
fetched). So the scope check runs **in the middleware**, which reads the already-body-capped
request (1 MiB, unchanged) and branches on `method`: a `tools/call` is checked against the tool
named in `params.name`; `initialize`, `tools/list` and `ping` require a valid token and no scope.
That is what lets a 403 be a real HTTP 403 with a `WWW-Authenticate` challenge, and what lets the
401's `scope=` name the scope the refused call actually needed. *Verify item 12*: whether rmcp
3.3.0 or 3.4.0 lets a tool handler set the response status after all — if it does, the check
moves next to `principal_for` and the middleware stops parsing bodies. A legal fallback if
neither holds: a fixed `scope="willikins:read"` in the 401, since the specification says the
challenged scope set MAY be a subset, a superset, or neither, and names a minimal read scope as
the recommended initial set.

The hierarchy is a resource-server obligation at 2026-07-28 — "Servers **MUST** account for
scope hierarchies, where a broader scope implies narrower ones, when deciding whether a token is
sufficient for an operation" (research §1.3) — so a token carrying only `willikins:apply`
satisfies a `describe` call. `scopes_supported` lists exactly these four and never
`offline_access`, which that revision tells protected resources not to advertise. The 403's
`scope` attribute names only what the refused call needed, which is 2026-07-28's rule and the
reverse of 2025-11-25's; naming less is safe under both.

**Where the approver role comes from: an allowlist of subjects in configuration,
`WILLIKINS_APPROVER_SUBJECTS`, in addition to the `willikins:approve` scope.** A claim or a group
was considered and rejected: the claim name that carries roles or groups is per-provider, and the
research note fetched no group-claim name for any of the six providers, so building on one would
freeze a provider choice the operator has not made (decision 10). A scope alone is not enough
either, because on a provider that offers open registration — which is exactly the property
decision 10 requires — the set of clients that can *ask* for a scope is open by design. The
allowlist is the authority; the scope is the statement of intent, and both are required. It is
also the one place where "who may approve" is legible to an auditor reading the deployment's
configuration rather than the provider's console.

*Pinned by tests 7 and 11.*

### 4. The approvals page

Basic auth is removed. `GET /approvals` without a session redirects to the provider's
authorization endpoint with `response_type=code`, `code_challenge_method=S256`, a fresh
`code_verifier`, and a `state` value; the callback exchanges the code at the token endpoint with
willikins' own client credentials and sets a session cookie.

**willikins is a confidential client here.** A server-rendered page whose client credentials and
tokens stay on the server is draft-16 §2.1's "web application", which is a confidential client;
§9 "Browser-Based Apps" is still a TODO placeholder in draft-16 and cannot be relied on
(research §2.6). So the client secret lives in the process, reaches it through Doppler like the
two provider credentials, and is a `secrecy`-backed newtype with the redacted `Debug` and no
`Display` or `Serialize`, exactly as `Credential` is.

**What the login asks for, and where the subject comes from.** The authorization request carries
`scope=openid willikins:approve` and `resource=<WILLIKINS_OAUTH_AUDIENCE>` — the same resource
indicator an MCP client sends — so the access token the callback receives is an access token for
*this* server. The callback then runs it through `oauth::validate`, the same function and the same
configuration the `/mcp` middleware uses: one validation path, not two. The session's `sub`,
`iss` and granted scopes are the claims that validation returned, which is what gives test 11's
"the token lacked `willikins:approve`" a concrete meaning. A provider that will not issue such a
token for a browser login fails decision 10's first must-have, and the live test is where that
shows.

**PKCE is used even though this is a confidential client**, because draft-16 §4.1.1 makes
`code_challenge` REQUIRED (the carve-out it points at, §7.5.1, was not fetched — research §7.3)
and `plain` is prohibited. `state` is a random value held **server-side only**, in the pending-login store, paired with its
`code_verifier`, single-use, expiring in minutes; it is checked on the callback, and so is the
`iss` parameter when the authorization response carries one (2026-07-28's rule for clients;
research §1.3). No pre-login cookie binds it to the browser, deliberately: a login-CSRF that logs
the victim's browser into the *attacker's* identity gains nothing here, because approval
authority is not "has a session" but "this session's `sub` is in `WILLIKINS_APPROVER_SUBJECTS`"
(decision 3), and the per-plan single-use nonce is what protects the decision itself. A callback
whose `state` is unknown, already used, or expired is **400**, never a redirect.

**The session cookie**: name `__Host-willikins-session`, `Secure`, `HttpOnly`,
`SameSite=Lax`, `Path=/`, no `Domain`. Signed through `axum-extra`'s `SignedCookieJar` and
carrying an opaque session id only; the session record itself (subject, issuer, granted scopes,
expiry) is server-side and in memory. `Lax` rather than `Strict` because the browser arrives at
the callback by a cross-site redirect from the provider and must present the cookie on that
landing navigation; `Strict` is `tower-sessions`' default and would break exactly that hop
(research §4.4). The `__Host-` prefix, `Secure`, `HttpOnly`, `SameSite` and "signed or
server-side" are the confused-deputy consent-cookie checklist, which is a MUST only for an MCP
proxy server's consent page and is adopted here as the closest applicable analogue (research
§1.2, §8). `X-Frame-Options: DENY` and a `frame-ancestors 'none'` CSP are set on the page for
the same reason. The signing `Key` is generated at startup and never configured: a session
surviving a redeploy is not wanted (see below), so there is nothing to keep stable, and one
fewer secret is one fewer secret.

**Everything milestone 2 built on the page stays.** The single-use per-plan nonce, the
Origin/Referer check (made IPv6-aware by task 9b), the body cap, and the journaling of every
refusal with the `AuthFailedReason` that is literally true of it. The session replaces the
credential, not the CSRF defences: `SameSite=Lax` does not cover a top-level POST form
submission from another site, and the nonce does.

**Lifetime.** The session's lifetime is `WILLIKINS_SESSION_TTL_SECONDS`, default 3600, and it is
**capped below the approval window** (default 86400): a session must expire well inside the
window a plan can wait in, so that a plan pending overnight cannot be approved by a browser
nobody has re-authenticated in front of since. The session is in memory and is therefore lost on
a redeploy, exactly as the nonces already are; the README already tells the operator to reload
the page after a redeploy, and this extends that line to "log in again".

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
`plan`/`apply`/`approve` loop, which is the flow milestone 2 built for exactly this reason. The
cost is that a developer testing the HTTP transport by hand needs a token; the fake
authorization server of decision 7 mints one in-process, and the CLI keeps working with none.
Against that, the design doc's own sentence for this milestone is "short-lived credentials, no
static bearer tokens", and a loopback exemption is precisely a static bearer token that survives.

*Pinned by test 12.*

### 6. What happens to `hash-token` and the two hash variables

**Removed, and refused at startup on every bind** — not kept for loopback, not ignored.

`hash-token` is deleted from both binaries, along with its tests, and the README's section on it
goes with it. `WILLIKINS_AGENT_TOKEN_HASHES` and `WILLIKINS_APPROVER_TOKEN_HASH` become *retired*
variable names: if either is set, `serve --http` refuses to start with
`StartupError::RetiredVariable { name }`, naming the variable and pointing at the OAuth
configuration that replaces it.

The reasoning is the one `WILLIKINS_FAKE_CATALOG` already established in this codebase: a
variable that used to decide how the server authenticates must never quietly become a no-op. An
operator who upgrades the image and keeps the old variables set would otherwise have a service
that looks configured and is in fact open to anyone the provider will issue a token to, which is
the opposite of what those variables meant. Refusing is loud, happens before the listener binds,
and costs one deployment step in the go-live sequence (decision 9, step 4: delete the variables
*before* the deploy, never after).

*Pinned by test 12, whose last case is that `hash-token` no longer exists on either binary.*

### 7. The fake authorization server, and adversarial pass 3

Every attack in this milestone needs a token with a chosen flaw, and no external provider can be
asked for one. So the test harness grows an **in-process fake authorization server**:

- An axum router on a loopback port with an OS-assigned port number, started per test, in the
  same process, on the same runtime — the shape `crates/willikins-server/tests/http_smoke.rs`
  already uses for the server itself.
- It holds a test key pair generated in the test, serves `/jwks.json` (with a `kid`), serves an
  RFC 8414 authorization-server metadata document and an OIDC discovery document, and offers
  `/authorize` and `/token` endpoints good enough to drive the approvals login end to end
  (including a real PKCE `code_verifier` check, so test 10 proves willikins sends one).
- `mint(flaws)` returns a signed token with any combination of: wrong `aud`, wrong `iss`,
  `exp` in the past, `exp` inside the leeway, `alg: none`, a symmetric `alg`, an `alg` outside
  the allowlist, a `kid` that is not in the JWKS, no `kid`, a `typ` other than `at+jwt`, missing
  `aud`, missing `iss`, a scope set, an `aud` array containing this resource among others, and
  an unlisted `sub`.
- It can be told to rotate its key (serving a new `kid`), to serve a metadata document whose
  `issuer` disagrees with the URL it was fetched from, and to hang a JWKS response for longer
  than willikins' JWKS timeout.

This is what makes pass 3 deterministic rather than a story, and it is why task 2 comes before
task 3: the middleware is written test-first against it.

**Pass 3's attacks**, each with the status and reason the test asserts:

| Attack | Expected |
| --- | --- |
| Wrong audience | 401, `error="invalid_token"`, `AuthFailedReason::InvalidAudience` |
| Wrong issuer | 401, `invalid_token`, `AuthFailedReason::InvalidIssuer` |
| Expired beyond the leeway | 401, `invalid_token`, `AuthFailedReason::TokenExpired` |
| `alg: none`, and a symmetric `alg` signed with a guessed secret | 401, `invalid_token`, `AuthFailedReason::InvalidAlgorithm`; the symmetric case also fails startup validation if it is ever configured |
| A key rotated out of the JWKS | 401, `invalid_token`, `AuthFailedReason::UnknownKey`, and at most one refetch |
| A token minted for another resource (valid signature, valid issuer, other `aud`) | 401, `invalid_token`, `InvalidAudience` — the token-passthrough MUST, from the receiving side |
| A session cookie replayed against `/approvals` after the session expired, and one replayed from another browser | 401 (re-login), and the plan's nonce not burned |
| The passthrough case: a tool call whose provider request would carry the inbound token | the mock provider sees `Authorization` equal to the operator credential and nothing else; the handler sees no `Authorization` header at all |
| A metadata document that lies about its issuer | willikins never fetches AS metadata (decision 13), so the attack lands on the client half of the approvals login: the callback refuses with 400 when the authorization response's `iss` is not the configured issuer |
| A JWKS endpoint that hangs | the request answers 401 `invalid_token` within `WILLIKINS_JWKS_TIMEOUT_SECONDS`, the blocking pool does not fill, `/healthz` keeps answering |
| A token in the query string (`?access_token=...`) with no header | 401 with `MissingCredential`: OAuth 2.1 §5.1 is normative that resource servers MUST ignore an access token in a URI query parameter (research §2.6) |
| An `aud` array carrying this resource plus others | **accepted** — RFC 9068 says `aud` must *contain* a resource indicator for this server (research §2.5) |
| A bearer token presented at `/approvals`, and a session cookie presented at `/mcp` | 401 on both; neither surface ever reads the other's credential |
| `state` mismatch, `state` replayed, and a `redirect_uri` the deployment did not register | 400 on the first two; the third is the authorization server's refusal, asserted against the fake |
| A valid token whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS`, presented after login | 403 on the approve POST, `AuthFailedReason::WrongRole`, and the decision not journaled as a grant |
| A valid `willikins:read` token calling `apply` | 403, `error="insufficient_scope"`, `scope="willikins:apply"`, `resource_metadata` present |

Every bypass pass 3 finds becomes a fixture plus a test, as in passes 1 and 2, and the pass is
recorded under `docs/research/` with the same shape.

**One opt-in live login test, never in the workspace gate.** Interactive browser login cannot be
automated against a provider whose grant details this plan has not fetched, so the live test is
shaped like the existing live smoke test: behind the crate's `live-tests` feature plus
`WILLIKINS_LIVE_TESTS=1`, `#[ignore]`d, reading an operator-obtained short-lived access token
from an environment variable. It fetches the provider's real discovery document and JWKS, starts
a real server configured against the real issuer, calls `list_tools` with the real token and
asserts 200, then asserts the four refusals that need no minting (no token, a truncated token, a
token with a tampered signature, the same token against a server configured with a different
audience). The browser half of the login stays in "Verify with a browser". *Test 18.*

### 8. The exposure work pass 2 handed over

Four items, as tasks rather than notes, from `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`,
"Handed to milestone 3 (or 2c, the OAuth milestone)", items 1, 2, 3 and 9. They are here because
each one is a consequence of the same decision — no public domain until authentication is
stronger — and that decision changes in this milestone.

- **Bound the header read** (its item 1, finding 8). `axum::serve` does not expose hyper's
  `header_read_timeout`, so this replaces `axum::serve` with an accept loop over
  `hyper_util::server::conn::auto::Builder` carrying its own graceful shutdown and connection
  accounting. The pass left an `#[ignore]`d slowloris test written so that it fails once the
  header read is bounded; this task flips it into the assertion. *Task 9a, test 13.*
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

1. **Choose the host.** A generated `*.up.railway.app` domain or a custom domain. Either way
   the operator clicks Generate Domain or runs `railway domain`; a domain is not automatic
   (research §5.3). A custom domain needs **both** a CNAME and a TXT ownership record — with
   only the CNAME the domain answers 404 even after DNS resolves — and Railway issues the
   certificate itself, giving up after 72 hours. **Recommendation: a custom domain**, because
   the identifier it freezes is one the operator owns and can re-point if the deployment ever
   moves off Railway, whereas a generated hostname freezes a vendor name into a value that the
   RFC requires to be byte-identical forever after. That is a recommendation, not a decision;
   it is in "Open decisions" below.
2. **Declare it.** `.railway/railway.ts` can declare a custom domain (`domains: ["…"]`, or
   `{ domain, port }`) and **cannot** declare a generated one — that is documented, and it is the
   primary source behind the file's own header comment (research §5.4). So: a custom domain goes
   in the file; a generated one is created in the dashboard or by the CLI and the file's header
   comment is updated to say which. **Conditional in both directions on a verify item**: whether
   an omitted `domains` key deletes an already-attached *custom* domain is not stated anywhere —
   the documented "omit means delete" exemption covers only generated domains — so the file is
   never applied against a live custom domain until a read-only `railway config plan` has shown
   what the apply would do. Never settle this by applying.
3. **Measure what the edge sends.** Before any allowed-hosts value is frozen, deploy a build
   that echoes the inbound `Host`, `X-Forwarded-Host` and `X-Railway-Edge` on `/healthz` and
   read them from a real public request. What `Host` reads at the service is documented nowhere
   on `docs.railway.com` (research §5.2, §7.3) and it decides both `WILLIKINS_ALLOWED_HOSTS` and
   rmcp's `allowed_hosts`.
4. **One variable change, before the deploy that needs it.** In a single edit: set the whole
   OAuth block, give `WILLIKINS_ALLOWED_HOSTS` the measured public host alongside the
   private-domain reference, and delete `WILLIKINS_AGENT_TOKEN_HASHES` and
   `WILLIKINS_APPROVER_TOKEN_HASH`. Only then deploy the 2c image. Doing the deletion and the
   OAuth block as two changes around the deploy leaves a window where the service refuses to
   start — either `RetiredVariable` or `Missing`, depending on which half landed first — so they
   are one change. (`healthcheck.railway.app` needs no entry: `/healthz` is served outside the
   host check.)
6. **Connect Doppler's Railway integration for the credentials** — now three secrets, not two:
   `WILLIKINS_GITHUB_TOKEN`, `WILLIKINS_DOPPLER_TOKEN` and the approvals login's
   `WILLIKINS_OAUTH_CLIENT_SECRET`. Doppler's integration is the only path a real credential
   takes onto the service, as it has been since task 12.
7. **Remove `WILLIKINS_FAKE_CATALOG`.** Until this step the deployed service serves the fake,
   in-memory catalog; after it, the real one.
8. **Record the tenancy.** The live service serves **one GitHub organization and one Doppler
   workplace** until milestone 3's credential routing lands. The operator runs several of each
   (milestone 2 plan, "Notes for milestone 3"), and nothing in 2c changes that: a public domain
   changes who may reach the server, not how many organizations its two credentials cover.

*Pinned by test 19, which is a checklist run by hand with the operator, not a cargo test.*

### 10. The identity provider is the operator's decision

The plan is provider-agnostic. Any OIDC authorization server passes if it satisfies **two
must-haves**, stated as the research note defines them (research §6):

1. **Audience or resource-indicator binding.** The authorization server must be able to issue an
   access token whose audience is willikins' resource identifier. Honouring RFC 8707's `resource`
   parameter is the interoperable form, because that is the parameter a stock MCP client is
   required to send; a provider that binds the audience through a proprietary parameter instead
   still works for a client the operator configures, and does not work for one they do not.
2. **A registration path for MCP clients.** Client ID Metadata Documents, or an open Dynamic
   Client Registration endpoint, or pre-registration if the operator is content to register each
   client by hand before it can ever connect.

A third condition falls out of the out-of-scope list rather than the specification: **the
provider must issue JWT access tokens** for the registered resource, since introspection is out
of scope.

Everything provider-specific lives in **one configuration block** — issuer URL, JWKS URI,
audience, the approvals client's id and secret, the authorization and token endpoints, the
algorithm allowlist — and **one live test target** (test 18). No provider name appears anywhere
in `crates/`, in a fixture, or in an error message. Swapping providers is an environment change
plus one live test run.

*Pinned by test 3, which runs the whole validation matrix against the fake authorization server
and therefore against no provider at all, and by test 18 against the real one.*

### Eight smaller decisions the research note left open

**11. The revision anchor is 2026-07-28's resource-server obligations.** willikins already
advertises `V_2026_07_28` in `get_info` while rmcp 3.3.0 negotiates `2025-11-25` (research §1.1);
targeting the later revision's server-side rules satisfies both, because all three deltas are a
tightening: the scope-hierarchy MUST, the `offline_access` SHOULD NOT, and a 403 `scope` that
names only what is needed (2025-11-25 merely recommended naming more). *Conditional*: the
2026-07-28 sub-pages `/authorization-server-discovery`, `/client-registration` and
`/security-considerations` were not fetched, and the index says the last covers "mix-up" attacks,
a term absent from the page that was fetched — so a further resource-server obligation may exist.
Fetch all three before task 4 freezes the metadata and challenge surface.

**12. The resource identifier is `<public base URL>/mcp`, not the bare origin.** Both are legal
and picking one freezes the other (research §2.2). `/mcp` is picked because RFC 9728 §3.3 says
that when the client reached the document through the `resource_metadata` challenge, the
returned `resource` must be identical to the URL the client used to request the resource — and
that URL is `/mcp`. The origin also hosts `/approvals` and `/healthz`, which are not the
protected resource. The PRM document is therefore served at
`/.well-known/oauth-protected-resource/mcp`, built by **inserting** the well-known string between
host and path, never by appending it to the path — the research note calls this the single most
likely implementation mistake in the milestone. The identifier is configuration
(`WILLIKINS_PUBLIC_URL`, with the audience defaulting to `<public URL>/mcp`), never a literal in
code, because the host itself is verify item 1.

**13. Introspection, opaque tokens and AS-metadata discovery are all absent.** See the
out-of-scope list for the first two. For the third: willikins does not fetch the authorization
server's metadata document at all. The issuer and the JWKS URI are two configuration values the
operator already has, providers disagree on whether RFC 8414 is even served (Zitadel publishes
OIDC discovery and no RFC 8414 endpoint — research §6), and a startup-time fetch of a document
that is only used to learn two strings is a dependency with no gain. The consequence is stated
in pass 3's table: "a metadata document that lies about the issuer" cannot attack the resource
server, only the approvals login's callback, where the `iss` check catches it.

**14. The accepted `typ` set is configuration, defaulting to `at+jwt` and `application/at+jwt`.**
RFC 9068 §4 requires rejecting any other value (research §2.5), and that is the default. But no
fetched sentence says that any of the six surveyed providers actually sets `typ` to `at+jwt`, so
freezing it would be freezing an assumption about a provider not yet chosen.
`WILLIKINS_OAUTH_ACCEPTED_TYP` may be widened by the operator, the widening is logged at startup
as a named warning, and "what `typ` the chosen provider emits" is a verify item answered by test
18.

**15. The algorithm allowlist is configuration with no default, and may hold only asymmetric
families.** `WILLIKINS_OAUTH_ALGORITHMS` is required in http mode. Startup refuses a symmetric
family (`HS*`) with `StartupError::SymmetricAlgorithm`: a resource server verifying an HMAC would
have to hold the signing secret, which would make it able to mint its own tokens and break trust
boundary 1. No default algorithm is written into the plan because the research note fetched no
statement of any provider's signing algorithm, and `jsonwebtoken` checks the verifier's key
family against every allowed algorithm's family anyway (research §4.2), so a mismatch is a
startup or validation error rather than a silent acceptance.

**16. The JWKS is fetched at startup, refreshed in the background, and refetched on an unknown
`kid` at most once per window.** Startup fetches it and **refuses to start** if the fetch fails
(`StartupError::JwksUnavailable`), which matches the seven existing startup refusals and fails
closed. The fetch is `ureq` inside `tokio::task::spawn_blocking` — the tree has no async TLS
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

**17. Clock skew is bounded at `WILLIKINS_OAUTH_LEEWAY_SECONDS`, default 60.** That is
`jsonwebtoken`'s own default `leeway` (research §4.2), and it is made explicit rather than
inherited so that a future version changing its default cannot change willikins' behaviour
silently. `validate_nbf` is turned on, `required_spec_claims` is set to `exp`, `aud`, `iss`.

**18. The rmcp 3.4.0 bump is its own task and is conditional.** 3.4.0 does not move any
resource-server work into rmcp (research §3.5), so nothing here needs it; what it does offer is
`enforce_origin_validation()`, which validates Origin even with an empty allowlist — relevant the
moment a browser can reach the host. It is not a lock bump: `ServerInfo` is deprecated at 3.4.0
and used at three sites in `mcp.rs` under `-D warnings`. *Conditional*: no cargo command was
permitted during the research, so whether `cargo update -p rmcp --precise 3.4.0` resolves and the
four gates pass is unverified, as is the effect of 3.4.0's cancellation and pre-init changes on
willikins' stateless configuration. Task 1b either lands it with the rename and an
`allowed_origins` value, or records why it was skipped; nothing else in the milestone depends on
it.

## Pinned dependencies (new)

Caret requirements on the major; `Cargo.lock` pins the rest. Every version, MSRV and licence cell
below is quoted in the research note from a source fetched on 2026-09-16, and every one was
measured against the workspace's **declared** MSRV of 1.88 (`Cargo.toml:21`), not against the
1.97 toolchain the Dockerfile builds with.

| Crate | Requirement | Why |
| --- | --- | --- |
| `jsonwebtoken` | `{ version = "11", default-features = false, features = ["<backend>"] }` (11.0.0, MIT, MSRV 1.88, edition 2024) | validates the access token; parses the JWKS and selects by `kid` natively. Its defaults are not enough on their own — see decisions 15 and 17 |
| `axum-extra` | `{ version = "0.12", default-features = false, features = ["cookie-signed"] }` (0.12.6, MIT) | the approvals session cookie. Requires `axum ^0.8.9` and `axum-core ^0.5.2`; the lock holds 0.8.9 and 0.5.6, so no axum bump |
| `hyper-util` | `0.1`, feature `server-auto` (0.1.20 is already in the lock, pulled by axum; whether that feature is enabled there is a build-time check for task 9a) | task 9a's accept loop, for the header-read timeout `axum::serve` does not expose |
| JWKS fetch and cache | no crate — hand-rolled over the tree's `ureq` 3.4.2 inside `spawn_blocking`, parsed into `jsonwebtoken::jwk::JwkSet` | `jsonwebtoken` has no HTTP client, and the tree has no async TLS client at all |

Rejected, each for a fetched reason (research §4.1, §4.5): `reqwest` (a second async HTTP+TLS
stack in an image that has none), `oauth2` 5.0.0 (every bundled client mismatches this tree and
its `base64 >=0.21, <0.23` bound cannot unify with the tree's 0.23.1), `openidconnect` 4.0.1 (23
non-optional dependencies duplicating four majors, and it pulls `rsa`), `josekit` (non-optional
`openssl`), `jwt-simple` (BoringSSL by default, audience unchecked by default), `jwks-client`
(one release, 2020), `tower-sessions` (needs a session store this server does not have; its
cookie defaults are copied instead).

**The one open dependency question is a Dockerfile question.** Since 10.0.0 `jsonwebtoken`
requires exactly one crypto backend. `rust_crypto` is pure Rust and builds in the current image
unchanged but pulls `rsa` 0.9, which carries RUSTSEC-2023-0071 with `patched = []` — a private-key
timing leak, which does not apply to a resource server that only verifies with public keys, but
which any future `cargo-audit` or `cargo-deny` gate would flag forever. `aws_lc_rs` avoids it and
wants a C/C++ compiler the Dockerfile deliberately omits, and adopting it would falsify the
Dockerfile's own "no `aws-lc-sys`, `openssl-sys`, or `cmake` anywhere in the tree" comment.
**No cargo command was permitted during the research, so neither was test-built.** Task 1 builds
both in the image and picks; whichever wins, the choice is recorded in the Dockerfile with its
reason, and `rust_crypto` additionally gets a documented ignore entry ready for the day an audit
gate exists.

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
  the seven checks of decision 1, in order, each mapping to one `TokenRejection` variant and one
  `AuthFailedReason`. The function is pure over its inputs plus the cache, so the whole validation
  matrix is a unit test as well as an end-to-end one.
- `oauth::middleware::require_token` — extracts the header (and nothing else: a query parameter
  is not read), validates, derives the principal (decision 2), inserts `PrincipalId` and the
  granted scope set into the request extensions, **removes the `Authorization` header**, and calls
  `next.run`. On refusal it journals through `Butler::record_auth_failure` and answers the
  challenge of decision 1.
- `oauth::scope` — the four scopes, the hierarchy, and the tool-to-scope table of decision 3. The
  check runs in the middleware, on the parsed JSON-RPC envelope, for the reason decision 3 gives;
  the granted set is also carried in the request extensions so a handler can assert it.
- `oauth::metadata` — the RFC 9728 document and its route, registered on the root router outside
  both the auth middleware and rmcp's nest, exactly as `/healthz` is.
- `http::approvals` gains the login: `GET /approvals/login`, `GET /approvals/callback`,
  `POST /approvals/logout`, an in-memory `SessionStore` and `PendingLoginStore` (the `state` and
  `code_verifier` pairs, single-use, minutes-long), and the `SignedCookieJar` of decision 4. The
  nonce store, the Origin/Referer check and the body cap are untouched apart from task 9b.
- `http::auth` loses `bearer_auth`'s hash comparison and `basic_auth` entirely. `TokenHash`,
  `matches_any` and the constant-time compare go with them; nothing else in the crate uses them.
- `startup` gains the refusals of decision 6 and decisions 15 and 16, each its own
  `StartupError` variant with the variable name in the message.

**`mcp.rs`** changes in one place: `principal_for` reads the principal the OAuth middleware
inserted, unchanged in shape. It does **not** gain a scope refusal, because an authorization
refusal is a transport-level status in the specification's table rather than a domain error, and
a tool result cannot carry one (decision 3).

### willikins-journal

The new `AuthFailedReason` variants land in **task 3**, with the middleware that produces them,
because tests 3 to 5 assert them. The claim fields land in task 5. Both are additive.

Three new **optional** fields — `subject`, `issuer`, `client_id` — on every event that carries a
`principal`, each bounded and escaped before it is written (decision 2). No existing field
changes type, no variant is removed, no field becomes required. Both existing fixtures plus the
new `pre-2c-every-event.jsonl` replay unchanged through `Journal` and through
`willikins_journal::replay`. New `AuthFailedReason` variants, additively, one per check in
decision 1's list: `MissingCredential` and `WrongRole` already exist and keep their meaning.
`post-pass-2-new-shapes.jsonl`, which pass 2 froze as "the next pass's baseline", is that
baseline: `pre-2c-every-event.jsonl` supplements it with the events this milestone touches, cut
from the current binary before task 3 changes anything.

### willikins-cli

`hash-token` is removed. `serve --http` reaches the same configuration path as the binary, so it
inherits every new refusal. The CLI's own flow (`plan`, `apply`, `approve`, `reject`, `runs`,
`run` against a journal file) is unauthenticated and unchanged: it is the operator's local flow,
on the machine that holds the credentials, and decision 5's argument for removing loopback bearer
tokens rests on it staying that way.

### Dockerfile and deployment

One new build input (the `jsonwebtoken` backend of the dependency section) and one new secret
(`WILLIKINS_OAUTH_CLIENT_SECRET`) through Doppler's Railway integration. `.railway/railway.ts`
gains a `domains` entry only on the custom-domain branch of decision 9, step 2, and only after a
read-only `railway config plan`.

## Environment variables

Added:

| Variable | Default | Required | Value |
| --- | --- | --- | --- |
| `WILLIKINS_PUBLIC_URL` | none | `serve --http` | The canonical `https://` origin the service is reached at; no path, no query, no fragment. |
| `WILLIKINS_OAUTH_AUDIENCE` | `<WILLIKINS_PUBLIC_URL>/mcp` | Never; optional | This server's resource identifier. Must equal the PRM document's `resource`. |
| `WILLIKINS_OAUTH_ISSUER` | none | `serve --http` | The authorization server's issuer identifier, compared to `iss` by exact string match. |
| `WILLIKINS_OAUTH_JWKS_URI` | none | `serve --http` | An `https://` URL. Not discovered; see decision 13. |
| `WILLIKINS_OAUTH_ALGORITHMS` | none | `serve --http` | Comma-separated JWA names. Asymmetric families only. |
| `WILLIKINS_OAUTH_ACCEPTED_TYP` | `at+jwt,application/at+jwt` | Never; optional | Comma-separated. Widening is logged at startup. |
| `WILLIKINS_OAUTH_LEEWAY_SECONDS` | `60` | Never; optional | Whole seconds of clock skew. |
| `WILLIKINS_JWKS_TIMEOUT_SECONDS` | `5` | Never; optional | Whole seconds; must be well under the 30 s request timeout. |
| `WILLIKINS_JWKS_REFRESH_SECONDS` | `3600` | Never; optional | Background refresh interval. |
| `WILLIKINS_JWKS_MIN_REFETCH_SECONDS` | `60` | Never; optional | Floor between unknown-`kid` refetches. |
| `WILLIKINS_OAUTH_CLIENT_ID` | none | `serve --http` | The approvals page's own OAuth client id. |
| `WILLIKINS_OAUTH_CLIENT_SECRET` | none | `serve --http` | Confidential-client secret; a `secrecy` newtype, never logged. Doppler only. |
| `WILLIKINS_OAUTH_AUTHORIZE_URL` | none | `serve --http` | The provider's authorization endpoint. |
| `WILLIKINS_OAUTH_TOKEN_URL` | none | `serve --http` | The provider's token endpoint. |
| `WILLIKINS_OAUTH_REDIRECT_URI` | `<WILLIKINS_PUBLIC_URL>/approvals/callback` | Never; optional | Must match the value registered with the provider by exact string comparison. |
| `WILLIKINS_APPROVER_SUBJECTS` | none | `serve --http` | Comma-separated `sub` values allowed to approve. |
| `WILLIKINS_SESSION_TTL_SECONDS` | `3600` | Never; optional | Capped below `WILLIKINS_APPROVAL_WINDOW_SECONDS`; startup refuses a larger value. |
| `WILLIKINS_LOG` | the current INFO behaviour | Never; optional | A `tracing_subscriber` filter directive (task 9c). |

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
   byte-identical to `WILLIKINS_OAUTH_AUDIENCE`; `authorization_servers` has at least one entry;
   `scopes_supported` is exactly the four of decision 3 and never contains `offline_access`;
   `bearer_methods_supported` is `["header"]`; no parameter with zero values is present. The
   route answers with an unknown `Host` (it is outside rmcp's host check) and a bearer token is
   not required and not read. A companion test asserts the URL is built by insertion: a
   configured audience of `https://h/mcp` publishes at `/.well-known/oauth-protected-resource/mcp`
   and **404**s at `/mcp/.well-known/oauth-protected-resource`.
2. **The 401 challenge.** No `Authorization` header → **401**, `WWW-Authenticate` containing
   `Bearer`, `resource_metadata="<the exact URL from test 1>"` and `scope=` naming what the call
   needed, journaled `AuthFailedReason::MissingCredential`. A header that is not a `Bearer` credential (`Basic …`, say) → **401**
   `MissingCredential`, which is what ships today and what
   `crates/willikins-server/tests/adversarial_10b.rs` pins; whether RFC 6750 §3.1 calls that
   malformed, and therefore **400**, is verify item 13, and the task that ever flips it names
   every pinned test it flips. A well-formed but rejected token → **401** with the same header
   plus `error="invalid_token"`. `?access_token=` in the query string with no header → **401**
   `MissingCredential` (the parameter is never read).
3. **The validation matrix**, against the fake authorization server, one case per check of
   decision 1: wrong `aud`, missing `aud`, wrong `iss`, missing `iss`, `exp` in the past,
   `alg: none`, a symmetric `alg`, an `alg` outside the allowlist, a `typ` outside the accepted
   set, a `kid` absent from the JWKS, a header with no `kid`, a signature from a foreign key.
   Every one is **401** `error="invalid_token"` with the matching `AuthFailedReason`; a token
   whose `aud` is an array containing this resource among others is **200**. Each case also runs
   as a unit test against `oauth::validate` so the reason is asserted, not just the status.
4. **Clock skew.** A token expired by less than `WILLIKINS_OAUTH_LEEWAY_SECONDS` is accepted; one
   expired by more is **401** `AuthFailedReason::TokenExpired`. A token whose `nbf` is in the
   future is **401** — `jsonwebtoken`'s `validate_nbf` defaults to false, so this test is what
   proves the override is in place.
5. **JWKS cache, rollover and hang.** After the fake rotates its key, the first token signed with
   the new `kid` triggers exactly one refetch and is accepted; a second unknown `kid` inside
   `WILLIKINS_JWKS_MIN_REFETCH_SECONDS` triggers no refetch and is **401**
   `AuthFailedReason::UnknownKey`. With the JWKS endpoint hanging, a request with an unknown
   `kid` answers **401** within `WILLIKINS_JWKS_TIMEOUT_SECONDS`, `/healthz` still answers **200**
   throughout, and the blocking pool is not exhausted. Startup with an unreachable JWKS refuses
   with `StartupError::JwksUnavailable`.
6. **The inbound token neither leaves nor lands.** A test-only tool that returns the request's
   `Parts` headers sees **no** `Authorization` header. A tool call that makes a provider request
   against the mock HTTP server asserts the outgoing `Authorization` equals the operator
   credential and contains no substring of the inbound token. The existing `expose_secret` and
   `SinkToken::new` guard tests are extended to cover the new module, and a sweep over the
   journal, every log line and every error body produced during a full plan-approve-apply cycle
   asserts the token's bytes appear nowhere.
7. **Scopes and the 403.** A `willikins:read` token calling `apply` → **403** with
   `error="insufficient_scope"`, `scope="willikins:apply"` and `resource_metadata` present;
   calling `plan` → **403** `scope="willikins:plan"`. A `willikins:apply` token calling
   `describe` → **200** (the hierarchy). A token with no `scope` claim at all → **403** on every
   tool. `willikins:approve` alone → **403** on every MCP tool.
8. **Principal and journal.** The same `(iss, sub)` always derives the same
   `oauth-<12 hex>` principal and two different subjects never collide; the principal parses
   under `PrincipalId`'s grammar for a `sub` containing `|`, `:` and non-ASCII characters. Every
   event carrying a principal also carries `subject`, `issuer` and `client_id` when present, each
   bounded and escaped; a token with no `client_id` claim journals without it and does not fail.
   `pre-2c-every-event.jsonl` and both pass-2 fixtures replay through `Journal` and through
   `replay` unchanged.
9. **Transport separation.** A valid access token presented at `GET /approvals` → **401**, no
   session created. A valid session cookie presented at `/mcp` with no `Authorization` header →
   **401** `MissingCredential`. Neither surface reads the other's credential in any code path.
10. **The approvals login.** `GET /approvals` with no session → **302** to the configured
    authorization endpoint carrying `response_type=code`, `code_challenge_method=S256`, a
    `code_challenge` that is the base64url SHA-256 of the verifier the server kept, `state`, and
    `redirect_uri` equal to the configured value. The callback with a good code sets
    `__Host-willikins-session` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/` and no
    `Domain`, and the response carries `X-Frame-Options: DENY`. A callback with an unknown,
    replayed or expired `state` → **400**; with an `iss` parameter that is not the configured
    issuer → **400**. A handler that fails to return the jar sets no cookie — asserted, because
    `axum-extra` documents that footgun and a silent failure here is an open approvals page. A
    session older than `WILLIKINS_SESSION_TTL_SECONDS` → **401** and a fresh redirect. Basic
    credentials presented → **401** with no `WWW-Authenticate: Basic`. The per-plan nonce is
    still required: a `POST` without it is **403** and journals
    `AuthFailedReason::InvalidNonce`.
11. **The approver role.** A session whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS` → the
    approve and reject POSTs are **403** `AuthFailedReason::WrongRole` and nothing is journaled
    as a grant; the pending list is not rendered. A session whose token lacked
    `willikins:approve` but whose `sub` is listed → **403** as well. Both together → the decision
    is journaled with the derived principal and the approver's `subject`.
12. **Startup refusals and the retired variables.** `WILLIKINS_AGENT_TOKEN_HASHES` set (even
    empty-valued) → `StartupError::RetiredVariable { name }`, and the same for
    `WILLIKINS_APPROVER_TOKEN_HASH`; the message names the OAuth variable that replaces it. Each
    missing required OAuth variable → `StartupError::Missing` naming it. A symmetric algorithm in
    the allowlist → `StartupError::SymmetricAlgorithm`. `WILLIKINS_SESSION_TTL_SECONDS` larger
    than the approval window → `StartupError::SessionOutlivesApprovalWindow`. A loopback
    `--bind` with no OAuth configuration → the same refusals as any other bind (decision 5).
    `hash-token` is gone from both binaries: invoking it exits non-zero with a usage error.
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
    only if rmcp ran the handler on its own task.
17. **Adversarial pass 3.** Every attack in decision 7's table, recorded under `docs/research/`
    as passes 1 and 2 were, with every bypass becoming a fixture plus a test.
18. **The live token test.** Opt-in, `#[ignore]`d, behind the `live-tests` feature plus
    `WILLIKINS_LIVE_TESTS=1`, never in the workspace gate. It fetches the real provider's
    discovery document and JWKS, runs a real server against the real issuer, calls `list_tools`
    with an operator-supplied token and asserts **200**, then asserts **401** for no token, a
    truncated token, a tampered signature, and the same token against a server configured with a
    different audience. It prints the token's complete JOSE header (`typ`, `alg`, `kid`, whatever else)
    and its claim names, so verify item 6 — decision 14's accepted `typ` set, decision 2's
    tolerance of a missing `client_id`, and decision 16's `kid` assumption — is answered by
    running it.
19. **Go-live checks**, run by hand with the operator, in decision 9's order: the measured `Host`
    and `X-Forwarded-Host`, the PRM document fetched over the public domain by a client with no
    credential, a 401 whose `resource_metadata` URL resolves, the old variables gone before the
    deploy, `WILLIKINS_FAKE_CATALOG` gone after it, and one real plan-approve-apply cycle.

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

Dependency order. **Everything runs sequentially on `main`**, one lane at a time: this host has
11 GB of RAM shared with other sessions, and the milestone 2 experiment with parallel worktree
lanes took six hours and was OOM-killed. Agents stage only their own paths and the coordinator
commits by path.

| # | Task | Depends on | Delegate to |
| --- | --- | --- | --- |
| 0 | Research: `docs/research/2026-09-16-m2c-authorization.md`. **Done 2026-09-16** | | six parallel passes |
| 1 | Dependency pins: `jsonwebtoken` 11 with a backend chosen by building **both** in the Dockerfile image, `axum-extra` 0.12 with `cookie-signed`; record the backend choice and its reason in the Dockerfile; amend the "no `aws-lc-sys`, `openssl-sys`, or `cmake`" comment if `aws_lc_rs` wins | 0 | sonnet, verified by opus |
| 1b | rmcp 3.4.0 (conditional, decision 18): `cargo update -p rmcp --precise 3.4.0`, the `ServerInfo` → `ServerConfig` rename at three `mcp.rs` sites, `enforce_origin_validation()` with an `allowed_origins` value, all four gates. Lands it or records why not; nothing depends on it | 1 | sonnet, verified by opus |
| 2 | **First commit:** freeze `pre-2c-every-event.jsonl` from the current binary, before any 2c change (decision 2). Then the in-process fake authorization server (decision 7): JWKS with `kid`, both metadata documents, `/authorize` and `/token` with a real PKCE check, `mint(flaws)`, key rotation, a lying metadata document, a hanging JWKS. Shared test support, used by tasks 3, 4, 6, 7 and 10 | 1 | sonnet, verified by opus |
| 3 | The resource-server middleware, test-first against task 2: `OAuthConfig`, `JwkCache`, `validate`, `require_token`, the `Authorization` strip, the new `AuthFailedReason` variants, the startup refusals of decisions 15 and 16. Tests 3, 4, 5, 6 | 2 | sonnet, verified by opus |
| 4 | The metadata document and the 401/403 surface: the RFC 9728 route outside the auth middleware and rmcp's nest, the insertion-built URL, the challenge header. Tests 1, 2 | 3 | sonnet, verified by opus |
| 5 | Principal and journal: `oauth-<12 hex>` derivation, the three optional claim fields bounded and escaped (the `AuthFailedReason` variants landed in task 3). Test 8 | 3 | sonnet, verified by opus |
| 6 | Scopes: the four, the hierarchy, the tool table, the 403 with `insufficient_scope`, `scopes_supported`. Test 7 | 4, 5 | sonnet, verified by opus |
| 7 | The approvals login: authorization code with PKCE, `state`, the callback, the signed `__Host-` session cookie, the session store and its TTL cap, `X-Frame-Options`/CSP, Basic auth removed, the nonce and origin defences kept, `WILLIKINS_APPROVER_SUBJECTS`. Tests 9, 10, 11 | 6 | sonnet, verified by opus |
| 8 | Retire the static secrets: `hash-token` deleted from both binaries, both variables refused at startup, README and the variable reference updated. Test 12 | 7 | sonnet |
| 9a | Bound the header read: replace `axum::serve` with a `hyper_util` accept loop carrying graceful shutdown and connection accounting; flip the pass-2 slowloris test. Test 13 | 8 | sonnet, verified by opus |
| 9b | IPv6-aware origin check. Test 14 | 7 | sonnet |
| 9c | The log level story: `WILLIKINS_LOG`, then the sweep of everything the process emits at DEBUG and TRACE, rmcp's own output included; amend milestone 2's trust boundary 5 in that plan's own addendum style. Test 15 | 8 | sonnet, swept by opus |
| 9d | Move the concurrency permit into the blocking closure. Test 16 | 8 | sonnet |
| 10 | Adversarial pass 3 against the fake authorization server: decision 7's whole table, recorded under `docs/research/`; freeze `post-2c-new-shapes.jsonl` | 9a–9d | opus |
| 11 | The live token test (18), written now and run when the operator supplies a token from the chosen provider | 10 | sonnet, run with the operator |
| 12 | Go-live (decision 9, test 19): domain, measured `Host`, allowed hosts, the retired variables removed first, Doppler's three secrets, `WILLIKINS_FAKE_CATALOG` removed, one real cycle; mark this plan **Completed** | 11, operator | coordinator with the operator |

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
3. **Task 4: the three 2026-07-28 sub-pages** — `/authorization-server-discovery`,
   `/client-registration`, `/security-considerations` — and the `modelcontextprotocol/ext-auth`
   extensions, none of which were fetched. The index says `security-considerations` covers
   "mix-up" attacks, a term absent from the page that was fetched, so a further resource-server
   obligation may exist.
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
6. **Task 3 and test 18: what `typ`, `alg` and claims the chosen provider actually emits**, and
   in particular whether its access token carries `client_id` — RFC 9068 §2.2's claim list was
   not fetched, only §4. Decision 14's default and decision 2's tolerance of an absent
   `client_id` both depend on this.
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
10. **Task 12: whether an omitted `domains` key deletes an already-attached custom domain.** The
    documented "omit means delete" exemption covers only generated domains. Settle with a
    read-only `railway config plan`; never by applying.
11. **Decision 10 and test 18: the chosen provider's own claims.** For Logto specifically: its
    CIMD pages were fetched from the docs repository's `master` branch rather than the v1.43.0
    tag, so the feature may postdate the current release, and whether a token requested without a
    `resource` parameter is a JWT was not established. For any other provider the note's own
    caveats apply: Keycloak's token format is not stated outright and its RFC 8707 support is
    "planning"; Zitadel's default token format is unestablished and it accepts `resource` and
    ignores it; authentik's behaviour on `resource` at `/authorize` (as opposed to the
    token-exchange endpoint) is undocumented; Ory Network's free tier is unquotable; GitHub
    publishes no discovery document, which is a live-probe absence rather than a statement.

12. **Task 6: whether rmcp 3.3.0 or 3.4.0 lets a tool handler set the HTTP response status.**
    The research found no `WWW-Authenticate` or `UNAUTHORIZED` path anywhere in rmcp's server
    transport at either version, and 3.4.0's "map handler-generated HeaderMismatch to HTTP 400"
    release line was not chased to its source. If such a mechanism exists, the scope check moves
    out of the middleware and stops parsing bodies (decision 3).
13. **Task 3: whether a non-`Bearer` `Authorization` header is malformed (400) or absent (401).**
    RFC 6750 §3.1 was not fetched. Today's behaviour is 401 `MissingCredential` and
    `adversarial_10b.rs` pins it; it is kept until the RFC is read.

## Risks

- **The audience value is frozen the moment it is published.** RFC 9728 §3.3 makes the `resource`
  value byte-identical to the URL a client used; RFC 8707 §3 warns that a multi-tenant resource
  needs the tenant in the URI. Milestone 3 routes credentials across several GitHub organizations
  and Doppler workplaces, and whatever identifier 2c freezes constrains how per-organization
  resources can later be expressed. Mitigation: pick the host in step 1 of the go-live sequence
  with milestone 3 in mind, and treat a change of audience as a client-visible breaking change.
- **A provider that does not honour `resource`** still works for a client the operator
  configures and fails for one they do not. Decision 10's first must-have is what keeps this
  visible; test 18 is what makes it concrete before anything depends on it.
- **The `rsa` advisory, if `rust_crypto` wins task 1.** RUSTSEC-2023-0071 has `patched = []`
  deliberately. The leak is of a *private* key through signing or decryption timing and a
  resource server only verifies with public keys from a JWKS, so it does not apply here — but any
  future `cargo-audit` or `cargo-deny` gate flags it forever and will need a documented ignore.
- **A plain-HTTP POST to the public domain is silently converted to a GET** at Railway's edge.
  An MCP client or an approvals form that reaches `http://` does not fail loudly; it gets a
  method it did not send. Mitigation: `WILLIKINS_PUBLIC_URL` is `https://` and the README says so;
  a GET at `/mcp` is already a 405 from rmcp, which is the visible symptom.
- **There is no per-IP rate limit at Railway's edge**, and the per-principal token buckets key off
  a principal that now comes from a provider that may allow open client registration. A public
  domain therefore widens who can reach the rate limiter, not who can bypass it; the buckets stay
  the only defence, and a per-IP limiter cannot be built until verify item 8 settles whether any
  forwarded header is trustworthy.
- **Sessions and nonces are in memory and die on a redeploy.** Approving after a redeploy means
  logging in again. The journal is durable and a pending plan is unaffected; this is the same
  property milestone 2 recorded for nonces, extended to sessions.
- **Replacing `axum::serve` (task 9a) is a transport rewrite, not a line.** Graceful shutdown,
  connection accounting and the run-drain bound all move into hand-written code. It is sequenced
  after the OAuth work deliberately, so a regression there cannot be confused with an
  authentication failure.
- **Build time.** `jsonwebtoken` plus a crypto backend adds minutes to a cold build on this host.
  Every agent runs cargo in the background with the 600,000 ms timeout, one at a time.

## Notes for milestone 3

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
- The remaining pass-2 hand-overs: validating tool outputs against declared port types, the
  distinguishable redaction marker, measuring the journal fold before caching it, a
  `journal_version` field, and `willikins journal repair`.
- **Edge rules** could fence `/approvals` by IP before a request reaches the service, but Client
  IP matching is IPv4-only and they need a plan allowance. Under Attack Mode cannot be used while
  `/mcp` and `/approvals` share a domain, because it turns away every non-browser request.

## Open decisions

Two, both the operator's, each with the recommendation this plan would take by default and the
one question that settles it.

**The identity provider. Recommended default: Logto.** The one reason: of the six providers
surveyed it is the only one that passes both must-haves — it honours RFC 8707's `resource`
parameter by name, and its Client ID Metadata Documents give a client with no prior relationship
a registration path (research §6). Everything else in the table fails one or the other: Auth0 and
Zitadel bind the audience through a non-`resource` parameter, Keycloak "cannot recognize" the
parameter and its CIMD is experimental, authentik rejects it and its DCR needs a bearer token,
Ory Hydra has no login UI at all, and GitHub OAuth apps fail both and publish no discovery
document. *Conditional* on verify item 11: Logto's CIMD pages were read from the docs
repository's `master` branch rather than the v1.43.0 tag, and whether a token requested without a
`resource` parameter is a JWT was not established — test 18 answers both before anything is
frozen.

**The question for the operator: self-hosted Logto or Logto Cloud?** Self-hosting puts another
service on the operator's infrastructure and another thing to keep patched in the authentication
path; Cloud puts the identity of the only human who can approve a destructive plan in a vendor's
hands. Nothing in this plan depends on the answer — the configuration block is the same either
way — but the issuer URL, and therefore the audience and the registered `redirect_uri`, are
frozen by it.

**The public host. Recommended default: a custom domain on a name the operator owns.** The one
reason: the resource identifier, the audience, the PRM document's `resource` and the registered
`redirect_uri` are all frozen by it and RFC 9728 requires the value to match byte for byte
forever after, so a vendor hostname would put `up.railway.app` inside a value that cannot be
changed without breaking every client. **The question: which hostname?** It needs a CNAME and a
TXT record before Railway will route to it, and it is the value step 3 of the go-live sequence
measures `Host` against.
