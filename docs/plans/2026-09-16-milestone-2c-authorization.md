# Milestone 2c: authorization — willikins issues and validates its own tokens

**Created:** 2026-09-16
**Reviewed:** 2026-09-16 (via document-review workflow: coherence, feasibility, security-lens,
scope-guardian, adversarial; findings folded in below)
**Addendum:** 2026-10-20 (resumption, and one new operator decision). The plan was shelved on
2026-09-16 while milestones 2b and 3g to 3o landed. It is now restated against today's code so it
can be implemented. **The operator's new decision:** the deployed server reads its workflow
documents from the **mounted volume**, because the operator's documents are private and must never
ship in the public repository or its image (decision 25). Three research passes, all dated
2026-10-20, are folded in here. A code audit corrects stale paths, the test layout after 3k,
`StartupError`'s eight variants, eleven `Butler` methods once 3m's rollback is counted, the
teardown events, the five token-less `expose_secret` exceptions already in `clippy.toml`, and a
dependency tree that already carries `jsonwebtoken` 11 with `rust_crypto` and `p256`. It also
restores go-live steps 2 to 5, which an earlier edit had dropped. A client-and-spec pass checks
the plan against what the Claude and ChatGPT apps require. That pass adds four things: `state` is
echoed on every authorization response; the 401 challenge names the MCP scopes an agent needs;
the consent redirect gets an explicit CSP; and `WILLIKINS_PUBLIC_URL` must be canonical. A
volume pass supplies decision 25's Railway facts and code changes. Each fact the passes could not
fetch verbatim is a new verify item (24 onward). The precondition is met: the milestone 2 plan
carries `Completed: 2026-09-16`. What still needs the operator is under "Open questions,
2026-10-20" at the end.
**Addendum:** 2026-09-16 (the operator rules out any external identity provider; willikins becomes
its own authorization server). The operator's words: "I won't have the entire project's authn /
authz depend on the 'open source' edition of a SaaS ... There is *no way* I'll shove such a tool
down the throat of anyone who wants to use Willikins." Delegating to somebody else's provider is
banked instead as a later, pluggable improvement
(`todos/2026-09-16-pluggable-auth-adapters.md`), and the seam that keeps it cheap has its own
section below. Two documents did the work this addendum folds in:
`docs/research/2026-09-16-m2c-own-authorization-server.md` — six primary-source passes, the pin
table with licences and MSRVs, the maintenance and Dockerfile findings, and the verify list — and
`docs/research/2026-09-16-m2c-scope-revision.md`, which turns that into a decision-by-decision
revision of this plan, a task shape, the seam, the open decisions and the risks. **This addendum
supersedes the first of the three operator decisions in the addendum below**: there is no identity
provider left to be self-hostable, because there is no identity provider.
**Addendum:** 2026-09-16 (operator) — three decisions the operator made on reading the review: the
**Addendum:** 2026-09-16 (the four open decisions are settled by the operator) — **passkeys** for the human login ("Passkeys are good enough"); the signing key **Doppler-injected as two variables**, the current one and an optional retiring one; **pre-registered clients**; a **3600-second** access-token lifetime that is also the key-rotation overlap window. The Open decisions section is now empty of operator questions and records what each delta would have been. The operator also raised Doppler's secret **version history** against the two-variable shape: it is an audit and recovery feature, viewed in the dashboard, and it does not put two values in one running process, so it changes the *rollback* story rather than the overlap (decision 22). And they rejected this plan's word "permanently" for the WebAuthn relying-party identifier: "All it takes for to stop being permanent is wiping accounts and changing the domain." Correct, and the second time this plan has dressed a migration up as a law; every such sentence is now written as what the migration costs.
identity provider must be **self-hostable open source**, because willikins is an open-source tool
and an open-source tool may not require a third-party SaaS account to run ("Willikins is *open
source*, *first and foremost*. Since when do you make open source tools dependent on third-party
SaaS?") — **superseded, see above, by the decision that there is no separate provider at all**; the
public host is **`willikins.example.com`**; and no identifier is permanent ("But no, nothing
is forever"), so the audience gets a documented migration path instead of a promise of permanence.
Decisions 10 and 12 and the go-live sequence carry them. An earlier draft of this line claimed the
WebAuthn relying-party identifier as "the one identifier that turns out to be permanent after all";
it is not, and the operator said so: moving it costs a re-enrolment of every passkey, which
decision 12 now prices instead of forbidding.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (the 2026-09-15 addendum and milestone
2c in the milestone list)
**Previous:** `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`
**Research:** `docs/research/2026-09-16-m2c-authorization.md` (six parallel passes on the
resource-server half), `docs/research/2026-09-16-m2c-own-authorization-server.md` (six more on the
authorization-server half) and `docs/research/2026-09-16-m2c-scope-revision.md` (the revision this
plan is folded from), plus `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md` for the
exposure hand-overs. Every fact in all three is quoted from a source fetched on 2026-09-16, and
**no cargo command was run for any of them**.

Every section below that rests on something the research could not settle says so in place and
names the verify item it waits on. Nothing from either note's verify list is written into frozen
code here; where a decision touches one, the decision is stated as conditional and the condition is
named.

## Goal

The service gets a public domain and becomes its own authorization server. An MCP client reads two
metadata documents willikins publishes and learns where to get a token. The human behind that
client authenticates to willikins itself — no third-party account anywhere, and no vendor identity
product a self-hoster would inherit — consents at a page willikins renders, and the client receives
an access token willikins minted with this server as its audience. willikins then validates that
token **on the same fixed-order path it would validate any other token**, on every request, and
never forwards it anywhere. The operator approves a plan by logging in with the same identity in a
browser instead of typing a Basic-auth password the browser then stores. The static bearer hashes
and `hash-token` are gone: no shared secret authenticates a *caller* to willikins any more (trust
boundary 5 names the one secret that remains and says why it is not one).

One sentence of the delegating draft's goal does not survive, and it is replaced rather than
quietly dropped. That draft promised a client that has never met this deployment could discover,
register and connect unaided. Registration is now **pre-registration** (decision 23), one
configuration entry the operator adds by hand, so what a stranger client can do unaided is
*discover*: fetch both documents, learn the endpoints, the issuer and the one supported PKCE
method, and then be refused at `/authorize` by name until its `client_id` is registered. Test 18 is
written about exactly that pair — the unregistered client refused, the registered one completing
the flow — because a goal no test exercises is a goal nobody checks.

The milestone is done when every acceptance test below passes, including adversarial pass 3, which
attacks the **issuing** side at least as hard as the validating side, and the conformance run of
test 18 has driven a stock MCP client through discovery, `/authorize`, `/token` and `list_tools`
against a real deployment.

**The two clients the operator named are hosted, not local.** The operator, 2026-09-20
(`todos/2026-09-20-remote-mcp-from-a-phone.md`): "ideally, I should be able to use it straight
from the Claude or ChatGPT iOS apps, as a simple MCP". Both apps connect from their vendor's
infrastructure. They call `/mcp`, both well-known documents and `/token` from there, open
`/authorize` in the user's browser, and redirect to the vendor's own HTTPS callback. So the
milestone is also not done until test 19 has connected each app on the web and used it from that
app on iOS, against the public domain. Loopback redirects remain for local clients only.

## Out of scope

- **Refresh tokens.** Not issuing them is conformant — MCP says clients "MUST NOT assume refresh
  tokens will be issued; the authorization server retains discretion" — and it deletes the only
  stateful authorization-server requirement there is: rotation with reuse detection and family
  revocation. Decision 21 states the cost it buys instead.
- **Client ID Metadata Documents, and dynamic client registration.** CIMD is graded SHOULD and is
  worth adding when a named client needs one; dynamic registration is graded MAY, explicitly
  deprecated, and should not be built at all. Decision 23 prices both.
- **A revocation endpoint (RFC 7009).** A case-insensitive search for "revoke" across all five
  fetched MCP authorization pages returns zero hits and RFC 8414 makes `revocation_endpoint`
  OPTIONAL. With short-lived tokens and no refresh tokens, the leak window is the token lifetime.
- **Consent persistence** ("remember this client"). No requirement, and one consent click per
  authorization is a feature at this size rather than a cost.
- **Token introspection (RFC 7662) and opaque access tokens.** RFC 7662 was not fetched at all, and
  an introspection round trip sits inside a handler with a 30 s budget on every single request.
  This milestone issues and validates RFC 9068 JWTs, which is what makes introspection unnecessary.
- **Sender-constrained tokens.** RFC 9449 (DPoP) and RFC 8705 (mutual-TLS) were named by fetched
  text but not fetched. Bearer tokens over the edge's TLS are what the specification's own
  transport section describes.
- **OIDC discovery, ID tokens, and pushed authorization requests (RFC 9126).** RFC 8414 alone
  satisfies MCP's discovery MUST and is the cheaper branch; the rest were never MUSTs.
- **A per-user upstream OAuth flow to GitHub or Doppler.** Operator-provisioned credentials still
  reach both. willikins therefore is not an "MCP Proxy Server" in the specification's defined sense
  and the confused-deputy MUSTs — a per-user registry of approved `client_id`s consulted before
  forwarding upstream — do not apply. Credential routing across several GitHub organizations and
  Doppler workplaces is milestone 3's work, which is the same reason.
- **Multi-tenancy.** One server still serves one GitHub organization and one Doppler workplace
  until milestone 3's credential routing. The public deployment changes who can reach the server,
  not how many tenants it serves.
- **Multi-replica sessions and a shared session store.** `.railway/railway.ts` pins one replica,
  which is what makes an in-memory session store correct at all. Scaling out needs a shared store
  and a configured cookie signing key first, in that order.
- **Automated key-rotation scheduling.** The overlap *mechanism* is in scope and is not deferrable
  (decision 22); the cron is. No RFC prescribes a schedule.
- **Pluggable authentication adapters.** Banked at `todos/2026-09-16-pluggable-auth-adapters.md`.
  What this milestone owes that todo is a seam, not an abstraction: see "The pluggable seam" below.
- **The pass-2 hand-overs this milestone does not take**, named one by one so the split is not a
  judgement call later (`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`, "Handed to
  milestone 3 (or 2c, the OAuth milestone)"). Items **1, 2, 3 and 9** are *in* scope and are tasks
  17, 13, 14 and 15 below. Item **4** — revisit the concurrency bound with a real measurement, and
  make it configurable only if a deployment needs it — stays milestone 3's: task 15 moves the
  existing permit and changes no bound. Items **5 to 8** — validating tool outputs against declared
  port types and the distinguishable redaction marker, measuring the journal fold before caching
  it, a `journal_version` field, and `willikins journal repair` — stay milestone 3's, with one
  carve-out: the fold measurement (item 6) is pulled into adversarial pass 3's scope by review
  resolution 2 below, because a public listener makes the fold's cost attacker-reachable. Item
  **10** — tighten the authenticated foreign-`Host` assertion to the status rmcp actually returns —
  goes into pass 3.
- Push notifications, chat integration, MCP elicitation, composition (2b), templates (3).

## Trust boundaries

Milestone 2's five boundaries still hold, with one amendment: boundary 5's sentence "INFO is the
ceiling: the binary installs no `EnvFilter` and does not read `RUST_LOG`" is replaced by task 14
below, which gives the operator a level and sweeps what the process emits at it. This milestone
adds **six** boundaries. Each is normative for the crates below and has an acceptance test.

Boundary 1 was one boundary in the delegating draft and is now two. That is a rewrite and not an
amendment, and it is worth saying why in one sentence: four of the six clauses in "willikins
validates tokens and issues none" become false, so amending it would leave a boundary whose words
no longer describe the system while still being cited as though they did.

1a. **The resource-server boundary, and it survives intact.** At `/mcp` willikins is an OAuth 2.1
   resource server. It validates **every** token on the same fixed-order path whoever minted it,
   **its own included**, and never skips a check because it recognises the issuer, the key or the
   signature. `oauth::validate` has no "did I mint this" branch. This is not only hygiene: it is
   the seam the pluggable-auth todo rests on, and decision 7's foreign-issuer fixture is what
   asserts it rather than prose. Tests 1, 2, 3.
1b. **The authorization-server boundary, and it is new.** willikins holds exactly **one** live
   signing key pair, two only during a rotation overlap. It mints RFC 9068 access tokens for
   **one** audience. It issues **no** refresh token and runs **no** `/register`. It publishes only
   the public half, at `jwks_uri`. The private key never leaves the process, never reaches the
   journal, a log line or an error body, and has a redacted `Debug` with no `Display` and no
   `Serialize` — the `Credential`/`Value` discipline the codebase already enforces, applied to a
   signing key. Tests 20, 21, 22, 23.
2. **The inbound token is a credential for this server and for nothing else.** It is never
   attached to any provider request (GitHub, Doppler, Buildkite, SigNoz, App Store Connect or any
   later one), never exchanged for an upstream token, never written to the journal, a log line or
   an error message, and never reaches a tool handler. The auth middleware removes `Authorization`,
   `Cookie` and `Proxy-Authorization` before `next.run`, because rmcp verifiably inserts the
   complete request `Parts`, headers included, into every handler's extensions. **The sites that
   may read a secret's bytes are the ones `clippy.toml`'s `expose_secret` entry names, and no
   other.** Today those are:
   - the `Credential` methods in `willikins-providers-http`, which are the only sites that put an
     operator credential on an outgoing wire;
   - each secret domain type's derived `expose`;
   - **five token-less exceptions**, each scoped to its own `Tool::read`: `OpaqueSecret`'s and
     `DopplerSecretValue`'s `reveal_for_transform`, `AppleSigningKey`'s `reveal_for_signing`
     (signing one App Store Connect JWT), and `GitHubToken`'s and `BuildkiteToken`'s
     `reveal_for_authorization`.

   The inbound token is never an input to any of them. **2c adds exactly one site, a sixth
   exception.** The ES256 signing key cannot sign without reading its bytes. So it becomes a
   `#[domain(secret)]` type in `willikins-types`, modelled on `AppleSigningKey`, with its own
   `reveal_for_signing` called only by `issuer::mint`. That exception is named in `clippy.toml`'s
   reason and in `crates/willikins-core/tests/it/expose_secret_guard.rs`. The delegating draft's
   `Credential::authorize_basic` does not exist, because decision 4 removes the client and its
   secret together. Test 6.
3. **Two surfaces, two credential kinds, two configured allowlists.** `/mcp` accepts an
   `Authorization: Bearer` access token and never a session cookie. `/approvals` accepts a session
   cookie and never a bearer token — it does not so much as look at an `Authorization` header.
   Neither surface treats a scope as authority on its own. On `/mcp` a validated token acts only
   if its `sub` is listed in `WILLIKINS_AGENT_SUBJECTS`, and the scopes it carries then decide
   which tools it may call. On `/approvals` a session approves only if its `sub` is listed in
   `WILLIKINS_APPROVER_SUBJECTS` **and** its token carried `willikins:approve` — and the token in
   question is now one willikins minted for itself at the end of a passkey ceremony, which changes
   who issues it and changes nothing about the check. Milestone 2's "the approver hash is not an
   agent hash" refusal has no analogue — the same human may legitimately hold both an MCP client
   token and a browser session, and may be listed in both variables — so the separation becomes
   structural rather than a startup comparison. Tests 7, 9, 11.
4. **Every principal is derived from a validated token, never supplied.** No request field, no
   header and no tool parameter can name the caller, and that holds for a token willikins minted
   thirty milliseconds earlier exactly as it holds for one an adapter's provider minted. The
   principal is computed from claims that survived validation, and the claims it is computed from
   travel with it to the journal. Test 8.
5. **No static shared secret authenticates a caller to willikins.** The agent token hashes, the
   approver token hash and the `hash-token` subcommand are removed, not deprecated, and a
   deployment that still sets either variable refuses to start rather than ignoring it. The design
   doc's own words for this milestone are "short-lived credentials, no static bearer tokens". **The
   delegating draft's one carve-out is gone with its subject**: `WILLIKINS_OAUTH_CLIENT_SECRET`
   does not exist, because the approvals login is not an OAuth client of anyone (decision 4). What
   must **not** be written in its place is a claim that willikins now holds no secret at all. It
   holds a **signing** secret, which authenticates nothing to anyone, is shared with nobody, and
   whose compromise is strictly worse than a shared secret's — it mints tokens rather than
   presenting one. That statement belongs to boundary 1b, which is where it is. Test 12.

## Decisions

Twenty-five numbered decisions. Ten the milestone cannot start without, each with its reasoning and
the acceptance test that pins it; then eight smaller ones the resource-server research left
unresolved and the plan may not leave silent; then six the authorization-server half adds; then
one the 2026-10-20 addendum adds, on where the deployed server's documents come from. Four
questions are the operator's and are in **Open decisions** at the end, with a recommended default
and one reason each — **the plan below is written at all four defaults**, because a task table and
a pin table cannot be conditional, and Open decisions is what names the delta if one is overridden.

### 1. The resource-server half at `/mcp`

willikins validates access tokens at `/mcp`, publishes an RFC 9728 protected-resource-metadata
document, and answers an unauthenticated or badly authenticated request with a 401 that points
at that document. **It validates every token the same way whoever minted it** — the clause this
decision used to carry, "it issues nothing", moved to trust boundary 1a and became false there, and
what replaces it is the rule that matters: the fixed-order checks below run identically for a
willikins-minted token and a foreign one, with no shortcut for a signature the process recognises.
Decision 7's fixture is what asserts that, rather than this sentence.

**The validation set**, in the order it runs, for a JWT-format access token. RFC 9068 §4 is the
list; the MCP profile is what makes the audience check a server-side MUST:

0. The credential parses as a compact JWS at all: three base64url segments with a decodable JOSE
   header. Decision 1's remaining checks all read a parsed header, so "not a JWT" is its own
   refusal (`AuthFailedReason::Unparseable`) rather than an unnamed fall-through.
1. `typ` header is in the frozen accepted set: `at+jwt` or `application/at+jwt` (decision 14).
2. `alg` header is in the configured allowlist, which may contain only asymmetric families
   (decision 15). `none` never validates, and `jsonwebtoken` refuses a verifier whose key family
   differs from an allowed algorithm's family.
3. Signature verifies against the key whose `kid` matches the header's, from the key set
   `JwkSource` supplies (decision 16).
4. `iss` matches the derived issuer identifier — `WILLIKINS_PUBLIC_URL` exactly (decision 12) —
   as a string, by exact comparison.
5. `aud` contains this server's resource identifier, `<WILLIKINS_PUBLIC_URL>/mcp` — derived, never
   configured (decision 12) — or, while a migration is in progress, one of the identifiers listed
   in `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`, which are accepted but never published. RFC 9068 says
   "contains", so an `aud` array carrying an accepted resource among others is accepted.
6. `exp` is in the future, within `WILLIKINS_OAUTH_LEEWAY_SECONDS` of clock skew (decision 17).
   `nbf`, when present, is validated too — `jsonwebtoken`'s `validate_nbf` defaults to false and
   is overridden.
7. `exp`, `aud`, `iss` and `sub` are required claims. `jsonwebtoken`'s `required_spec_claims`
   defaults to `{"exp"}` only, so a token with no `aud` at all would otherwise pass its audience
   check; `set_required_spec_claims(&["exp", "aud", "iss", "sub"])` is what closes that. `sub` is
   on the list because decision 2 derives the principal from it and decision 3's two allowlists
   compare against it: a token with no `sub` has no identity this server can journal or authorize,
   so it is refused with `AuthFailedReason::MissingSubject` rather than given a principal derived
   from an empty string. The foreign-issuer fixture mints a `sub`-less token as one of its flaws so
   this is a test and not an assumption. The required set stays at four and not at the seven the
   issuer emits, deliberately: decision 17 says why.

Every failure in that list answers **401** with `error="invalid_token"`, which is RFC 9068 §4's
own instruction, and is journaled as `AuthFailed` with a reason that says which check failed.

**After a token validates, two more refusals run before any tool does**, and both are 403 rather
than 401 because the credential was good and the authority was not: a `sub` that is not in
`WILLIKINS_AGENT_SUBJECTS` is `AuthFailedReason::UnlistedSubject`, and a scope set that does not
admit the tool named in the body is `AuthFailedReason::InsufficientScope { needed }` with
`error="insufficient_scope"` (decision 3). The specification reserves **400** for a malformed
authorization request. Whether a non-`Bearer` `Authorization` header counts as malformed (400) or
simply absent (401) is verify item 22 — RFC 6750 §3.1 was not fetched — so today's behaviour, 401
`MissingCredential`, is kept until it is. The status table itself is the specification's, verbatim.

**Every check, and the `AuthFailedReason` it journals.** The variants are additive to
`willikins_journal::AuthFailedReason` (`crates/willikins-journal/src/event.rs`), which today holds
`MissingCredential`, `InvalidCredential`, `WrongRole`, `InvalidNonce`, `ForeignOrigin` and
`MalformedUsername`. The authorization-server half adds ten more, listed in the journal contract
below.

| Check | Status | Reason variant | State |
| --- | --- | --- | --- |
| No `Authorization` header, or a non-`Bearer` one, or a token only in the query string | 401 | `MissingCredential` | retained |
| Not a parseable compact JWS | 401 | `Unparseable` | new |
| `typ` outside the frozen set | 401 | `InvalidType` | new |
| `alg` outside the allowlist, or `none`, or a family mismatch | 401 | `InvalidAlgorithm` | new |
| No key for the `kid`, or a header with no `kid` | 401 | `UnknownKey` | new |
| Signature does not verify | 401 | `InvalidSignature` | new |
| `iss` missing or not the derived issuer identifier | 401 | `InvalidIssuer` | new |
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
whole. **The authorization server's own refusals reuse this machinery rather than inventing a
second one** (decision 20): `/authorize`, `/token` and the ceremony endpoints are all anonymously
reachable, so every refusal on them is a journal append an attacker can trigger, which is the exact
shape this paragraph already closed for `/mcp`. Pass 3 measures both halves: a flood of garbage
tokens against journal growth and approvals-page latency, and the fold cost itself.

**The document it serves.** One JSON object with `resource` (RFC 9728's only REQUIRED field),
`authorization_servers` with at least one entry — which is willikins' own issuer identifier now,
and is the MCP profile's MUST — `scopes_supported` (RECOMMENDED), `bearer_methods_supported:
["header"]`, and `resource_name`. Parameters with zero values are omitted, the response is 200
`application/json`, and the route is served outside the bearer middleware and outside rmcp's host
check exactly as `/healthz` is — an unauthenticated client must reach it before it holds a token,
and `/healthz` is the existing precedent for a root-level exemption. Decision 19 adds the second
document beside it under the same rule.

**The document is served at the path-inserted URL only, and the root is deliberately not a
route.** MCP clients that receive no `resource_metadata` pointer fall back to the path form and
then to `/.well-known/oauth-protected-resource`. Anthropic's lazy-authentication example serves
both. willikins does not, because RFC 9728 §3.3 says "The resource value returned MUST be
identical to the protected resource's resource identifier value into which the well-known URI path
suffix was inserted". A root document carrying `resource=<origin>/mcp` would break that, and one
carrying the bare origin would name a resource that does not exist. Every 401 carries the pointer
(test 2), so neither app needs the fallback. Test 19 checks that both discover through the header,
and test 1 asserts the root answers no document.

**The 401 shape.** `WWW-Authenticate: Bearer resource_metadata="<PRM URL>",
scope="willikins:read willikins:plan willikins:apply"`, plus `error="invalid_token"` when a token
was presented and rejected. The scope is **fixed**, not the scope the refused call needed, because
the token is validated before the body is read at all (decision 3). So at 401 time the server does
not know which tool was asked for.

**Why it names the three MCP scopes and not only `willikins:read`** (2026-10-20; the earlier text
named `willikins:read` alone). Both target apps take this challenge as authoritative for the
*first* token. Anthropic: "To control which scopes Claude requests, include a `scope` parameter in
the `WWW-Authenticate` header on your `401` response". MCP 2026-07-28: "Clients MUST treat the
scopes provided in the challenge as authoritative". A read-only first token leaves every `plan` and
`apply` waiting on a step-up, and that costs four things:

- Each step-up is another passkey consent, because no refresh token is issued (decision 21).
- Claude documents step-up on an HTTP 403 with a challenge.
- OpenAI documents re-linking only through per-tool `securitySchemes` plus a tool result carrying
  `_meta["mcp/www_authenticate"]` (developers.openai.com/plugins/build/auth.md: "Without both
  halves ChatGPT will not show the linking UI for that tool").
- Whether ChatGPT steps up on willikins' transport-level 403 at all is verify item 28.

The specification allows the challenged set to "match `scopes_supported`, be a subset or
superset", so naming the three is conformant. **It changes no authority.** The human still grants
scopes at the consent screen. `WILLIKINS_AGENT_SUBJECTS` still decides who may act at all
(decision 3). And the 403 of decision 3 still names the exact scope a call lacked. The rejected
alternative is omitting `scope` so that both apps fall back to `scopes_supported`. It reaches the
same set, but only through each client's fallback rule rather than a value the server states.
The specification's MUST is to implement *one of* the header or the well-known URI; clients prefer
the header, so willikins does both.

**The passthrough prohibition, stated for a server that itself calls providers, and unchanged by
this revision.** The specification says it twice: an MCP server must not pass through the token it
received, and must not accept a token not issued for it. willikins satisfies the first
structurally already, because no caller token has ever reached an upstream API: every outgoing
credential is an operator credential or a document-bound secret, read only at the sites trust
boundary 2 lists. This milestone's job
is to keep it that way while a real OAuth token is in the request: the inbound token is consumed by
the middleware, the header is stripped before rmcp sees it, and nothing downstream can read it.
That willikins now mints the token changes nothing here, and the second half — not accepting a
token not issued for it — is check 5, which is why a token minted for another resource is a pass-3
row rather than an assumption.

*Pinned by tests 1, 2, 3 and 6.*

### 2. Principal identity

Today a principal is `agent-<12 hex of the token hash>`. After 2c it is
`oauth-<12 hex of sha256(iss ‖ 0x00 ‖ sub)>`, and the claims it was derived from are recorded
beside it. **This decision is unchanged by the revision**, and its `iss` fold earns a second
reason: it is what stops an adapter's subjects ever colliding with willikins' own enrollment
identifiers.

**Why not the subject itself.** `PrincipalId`'s grammar is
`^[A-Za-z0-9][A-Za-z0-9._@-]{0,127}$`, declared as `PRINCIPAL_ID_PATTERN` at
`crates/willikins-core/src/apply/principal.rs:13` and re-exported through `willikins_journal`.
A raw `sub` need not fit it — a provider-qualified subject carrying a `|` is a common shape — and
a derived, in-grammar, deterministic id keeps the existing discipline (the same identity always
derives the same principal, distinct identities never collide) and keeps externally supplied text
out of an identifier that is compared, indexed and printed. The `iss` is folded in because a `sub`
is only unique within its issuer. **One note for the fold, now that willikins issues:** the
in-house `sub` is a UUID willikins generates at enrollment and would fit the grammar directly. The
derivation stays anyway, because it must keep working for a foreign `sub` that does not, and
because a principal whose shape depended on who issued the token would be a second code path where
the seam says there is one.

**What the journal records, and how the claims get there.** The claims themselves, as new
**optional** fields on every event that already carries a principal: `subject`, `issuer`,
`client_id`. Since 3m those events are, by name, `ToolCalled`, `PlanRecorded`, `ApprovalGranted`
and `ApprovalRejected` (whose field is `approver`), `ApplyRefused`, `RunStarted`, `TeardownPlanned`
and `TeardownStarted` (`crates/willikins-journal/src/event.rs`). They are authorization-server-supplied text — willikins' own or an adapter's — so each
is bounded and escaped the way `willikins_types::quoted` bounds a rejected literal before it is
written, and each is omitted when the claim is absent. `client_id` in particular must tolerate
absence: willikins emits it on every token it mints (decision 22), and RFC 9068 §2.2's claim list
was not fetched, so "a JWT access token always carries `client_id`" is not a fact this plan may
rely on for a token it did not mint.

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
is **eleven** methods in `crates/willikins-server/src/butler.rs`: `list_workflows`, `validate`,
`describe`, `list_tools`, `propose_slug`, `plan`, `approve`, `reject` and `apply`, plus 3m's
`plan_rollback` (`:1294`) and `apply_rollback` (`:1503`), which journal `ToolCalled` as
`rollback_plan` and `rollback_apply`. `record_auth_failure` (`:1215`) also changes: it gains the
parameter it does not have today, because it takes only a `Transport` and an `AuthFailedReason`.
(The 2026-09-16 text said "ten" and named nine; it predates 3m.) `claims` is `None` for every
caller that is not an OAuth caller: `willikins-cli`'s `--principal`, including its `rollback`
commands, `serve --stdio`'s local principal, and the existing test helpers. Task 6 lists every
changed signature: the eleven `Butler` methods, the eight `#[tool]`
methods in `crates/willikins-server/src/mcp.rs` and `principal_for` beside them, the approve and
reject handlers in `crates/willikins-server/src/http/approvals.rs`, `willikins-cli`'s commands in
`crates/willikins-cli/src/commands.rs` (the rollback paths included), and the shared helpers under
`crates/willikins-server/tests/common/`, which `tests/it/main.rs` mounts as `mod common`.

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
and pass 3 freezes `post-2c-new-shapes.jsonl` as milestone 3's baseline. The pre-2c fixture holds
**every `Event` variant the current binary writes**, including 3m's `TeardownPlanned`,
`TeardownStarted`, `TeardownStepFinished` and `TeardownFinished`. Otherwise the replay guarantee
would not cover them. Both existing fixtures, `pre-pass-2-every-event.jsonl` and pass 2's own
`post-pass-2-new-shapes.jsonl` baseline, keep replaying.

**The new identity values are plain domain types, not identifiers, and the plan says why**
(2026-10-20, reconciling 3i). Since 3i, every `#[domain(identifier)]` type prints as a short
prefix on every output surface, the approvals page included, and only the CLI's `--reveal` prints
one whole (`CLAUDE.md`; `approvals.rs`'s decision b7). 2c adds surfaces that show identity:

- the `WrongRole` page's `sub` and `iss`;
- the consent screen's client display name and redirect host;
- the 401 and 403 bodies;
- the journal's `subject`, `issuer` and `client_id`;
- the enrollment CLI's printed subject.

The subject (`OAuthSubject`) and the client id (`ClientId`) are **plain, non-identifier domain
types**, because the operator must copy them whole into `WILLIKINS_*_SUBJECTS` and the client
table. A prefix would make both bootstrap paths useless. They name a willikins-local account or
client, not a provider's record, so the reason identifiers are masked does not apply. The issuer
is the public URL. What does not change: every plan and teardown rendering on the rebuilt page
keeps `mask_json` with no reveal, and the journal keeps full values (decision b4).

*Pinned by test 8.*

### 3. Scopes

Four server-defined scopes, in a hierarchy, mapped onto the eight MCP tools. The specification
defines only the machinery — `scopes_supported`, the `scope` challenge parameter, and a client
selection rule — names no registry and no reserved scope, and calls a wildcard or omnibus scope
a common mistake. **Every conclusion in this decision survives the revision; one paragraph's
premise does not, and is replaced below rather than quietly kept.**

| Scope | Tools it admits |
| --- | --- |
| `willikins:read` | `validate`, `describe`, `list_workflows`, `list_tools`, `propose_slug`, `run_status` |
| `willikins:plan` | the above, plus `plan` |
| `willikins:apply` | the above, plus `apply` |
| `willikins:approve` | no MCP tool; the approvals page only |

**Two rows are assigned now for tools that do not exist yet** (2026-10-20). They are the rollback
tools of `todos/2026-10-16-rollback-over-mcp.md`: `rollback_plan` goes under `willikins:plan`, and
the teardown apply under `willikins:apply`. `run_status` for a teardown run stays under
`willikins:read`. `rollback_plan` belongs with `plan` because `Butler::plan_rollback` re-observes
provider state when it computes a teardown (`TeardownAction::Gone` is an observed state), so it
spends provider budget the way `plan` does. The teardown apply deletes, so it is an apply. Neither
needs a scope of its own: every teardown already needs a human's approval, and the page's
approver role is the gate for that.

**The table is tied to the tool list by a test, not by memory.** An unknown tool name is refused
403 (step 4 below). That fails closed, but it also means a new `#[tool]` would be silently 403'd in
production until someone remembered this table. So test 7 asserts that every tool in the
snapshotted tool list
(`crates/willikins-server/tests/it/snapshots/it__mcp_server__the_tool_list_and_every_schema_is_snapshotted.snap`)
has exactly one row in `oauth::scope`'s table, and that the table names no tool the list lacks. A
new tool without a scope row then fails the gate.

`plan` is separated from the read tools because it is the only read-side tool that calls live
providers and spends the organization's provider budget (GitHub, Doppler, Buildkite, SigNoz, App
Store Connect). That is why it already has its own rate-limit bucket. `apply` is separated
because it is the only tool that writes.
`willikins:approve` grants no tool at all: it is the browser-side intent, and decision 4 says
where the authority behind it lives. **The same table feeds the consent screen** of decision 20, so
what a human is asked to grant and what the middleware admits cannot drift apart.

**Which claim carries the scopes.** willikins **emits** the RFC 6749 `scope` claim, a single
space-separated string. It **accepts** both that and an `scp` array of strings, and it keeps
accepting both even though it now knows what its own issuer emits, because an adapter's tokens land
in this same validator and the two shapes are both in the wild. A token carrying neither is treated
as carrying the empty set and is refused on every tool, never as carrying everything.

**A scope is not authority. The subject allowlists are.** The delegating draft founded this on the
provider: decision 10 *required* an authorization server with an open registration path, so the set
of clients that may *ask* for `willikins:apply` was open by design. **That premise is false now** —
willikins pre-registers its own clients (decision 23) — and the conclusion survives on different
ground, which the plan states rather than leaving a reader to reconstruct. A scope is what a client
*asked for* and a consent screen *granted*. The human who clicks consent is not necessarily the
human a deployment authorises to apply, the consent screen is reachable by any registered client,
and a deployment whose only gate is "the token says `willikins:apply`" has delegated its apply
authority to whoever last clicked a button. Separating "what was granted" from "who may act" is the
point, and it is a property of consent, not of a provider's registration policy.

So: a required variable **`WILLIKINS_AGENT_SUBJECTS`**, comma-separated `sub` values allowed to
hold any MCP scope. It is required in http mode; empty or unset is a startup refusal
(`HttpConfigError::EmptyAllowlist`), never a permissive default, so a deployment cannot become open
by omission. A token that passes every check in decision 1 but whose `sub` is not listed answers
**403** with `AuthFailedReason::UnlistedSubject`, and **no tool runs** — the check happens in the
middleware, before rmcp sees the request at all. Only within that allowlist do the token's scopes
decide read, plan or apply.

The result is that "who may call `apply`" is legible in the deployment's own configuration as
`WILLIKINS_AGENT_SUBJECTS` ∩ the tokens carrying `willikins:apply`, rather than in a console an
auditor may not have. `WILLIKINS_APPROVER_SUBJECTS` keeps exactly the same role for approval, and a
subject may legitimately be in both lists: the same human may run an agent and approve its plans,
which is the flow milestone 2 built for.

**Entries are bare `sub` values, not issuer-qualified ones**, and that argument is **stronger**
after the revision, not weaker: a deployment has exactly one issuer and it is now its own, so a
listed `sub` is unambiguous by construction. **Where the values come from changed.** They are
willikins-generated enrollment identifiers, and the first path to them is the enrollment CLI, which
prints the subject when it enrols a credential (decision 24, go-live step 4). The "log in once and
read your own subject off the `WrongRole` page" bootstrap survives as the **second** path, for
anyone who enrolled without noting it down. Two consequences are recorded rather than designed
around. A move to a different issuer — which on the in-house path means a host move, and on the
adapter path means adopting somebody else's provider — rewrites **both** lists, because every
subject is reissued; that is a go-live step, not a silent breakage. And if a deployment ever needs
to trust two issuers at once, the answer is issuer-qualified entries and a multi-issuer
configuration block, which is milestone 3's work.

**Where the check runs, and how the challenge knows what to name.** The tool name lives in the
JSON-RPC body, which rmcp parses *after* the middleware, and rmcp exposes no way for a handler to
set an authorization status: `jsonrpc_http_status` in
`rmcp-3.3.0/src/transport/streamable_http_server/tower.rs:626` maps a handler's error to exactly
**400, 404 or 200** and nothing else, so 401 and 403 are unreachable from inside a handler at the
version the lock holds. The scope check therefore runs **in the middleware**, in this order:

1. **Validate the token first, before reading one byte of the body.** An unauthenticated request
   answers 401 carrying the fixed `scope="willikins:read willikins:plan willikins:apply"`
   (decision 1 says why it names the three MCP scopes rather than a minimal read scope). That is
   legal, because the specification says the challenged scope set MAY be a subset, a superset or
   neither. A challenge that named the exact scope of the refused call would require parsing an
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
   middleware from having to know rmcp's tool table to stay closed. **A conditional, for a client
   speaking 2026-07-28:** that revision's transport makes a server that processes the body check
   that the `Mcp-Method` and `Mcp-Name` headers match it. This middleware processes the body. If
   test 19 finds either app negotiating 2026-07-28 against rmcp 3.3.0's sessionless server (verify
   item 34), the middleware also compares `Mcp-Name` with `params.name` and refuses a mismatch with
   400. Until then nothing reads either header.
5. **`GET` and `DELETE` at `/mcp` still require a token**, and then meet rmcp's own **405** with
   `Allow: POST` (`tower.rs:1502`, reached because `legacy_session_mode` is false), which is a
   transport answer this milestone does not change.

*Verify item 21 is settled for rmcp 3.3.0* by the `jsonrpc_http_status` reading above: there is no
handler-set status and the check stays in the middleware. It stays open only for 3.4.0, and only as
an opportunity — nothing here waits on it.

The hierarchy is a resource-server obligation at 2026-07-28 — "Servers **MUST** account for
scope hierarchies, where a broader scope implies narrower ones, when deciding whether a token is
sufficient for an operation" — so a token carrying only `willikins:apply` satisfies a `describe`
call. **The `/mcp` document's `scopes_supported` lists the three MCP scopes** —
`willikins:read`, `willikins:plan` and `willikins:apply` — and never `offline_access`, which that
revision tells protected resources not to advertise and which decision 21 does not issue anyway.
(Until 2026-10-20 it also listed `willikins:approve`.) MCP describes the list as "the minimal set
of scopes necessary for basic functionality", and Claude requests all of it when a challenge
carries no `scope`. `willikins:approve` admits no MCP tool, so advertising it on the resource it
cannot be used at only invites clients to ask for it. **It is never granted at `/authorize`**:
the consent screen offers the three MCP scopes, and a request naming `willikins:approve` is
granted the remaining MCP scopes it asked for, with the token response's `scope` stating what was
granted (RFC 6749 lets a server issue less than was requested). `willikins:approve` is minted
only by the ceremony hop of decision 4. The RFC 8414 document publishes the same three
(decision 19). The 403's
`scope` attribute names only what the refused call needed, which is 2026-07-28's rule and the
reverse of 2025-11-25's; naming less is safe under both.

**Why an allowlist and not a claim or a group.** A role or group claim is a per-issuer invention:
willikins would have to define one for itself and then map every adapter's onto it, which is the
one piece of real translation work the pluggable-auth todo already assigns to an adapter — so
building the authorisation model on it would put that translation on the critical path of every
request instead. Two comma-separated variables cost one deployment step each and are readable by
anyone with access to the deployment's configuration.

*Pinned by tests 7 and 11.*

### 4. The approvals page, and the session behind it

Basic auth is removed. **The approvals page stops being an OAuth client of anyone** — not of an
external provider, and, the part that is easy to get wrong, **not of willikins' own `/authorize`
either**. Running a browser through a redirect loop back to the same process, to obtain a token the
same process could mint directly, is machinery with no security gain. The flow is:

> a passkey ceremony completes → the issuer mints an access token for `<WILLIKINS_PUBLIC_URL>/mcp`
> carrying the enrolled subject and `willikins:approve` → **`oauth::validate` accepts it on exactly
> the same path `/mcp` uses** → the session records the subject, issuer and scopes that validation
> returned.

That last hop is not ceremony; it is the seam. The delegating draft's own words survive verbatim
and are now the seam's load-bearing rule: **the same function and the same configuration the
`/mcp` middleware uses, one validation path and not two**. A consequence worth stating so a reader
does not think a check is dead: because willikins' own ceremony always mints `willikins:approve`,
the "listed subject whose token lacked the approve scope" case of test 11 is reachable **only**
through the foreign-issuer fixture. The check stays, because it is what an adapter's token will
meet.

**What this deletes**, listed so nobody looks for it later: the 302 to an authorization endpoint,
`/approvals/callback`, the `state` value and its store semantics, the `code_verifier`, the
`iss`-parameter check on the callback, the token exchange with its timeout and byte cap, the
refresh and id tokens dropped unread, `client_secret_basic`, `Credential::authorize_basic` and its
`clippy.toml` and `expose_secret_guard` exemption, the "willikins is a confidential client"
argument and its rejected counter-argument, and every `WILLIKINS_OAUTH_CLIENT_*`,
`_AUTHORIZE_URL` and `_TOKEN_URL` variable.

**`/approvals` never inspects `Authorization`, at all, in any code path.** One rule, and it is the
whole of trust boundary 3's browser half: a request without a valid session gets the same response
whatever headers it carries. A `GET` is 302 to `/approvals/login` with a bearer header, with a
Basic header, and with neither. A `POST` is **403** in all three cases. There is no
`WWW-Authenticate` on any of them, and in particular no `WWW-Authenticate: Basic`, which would
re-summon the browser password prompt this milestone exists to delete. Tests 9 and 10 assert the
responses are byte-identical with and without the headers, because "identical" is the property, and
a surface that answers differently for a bearer token is a surface that reads one.

**The ceremony store stays, and is re-founded.** The delegating draft's pending-login store held
OAuth state, and one reading of the revision would delete it along with the OAuth flow. It stays,
for a fetched reason: `webauthn-rs` requires the in-progress ceremony state to be **server-side**
and refuses to derive serde on it by default, precisely so it cannot be put in a cookie and
replayed. So the store keeps its shape — 300 s TTL, which is also the crate's own
`DEFAULT_AUTHENTICATOR_TIMEOUT`, pruned on insert, capped at **1,024** entries, answering **503**
when full rather than evicting a login someone is in the middle of.

**The `__Host-willikins-login` cookie stays too, re-founded as the ceremony binding.** It is short
lived (`Secure`, `HttpOnly`, `SameSite=Lax`, 300 s), it holds the ceremony id, and the finish step
requires it to be present and to match the record; a mismatch or an absent cookie is **400**. The
attack it closes is unchanged in shape and is now a published CVE's shape as well: **both** parties
can be listed in `WILLIKINS_APPROVER_SUBJECTS`, so an approver-attacker starts a ceremony,
withholds their own finish step, and gets a second approver's browser to complete it; without the
binding the victim's browser holds a session minted from the *attacker's* ceremony, and every
decision the victim makes is journaled under the attacker's `sub` — an audit trail that names the
wrong human, which is worse than a refusal. Decision 24's separate rule, that each ceremony is
bound to the account it finishes against, is the same class of defect from the other side.

**The session cookie**: name `__Host-willikins-session`, `Secure`, `HttpOnly`, `SameSite=Lax`,
`Path=/`, no `Domain`, and **no `Max-Age` and no `Expires`** — a non-persistent cookie, which OWASP
names and the delegating draft did not. Signed through `axum-extra`'s `SignedCookieJar` and
carrying an opaque **128-bit** session id from a CSPRNG; the session record itself (subject,
issuer, granted scopes, expiry, and the credential hash of the revocation rule below) is
server-side and in memory, capped at **1,024** with the same 503. The signing `Key` is generated at
startup and never configured: a session surviving a redeploy is not wanted, so there is nothing to
keep stable, and one fewer secret is one fewer secret — with the single-replica consequence in
Risks, because a startup-generated key makes a session minted on one replica a *forgery* on
another.

**`SameSite=Lax`, and it is now a seam constraint rather than a cookie detail.** The delegating
draft needed `Lax` because the OAuth callback landed through a cross-site redirect chain that
`Strict` withholds cookies from. That callback is gone, so the obvious move is `Strict` — and the
plan does **not** take it, for two reasons. First, the human session acquired a **second consumer**:
`/authorize`'s consent page needs it, and `/authorize` is reached by a top-level navigation an MCP
client launched from outside. **Whether `Strict` withholds a cookie on an externally-initiated
top-level navigation is a browser behaviour nobody fetched** (verify item 7), so flipping now would
be guessing at the one thing that would break the consent screen. Second, an adapter's cross-site
callback landing would need `Lax` again, which makes this a decision to take once and write down.
Default `Lax`; measure before changing. The corollary is stated rather than assumed: `Lax` also
withholds cookies on a cross-site **POST**, so a forged approval form submitted from another site
arrives with no session — but see the nonce rule below, which is stronger than the delegating draft
allowed.

**The per-plan nonce is load-bearing, not defence in depth**, and it is now **bound to the
session**. The delegating draft demoted it to defence in depth on the strength of `SameSite`. That
understates the risk: `SameSite` is scoped to the **registrable domain**, so every sibling host
under `example.com` is same-site to `willikins.example.com` and a bug on any of them is
inside the fence. OWASP's rule is to bind the token explicitly to session-specific data, and that
it must not be leaked in server logs or in a URL — so the nonce is bound to the session it was
issued to, a nonce from another session is `InvalidNonce`, and it is written to neither the journal
nor a log line.

**Five session properties are newly required because willikins now runs the login.** Each is
OWASP-cited, each is a few lines now and a security-review finding later, and the first cannot be
bolted on after a session shape has been frozen in tests.

1. **Unconditional session-id rotation at login**, destroying the previous id. Session fixation is
   a *new* surface: under delegated OAuth the session was born fresh at the callback, and a login
   that sets any pre-authentication cookie creates the classic target. OWASP requires the id to be
   renewed after any privilege-level change, and a guarded "rotate only if" is the shape **not** to
   copy.
2. **An idle timeout beside the absolute one**, in its own variable, and bumped on **writes** and
   not on reads. `WILLIKINS_SESSION_TTL_SECONDS` is absolute only, so an unattended browser could
   approve for a full hour. OWASP requires both and names 2 to 5 minutes as the idle range for
   high-value applications — which a page whose one action is approving a plan that provisions
   secrets is by its own framing. Bumping on reads is a mistake a shipped crate makes.
3. **`Cache-Control: no-store` on every response that carries a session id.** Unlike `no-cache`,
   which permits caching with revalidation, `no-store` keeps the response — `Set-Cookie` headers
   included — out of every cache, which is also the concrete answer to the edge-caching gap the
   earlier research flagged.
4. **128-bit ids from a CSPRNG and a non-persistent cookie**, both named above.
5. **Credential-change revocation.** The session is bound to a hash of the credential as of login
   and the two are compared in constant time on every request, flushing the session on mismatch.
   **This is the revocation the delegating draft's Risks section admitted it did not have**:
   changing or removing a credential now kills every live session for that account, without a
   redeploy.

**`Sec-Fetch-Site`, and the origin check.** OWASP now lists Fetch Metadata as a first-class CSRF
defence with a **mandatory** fallback to origin verification, which willikins already has. So it is
purely additive: a non-safe method arriving with `Sec-Fetch-Site: cross-site` is refused, and
`Vary` names `Sec-Fetch-Site, Origin`. **The two "fixes" this paragraph used to list are already
how the code behaves** (corrected 2026-10-20; both have been in place since 0e329eb, 2026-09-15).
`extract_authority` splits on `/`, `?` and `#`, and `host_matches`
(`crates/willikins-server/src/http/approvals.rs:276`) compares exactly and case-insensitively, so
there is no prefix comparison for `willikins.example.com.attacker.com` to pass. And
`origin_allowed` (`:289`) already refuses when both `Origin` and `Referer` are absent. Both become
pinning tests in task 8 and test 14, not changes. The one real defect is task 13's:
`rsplit_once(':')` on a bracketed literal with no port turns `[::1]` into `[:`.

**Everything else milestone 2 built on the page stays.** The body cap, the IPv6-aware origin parser
(task 13), `X-Frame-Options: DENY`, and the CSP — which **grows a `script-src` with a per-response
nonce**, because decision 24 puts the first `<script>` this page has ever carried on it — alongside
`frame-ancestors 'none'` and `form-action 'self'`, the second so that a markup-injection bug cannot
retarget the approve form at another origin. Every refusal is journaled with the
`AuthFailedReason` that is literally true of it.

**The consent screen's CSP is stated separately, because `form-action 'self'` can break the hand-back
to the client** (2026-10-20). The consent screen sits behind the same session (decision 20), and
approving it is a form POST answered by a redirect to the client's registered callback, for
example `https://claude.ai/api/mcp/auth_callback`. MDN, on `form-action`: "Whether `form-action`
should block redirects after a form submission is debated and browser implementations of this
aspect are inconsistent (e.g., Firefox 57 doesn't block the redirects whereas Chrome 63 does)."
Under the approvals page's policy, a Chrome user's consent would never reach Claude or ChatGPT. So
the consent response's CSP is `form-action 'self' <origin>`, where `<origin>` is the exact origin
of the **validated, registered** redirect URI of the pending request: the scheme, host and port
of a loopback redirect as the request gave them, never a wildcard. Only the consent page widens
this; the approvals page keeps `form-action 'self'`. A server-chosen, already-validated origin
adds nothing a markup-injection bug could retarget. The rejected alternative is ending the consent
POST on a same-origin page that navigates onward with script. It needs a second `<script>` and
gains nothing over naming the one origin. Whether Chrome and iOS WebKit then follow the redirect is
measured by test 22's browser check (verify item 35), not assumed.

**Logging out.** `POST /approvals/logout` drops the session record and clears the cookie.

**Pending teardowns are on the rebuilt page too** (2026-10-20, 3m). `Butler::decide`
(`crates/willikins-server/src/butler.rs:748`) already accepts either kind of id: it tries
`journal.plan(...)` and falls back to `journal.teardown(...)`, because the two share one `PlanId`
namespace and each id is minted for exactly one kind. The page does not. `pending_approvals_view`
(`crates/willikins-journal/src/journal.rs:295`) folds forward plans only, and `get_approvals`
renders, and issues nonces for, `PlanRecord`s only. So a teardown waiting for a human is invisible
there. The CLI says as much: "run `willikins approve`" (`crates/willikins-cli/src/render.rs:1141`).
On Railway the journal is on the volume and the container has no shell, so that path does not
exist. **Decided: the page lists pending teardowns** (task 24):

- A `pending_teardowns` view beside `pending_approvals_view`.
- A teardown-shaped section: the steps in order, through `mask_json` like every other value on the
  page.
- Session-bound nonces issued for teardown `plan_id`s by the same mechanism.
- The same `WrongRole` and approver rules.

A nonce is minted for one `plan_id`, and ids are never shared between kinds, so a forward plan's
nonce cannot decide a teardown, nor the reverse; test 31 asserts both directions anyway. The
rejected alternative was deferring the view to the todo and stating that no deployed teardown
could be approved until it landed. It was rejected because 2c rebuilds this page, and building it
twice costs more than one section. What stays in `todos/2026-10-16-rollback-over-mcp.md` is the
producer: an agent-callable `rollback_plan` (its scope row is fixed in decision 3) and teardown
`run_status`. Until that lands, a teardown reaches the deployment's journal only through a
`Butler::plan_rollback` caller, which in 2c means the tests (open question 4).

**The `WrongRole` page tells the operator their own identity.** A session whose `sub` is not in
`WILLIKINS_APPROVER_SUBJECTS` gets a 403 page that displays that session's own `sub` and `iss`,
escaped the way every other authorization-server-supplied string on that page is. It is the
**second** bootstrap path now — the enrollment CLI prints the subject first (decision 24) — and it
is still worth having, because the first path is a command someone may have run without reading its
output.

**Lifetime.** The session's absolute lifetime is `WILLIKINS_SESSION_TTL_SECONDS`, default 3600, and
it is **capped below the approval window** (default 86400): a session must expire well inside the
window a plan can wait in, so that a plan pending overnight cannot be approved by a browser nobody
has re-authenticated in front of since. The idle lifetime is
`WILLIKINS_SESSION_IDLE_TIMEOUT_SECONDS`, default 300, and is capped the same way. Sessions,
ceremonies and authorization codes are in memory and are therefore lost on a redeploy, exactly as
the nonces already are; the README gains a line saying so, next to the existing recovery notes.

*Pinned by tests 9, 10, 11, 26 and 31.*

### 5. Which listeners require OAuth

- `serve --stdio` keeps the local principal (`--principal`, default `local`) and gains nothing.
  The specification is explicit: implementations using a STDIO transport **SHOULD NOT** follow
  the authorization specification and should take credentials from the environment
  (`…-m2c-authorization.md` §1.2). Whoever runs it holds the machine that holds the GitHub and
  Doppler credentials.
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

**The cost, and how this revision improves it.** The delegating draft had to say plainly that after
2c there is no dev-only mode: driving the HTTP transport by hand needed a real token from a real
provider, because the fake authorization server is test-support code that starts inside a
`cargo test` process. Now that willikins issues its own tokens, a hand-run `serve --http` on
loopback can **enroll a passkey and log in against itself**, with no external provider and no
pasted token — the whole flow, in one process, on a developer's machine. That is stated as an
improvement **conditional** on one unresolved question and not as a fact: whether `__Host-`prefixed
cookies are honoured on `http://localhost` was not settled by any fetched source (verify item 9),
and without them the session half of that loop does not run. The conditional half is the session;
discovery, `/authorize`, `/token` and a `list_tools` call work on loopback either way, which is
what test 18's harness drives.

What has not changed is the refusal that matters: a `--dev-token` flag, an "insecure mode", or a
fake issuer the shipped binary can start are all a **second authentication path**, which is the
thing this decision exists to prevent. There is one issuer and it is the real one.

*Pinned by test 12.*

### 6. What happens to `hash-token` and the two hash variables

**Removed, and refused at startup on every bind** — not kept for loopback, not ignored.

`hash-token` is deleted from both binaries, along with its tests, and the README's section on it
goes with it. `WILLIKINS_AGENT_TOKEN_HASHES` and `WILLIKINS_APPROVER_TOKEN_HASH` become *retired*
variable names: if either is set, `serve --http` refuses to start with
`HttpConfigError::RetiredVariable { name }`, naming the variable and pointing at what replaces it.

**Which error type the new refusals live on.** `StartupError`
(`crates/willikins-server/src/startup.rs:45`) has **eight** variants, and every one of them is about
the **trusted workflow directory** or the `ServerStarted` write: `Directory`, `Symlink`,
`InvalidName`, `NameMismatch`, `Document`, 2b's `Compose`, `Check` and `Journal`. None is a
configuration refusal, so this milestone adds none to it. Decision 25's refusal of a symlinked
workflows directory *is* about the trusted directory, so it belongs here, and it reuses
`Symlink` rather than adding a variant. Configuration refusals go where the existing ones are:
`HttpConfigError` (`crates/willikins-server/src/http/config.rs:212`, today `NoAgentHash`,
`ApproverAmongAgentHashes`, `EmptyAllowedHosts`) gains **`RetiredVariable`**,
**`SymmetricAlgorithm`**, **`CannotVerifyOwnTokens`**, **`SessionOutlivesApprovalWindow`**,
**`InsecureUrl`**, **`NonCanonicalUrl`** (decision 12) and **`EmptyAllowlist`**; a required variable that is simply unset keeps reusing
`ConfigError::Missing { variable }` (`crates/willikins-server/src/config.rs:33`), which is the
shape `build_http_config` already reaches for.

**Two refusals cannot live on either, for the same reason.** `HttpConfigError` is `Copy` and is
produced by the pure `HttpConfig::build`, which runs *before* the tokio runtime exists and before
anything has touched the filesystem (see the ordering note under the task table). So the refusals
that need the runtime or the disk go on `cli.rs`'s own `StartError`, beside `Bind` and
`NoBindOrPort`: **no signing key is configured and none can be generated or written**, and **the
credential store cannot be read or written**. The delegating draft put `JwksUnavailable` there for
exactly this reason; decision 16 removes the startup JWKS fetch, so that variant has no producer
and is not added — the reasoning outlived the variant.

The reasoning for retiring loudly is the one `WILLIKINS_FAKE_CATALOG` already established in this
codebase: a variable that used to decide how the server authenticates must never quietly become a
no-op. An operator who upgrades the image and keeps the old variables set would otherwise have a
service that looks configured and is in fact open to anyone it will issue a token to, which is the
opposite of what those variables meant. Refusing is loud, happens before the listener binds, and
costs one deployment step in the go-live sequence (step 6: the variable change lands *before* the
deploy that needs it, never after).

**No commit on `main` may hold the no-op state this decision forbids**, which is a sequencing rule
on the tasks and not only on the shipped binary. So each half of the retirement lands in the same
task that removes the machinery it fed: task 4 removes `HttpConfig::build`'s agent-hash rules and
retires `WILLIKINS_AGENT_TOKEN_HASHES` in the same commit series that lands the OAuth middleware;
task 9 removes `basic_auth`, `TokenHash`, `matches_any` and the constant-time compare and retires
`WILLIKINS_APPROVER_TOKEN_HASH` in the same commit series that lands the passkey login. The rule
has a second half the revision makes sharper: **no commit may hold two gates on `/approvals`
either**, which is why the session core lands as library code in task 8 with no `/approvals`-level
assertion, and every assertion about that surface belongs to task 9, where the gate swaps.

*Pinned by test 12, whose last case is that `hash-token` no longer exists on either binary.*

### 7. The foreign-issuer fixture, and adversarial pass 3

The delegating draft needed an in-process fake authorization server because no external provider
could be asked for a token with a chosen flaw. willikins now mints its own, so the obvious move is
to delete the fixture. **That would be the expensive mistake.** What the fixture becomes is the
thing that proves the resource-server half still validates, on the identical path, a token it did
not mint — seam rule 2 asserted by a test instead of by prose, and the single cheapest insurance
the milestone buys.

So it shrinks and is repurposed:

- **It loses `/authorize` and `/token`.** Faking willikins' own endpoints tests nothing: the real
  ones are driven directly, by tasks 11 and 18. The wrong-`iss` authorization response goes with
  them, because the callback it used to attack no longer exists.
- **It loses its HTTP JWKS route and the hanging-JWKS mode.** Decision 16 removes the fetch, so a
  route with no consumer is fixture surface with no consumer — which is the same reasoning that
  removed the fake's metadata documents in the 2026-09-16 review (resolution 31). The fixture
  supplies its key set through the same `JwkSource` boundary the in-house issuer does.
- **It loses its blocking `start()` too**, for the same reason and one more. That entry point
  existed so a synchronous test spawning the real binary could point it at a fixture already
  serving a JWKS from its own runtime thread; with no HTTP route to serve and no
  `WILLIKINS_OAUTH_JWKS_URI` for a spawned binary to point at, it has no consumer either. The
  consequence is worth stating rather than discovering: **tests that spawn the binary exercise
  willikins' own issuer only**, and the foreign-issuer assertion is made in process, against
  `oauth::validate` and against a router the test builds, which is where test 3 already lives.
- **It keeps everything with a consumer:** the fixed test key pair, a key set carrying a `kid`,
  `mint(flaws)` with its whole flaw list, key rotation, and the shared environment-block helper —
  because every test that configures a server sets the same block, and a hand-written block in
  twelve files is twelve places to forget `WILLIKINS_AGENT_SUBJECTS`.
- **`mint(flaws)`** returns a signed token with any combination of: wrong `aud`, wrong `iss`,
  `exp` in the past, `exp` inside the leeway, `alg: none`, a symmetric `alg`, an `alg` outside
  the allowlist, a `kid` that is not in the key set, no `kid`, a `typ` other than `at+jwt`, missing
  `aud`, missing `iss`, missing `sub`, a scope set carried as `scope` or as `scp`, no scope claim
  at all, an `aud` array containing this resource among others, a `sub` that is in neither
  allowlist, and a `sub` listed as an agent but not as an approver. **Two flaws are new and exist
  because willikins now signs:** a token signed with **willikins' own key** carrying a foreign
  `iss`, and a token carrying the **configured `iss`** signed with a foreign key. Neither may be
  waved through on the strength of recognising half of itself.

**The harness gains a soft authenticator.** `webauthn-authenticator-rs` 0.5.5 enters as a
dev-dependency — same repository, same MSRV 1.88 — and its `SoftPasskey` is what makes the
ceremony, the user-verification lie (`new(falsify_uv)`) and the counter regression testable with no
browser at all.

**The fixture is plain HTTP on loopback, and the configuration rule that permits that survives —
with a new subject.** In the delegating draft the rule governed four provider URLs. There are no
provider URLs now; what remains is `WILLIKINS_PUBLIC_URL`, which is simultaneously the issuer
identifier, the audience's stem and the RP ID's source, and which a hand-run loopback server or a
test must be able to set to `http://127.0.0.1:<port>`. So the rule is: **`WILLIKINS_PUBLIC_URL`
must be `https://`, unless its host is a loopback literal — `127.0.0.1`, `[::1]` or `localhost` —
in which case `http://` is accepted and startup logs a named warning saying so.** A non-loopback
`http://` value is refused with `HttpConfigError::InsecureUrl`. This is permanent and is **not**
gated on a test build: a `cfg(test)` gate would mean the shipped binary runs a rule no test
exercises, which is the shape that rots. Both halves are pinned by test 12.

**Pass 3's attacks**, each with the status and reason the test asserts. The validating half's rows
are the delegating draft's, minus the callback rows that lost their subject; the issuing half's are
new, and pass 3 must attack them at least as hard.

| Attack | Expected |
| --- | --- |
| Wrong audience | 401, `error="invalid_token"`, `AuthFailedReason::InvalidAudience` |
| Wrong issuer | 401, `invalid_token`, `AuthFailedReason::InvalidIssuer` |
| Expired beyond the leeway | 401, `invalid_token`, `AuthFailedReason::TokenExpired` |
| `alg: none`, and a symmetric `alg` signed with a guessed secret | 401, `invalid_token`, `AuthFailedReason::InvalidAlgorithm`; the symmetric case also fails startup validation if it is ever configured |
| A key rotated out of the key set | 401, `invalid_token`, `AuthFailedReason::UnknownKey` |
| A token minted for another resource (valid signature, valid issuer, other `aud`) | 401, `invalid_token`, `InvalidAudience` — the token-passthrough MUST, from the receiving side |
| A token for a **previous** audience, with `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES` listing it and then not listing it | **200** while listed, 401 `InvalidAudience` the moment it is not; and the metadata documents and every challenge name only the current identifier in both cases (decision 12) |
| A token signed with **willikins' own key** carrying a foreign `iss` | 401, `invalid_token`, `InvalidIssuer` — recognising the signature is not recognising the issuer |
| A token carrying the **configured `iss`** signed with a foreign key | 401, `invalid_token`, `UnknownKey` or `InvalidSignature` — recognising the issuer is not recognising the signature |
| A valid token whose `sub` is in neither allowlist, at `/mcp` | **403**, `AuthFailedReason::UnlistedSubject`, and no tool runs — asserted by the journal carrying no `ToolCalled` for it |
| A token with no `sub` claim | 401, `invalid_token`, `AuthFailedReason::MissingSubject` |
| The passthrough case: a tool call whose provider request would carry the inbound token | the mock provider sees `Authorization` equal to the operator credential and nothing else; the handler sees no `Authorization`, `Cookie` or `Proxy-Authorization` header at all |
| A token in the query string (`?access_token=...`) with no header | 401 with `MissingCredential`: OAuth 2.1 §5.1 is normative that resource servers MUST ignore an access token in a URI query parameter |
| An `aud` array carrying this resource plus others | **accepted** — RFC 9068 says `aud` must *contain* a resource indicator for this server |
| A bearer token presented at `/approvals`, and a session cookie presented at `/mcp` | at `/approvals`, **byte-identical** to the same request with no header at all (302 on a GET, 403 on a POST); at `/mcp`, 401 `MissingCredential`. Neither surface ever reads the other's credential |
| An **oversized body from an authenticated caller** at `/mcp` | **413** from the middleware, which buffers under the same `max_body_bytes` rmcp enforces (decision 3) |
| A `tools/call` with no `name`, and one naming a tool that does not exist | **403** `insufficient_scope` in both cases: an unknown name maps to no scope (decision 3) |
| A body `ClientJsonRpcMessage` cannot parse, from an authenticated caller | **400** from the middleware, before rmcp |
| A `notifications/initialized` from an authenticated caller | forwarded unchanged; no scope is required of it |
| A valid `willikins:read` token calling `apply` | 403, `error="insufficient_scope"`, `scope="willikins:apply"`, `resource_metadata` present |
| A flood of garbage tokens at `/mcp` | journal growth stays bounded by the per-reason bucket (decision 1), the coalesced lines carry `suppressed`, and the approvals page's fold latency is **measured** and recorded — pass-2 hand-over item 6, pulled forward because a public listener makes the fold attacker-reachable |
| An authenticated request carrying a foreign `Host` | the status rmcp actually returns, measured rather than inferred — pass-2 hand-over item 10 |
| **Each code binding attacked one at a time**: wrong `client_id`, wrong `code_verifier`, changed `redirect_uri`, changed or newly introduced `resource` | `invalid_grant` in each case, from the **stored** binding and not from a request claim, with its named `AuthFailedReason` |
| One authorization code redeemed **twice concurrently** | exactly one token issued; the second is `invalid_grant` `CodeReplayed` |
| A code redeemed after its own TTL, with the store still holding it | `invalid_grant` `CodeExpired` — the independent expiry check |
| A code redeemed by the **wrong client** versus an entirely unknown code | **indistinguishable** on status and body |
| A phase-1 `/authorize` error (unknown `client_id`, unregistered `redirect_uri`) attempted as a redirect | **400 directly**, never a redirect — a redirected phase-1 error is an open redirect |
| A registered loopback redirect on a **different port**, and on a different host or path | accepted; refused |
| `/authorize` with no `code_challenge`, and with `code_challenge_method=plain` | refused, `invalid_request` |
| `/authorize` with a `resource` that is not the canonical identifier | `invalid_target` |
| A JWKS served after a restart with **no persisted key** | every outstanding token is 401 `invalid_token` — the cost decision 22 refuses to pay, asserted so nobody ships it by accident |
| A ceremony finished against a **different account** than it started for | refused, `CeremonyAccountMismatch` — CVE-2026-69199's shape |
| A recovery code replayed | refused, `RecoveryCodeReplayed`, and the used code is already replaced |
| A **UV-lying** authenticator, and a **counter regression** | refused; `CredentialPossibleCompromise` on the second |
| A flood of `GET /approvals/login` and of ceremony starts, and a flood of recovery attempts | the ceremony store stops at its cap with **503**; the session store likewise; the Argon2 semaphore bounds concurrent hashes; neither memory nor the journal grows without bound |
| A ceremony started in one browser and finished in another | **400**: the `__Host-willikins-login` cookie does not match (decision 4) |
| An expired session cookie, one idle past the idle timeout, and one replayed after `POST /approvals/logout` or after a credential change | **302 to `/approvals/login`** on a GET and **403** on a POST in every case, with the plan's nonce not burned |
| A valid token whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS`, presented after login | 403 on the approve POST, `AuthFailedReason::WrongRole`, the decision not journaled as a grant, and the 403 page showing that session's own `sub` and `iss`, escaped |

Every bypass pass 3 finds becomes a fixture plus a test, as in passes 1 and 2, and the pass is
recorded under `docs/research/` with the same shape.

*Pinned by test 17.*

### 8. The exposure work pass 2 handed over

Four items, as tasks rather than notes, from `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`,
"Handed to milestone 3 (or 2c, the OAuth milestone)", items 1, 2, 3 and 9. They are here because
each one is a consequence of the same decision — no public domain until authentication is
stronger — and that decision changes in this milestone. They are **orthogonal to who issues the
token**, so this revision leaves them alone apart from one addition to the sweep. The other six
items and where each one went are in the out-of-scope list above.

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
  with its own short pass-3 addendum — see the task table. *Task 17, test 13.*
- **Make the origin check IPv6-aware** (its item 2). The approvals Origin/Referer parser is not
  bracket-aware, which is pointless to fix until an origin can be a bracketed literal and
  necessary once one can. Decision 4's trailing-`/` rule and its "block when neither `Origin` nor
  `Referer` is present" rule land in the same task, because they are the same parser. *Task 13,
  test 14.*
- **The log level story, with a sweep of rmcp's own debug output** (its item 3, finding 10).
  Milestone 2's trust boundary 5 made INFO the ceiling because no level could be raised; this
  task gives the operator `WILLIKINS_LOG` (a `tracing_subscriber` filter directive, defaulting to
  the current INFO behaviour) and then sweeps what the whole process emits at DEBUG and TRACE —
  which the pass could not do, because the level could not be raised. **The sweep is larger than
  the delegating draft's**: it now also covers `/authorize`, `/token`, the ceremony endpoints, the
  authorization-code store and anything that touches the signing key. The sweep is the acceptance
  test: at every level willikins can be configured to emit, no secret byte, no private-key byte, no
  authorization code, no `code_verifier`, no recovery code, no document text, no header value and
  no JSON-RPC body reaches stderr, or the level that does is refused. *Task 14, test 15.*
- **Move the concurrency permit into the blocking closure** (its item 9). One line, no behaviour
  change; it stops the 64-call bound's correctness from depending on rmcp running the stateless
  handler on its own task. *Task 15, test 16.*

### 9. The go-live sequence

Eleven steps, and the order matters. Six things freeze the host the moment they are published:
the resource identifier, the issuer identifier, the audience, both metadata documents' published
values, the registered `redirect_uri`s and the **WebAuthn RP ID**. So the host is chosen before
any of them is written anywhere. (Steps 2 to 5 were lost from this list by an edit on 2026-09-16.
They are restored from commit 656613a, and the volume and client steps of the 2026-10-20 addendum
are inserted after them.)

1. **The host is `willikins.example.com`** (operator, 2026-09-16). It is a custom domain on a
   name the operator owns, not a generated `*.up.railway.app` one, so the identifier the
   deployment publishes belongs to the operator and survives a move off Railway. This revision
   made the step heavier: that host is now also the **issuer identifier** and the **WebAuthn RP
   ID**. The audience has a documented migration path (decision 12). **Moving the RP ID means
   re-enrolling**: `webauthn-rs` 0.5.5 has no Related Origin Requests, so changing the host
   invalidates every enrolled passkey, and each human enrols again at the new host through the
   recovery path of decision 24 (and test 25). Choose the host knowing that; it can still be
   changed. Therefore:
   `WILLIKINS_PUBLIC_URL=https://willikins.example.com`, written in the canonical form decision 12
   requires (lowercase scheme and host, no port, no trailing `/`). The issuer identifier is that
   string exactly, the resource identifier and audience are `https://willikins.example.com/mcp`,
   the two metadata documents are at `/.well-known/oauth-authorization-server` and
   `/.well-known/oauth-protected-resource/mcp`, and the RP ID is `willikins.example.com`. Railway
   does not attach a domain automatically: the operator adds it in the dashboard or with the CLI.
   A custom domain needs **both** a CNAME and a TXT ownership record; with only the CNAME it
   answers 404 even after DNS resolves. Railway then issues the certificate itself, and gives up
   after 72 hours. Both vendor apps also require the name to resolve to a globally routable
   **IPv4** address (Anthropic: "Connectors are IPv4-only"), which test 19 checks.
2. **Declare it.** `.railway/railway.ts` can declare a custom domain (`domains: ["…"]`, or
   `{ domain, port }`) and **cannot** declare a generated one. That is documented, and it is the
   primary source behind the file's own header comment. So a custom domain goes in the file; a
   generated one is created in the dashboard or by the CLI, and the file's header comment is
   updated to say which. **This is conditional on a verify item.** Nothing states whether an
   omitted `domains` key deletes an already-attached *custom* domain; the documented "omit means
   delete" exemption covers only generated domains. So the file is never applied against a live
   custom domain until a read-only `railway config plan` has shown what the apply would do. Never
   settle this by applying.
3. **Measure what the edge sends, and what it already bounds.** Before any allowed-hosts value is
   frozen, deploy a build that echoes the inbound `Host`, `X-Forwarded-Host` and `X-Railway-Edge`
   on `/healthz`, and read them from a real public request. What `Host` reads at the service is
   documented nowhere on `docs.railway.com`. It decides both `WILLIKINS_ALLOWED_HOSTS` and rmcp's
   `allowed_hosts`, and the question now covers four unauthenticated routes rather than one
   (verify item 14). In the same visit, **measure what the edge does to a trickled header
   stream**: open a connection, send headers one byte at a time, and record when and how it is
   closed. Railway documents a "Max 32 KB combined header size" and says "idle HTTP/1.1
   connections are closed after 60 seconds between requests". So the edge already bounds part of
   this shape, and task 17 is **defence in depth behind it**, not the only bound.
4. **Enroll, and set both allowlists.** This step changed shape with the login. The operator runs
   the **enrollment CLI** on the host and enrols **two** credentials (decision 24's rule:
   prevention before recovery). The recovery codes are saved somewhere other than the deployment,
   and the subject the CLI prints goes into `WILLIKINS_AGENT_SUBJECTS` and
   `WILLIKINS_APPROVER_SUBJECTS` as needed. The `WrongRole` page keeps its bootstrap role as the
   **second** path, for anyone who enrolled without noting the value down. Both variables are
   required in http mode and an empty one refuses startup, so a reachable service never runs with
   an unset allowlist. **Where "the host" is on this deployment is open question 2**: the image is
   distroless, and the credential store and journal sit on the volume (decision 25). Whether
   `railway ssh` reaches a shell-less container is verify item 29.
5. **Populate the volume: the operator's documents and the client table** (decision 25). The
   operator publishes the private document set at a reviewed tag onto `/data/workflows`, by hand,
   following decision 25's procedure: a manifest from `willikins documents check`, an upload to a
   staging directory, a checksum comparison, then a swap. This replaces whatever interim set the
   image-switch cutover of task 20 left there. In the same visit the operator writes the
   pre-registered client table at `WILLIKINS_CLIENTS_PATH` (`/data/clients.yaml`). It holds one
   entry per app, each with its exact HTTPS redirect URI (decision 23):
   `https://claude.ai/api/mcp/auth_callback` for the Claude apps, and
   `https://chatgpt.com/connector_platform_oauth_redirect` for ChatGPT. That is the stable URI
   OpenAI documents for a server that advertises `iss` support, which decision 19 does. Any local
   client the operator uses gets an entry with its loopback URIs. The client ids are the
   operator's to pick (for example `claude-hosted` and `chatgpt`). The table lives outside
   `workflows/`, where a non-document YAML would refuse startup.
6. **One variable change, before the deploy that needs it, with `.railway/railway.ts` in the same
   change.** In a single staged edit:
   - set the whole new block: the credential-store and client-table paths, both subject
     allowlists, and the three lifetimes (the signing-key pair arrives through Doppler in step 7);
   - check that `WILLIKINS_WORKFLOWS_DIR=/data/workflows` is still set (task 20 set it);
   - give `WILLIKINS_ALLOWED_HOSTS` the measured public host beside the private-domain reference;
   - delete `WILLIKINS_AGENT_TOKEN_HASHES` and `WILLIKINS_APPROVER_TOKEN_HASH`.

   If the deletion and the new block were two changes around the deploy, there would be a window
   in which the service refuses to start, with `RetiredVariable` or `Missing` depending on which
   half landed first. So they are one change. (`healthcheck.railway.app` needs no entry: `/healthz`
   is served outside the host check.)

   `.railway/railway.ts` must move with it. Its `env` block names five variables through
   `preserve()` today, and two of them are the retired hashes. **The file's own header says an
   omitted resource or field is an instruction to delete it, not to leave it alone.** So the file
   drops those two rows and gains a `preserve()` row for **every** new `serve --http` variable;
   without that, the next apply deletes the deployment's whole configuration and the service
   stops starting. (The `WILLIKINS_WORKFLOWS_DIR` row arrives earlier, with task 20.) **Require a
   read-only `railway config plan` showing no variable delete before any apply.** The same plan
   must settle two questions that nothing documents: what `preserve()` does for a variable that
   does not yet exist live, and whether a push-triggered deploy picks up variable edits staged on
   Railway's variables page. The cutover order, and why:

   1. Stage the whole variable change in Railway.
   2. Edit `.railway/railway.ts`, then read `railway config plan`.
   3. Push the 2c commit, whose auto-deploy is what picks the staged variables up.

   Staged edits might turn out **not** to be picked up by a push-triggered deploy. The fallback
   used to read: the previous healthy deployment stays active behind the healthcheck while the
   new one fails to start, then a deploy is triggered by hand. **That fallback is now
   unverified.** For a service with a volume, Railway says "we prevent multiple deployments from
   being active and mounted to the same service. This means that there will be a small amount of
   downtime when re-deploying a service that has a volume attached, even if there is a
   healthcheck endpoint configured" (docs.railway.com/volumes/reference, fetched 2026-10-20). Whether
   a failed new deployment hands the volume back to the old one is not stated (verify item 30).
   Plan for an outage, not for "no outage".
7. **Connect Doppler's Railway integration for the secrets.** Doppler injects every provider
   credential the README's variable table names: today `WILLIKINS_GITHUB_TOKEN`,
   `WILLIKINS_DOPPLER_TOKEN`, `WILLIKINS_BUILDKITE_TOKEN` and `WILLIKINS_SIGNOZ_API_KEY`, with
   `WILLIKINS_SIGNOZ_HOST` beside them. On open decision 2's branch it also injects
   **`WILLIKINS_SIGNING_KEY_PEM`**, plus `WILLIKINS_SIGNING_KEY_RETIRING_PEM` during a rotation.
   The App Store Connect signing keys are **not** deployment variables: a document reads them from
   Doppler at run time, through the Doppler credential. There is no
   `WILLIKINS_OAUTH_CLIENT_SECRET`, because the approvals page is not an OAuth client of anyone.
   Doppler's integration is the only path a real credential takes onto the service, as it has been
   since milestone 2's task 12.
8. **Remove `WILLIKINS_FAKE_CATALOG`.** Until this step the deployed service serves the fake,
   in-memory catalog; after it, the real one. It runs against the documents step 5 placed, so
   that step comes first.
9. **Back up what cannot be re-derived, and rehearse a restore.** That means the credential store,
   the signing key, the client table and the journal. This is the first backup-and-restore concern
   the project has ever had. Losing the store means every human re-enrols from the break-glass,
   and losing the key refuses every outstanding token. The signing key's backup is Doppler's
   version history (open decision 2). The volume's candidate mechanism is Railway's volume
   backups; their schedule and restore rules are quoted in decision 25 and stay a verify item
   until a restore has been rehearsed. **Documents are not restored from a volume backup.** A
   restore rewinds the journal and the credential store along with them. Documents are recovered
   by re-publishing the tag from the operator's checkout (decision 25). Railway allows one volume
   per service, no replicas with a volume, and downtime on redeploy. The mount path is `/data`,
   taken from `.railway/railway.ts` (verify item 12, settled for that half).
10. **Connect the apps.** In claude.ai, add a custom connector with the server URL
    `https://willikins.example.com/mcp`. Pick **"Use your own OAuth client"**, enter the Claude
    entry's client id, and leave the secret blank. The dialog's default (Claude's published
    identity, a Client ID Metadata Document) fails against a server that advertises neither CIMD
    nor a `registration_endpoint`. Anthropic says "You can't change authentication settings after
    you add a connector", so a wrong pick means deleting the connector and adding it again. Then
    use it from the Claude iOS app. In ChatGPT on the web, add the server as a custom MCP
    connector with the ChatGPT entry's client id as its static credential. Whether that form
    accepts a blank secret is verify item 27. Then use it from the ChatGPT iOS app. Each
    connection is one passkey consent at `/authorize`.
11. **Record the tenancy.** The live service serves **one organization or workplace per provider
    credential**: one GitHub organization, one Doppler workplace, one Buildkite organization, one
    SigNoz host. That holds until milestone 3's credential routing lands. The operator runs
    several GitHub organizations and Doppler workplaces (milestone 2 plan, "Notes for milestone
    3"), and nothing in 2c changes that. A public domain changes who may reach the server, not how
    many organizations its credentials cover.

Steps 7 and 8 are milestone 2's own remaining operator steps. The milestone 2 plan is
`Completed`, and `WILLIKINS_FAKE_CATALOG: preserve()` is still in `.railway/railway.ts`. So step 8
is still work, not a check, and step 7 is a check only for the credentials milestone 2 already
connected.

*Pinned by test 19, which is a checklist run by hand with the operator, not a cargo test.*

### 10. There is no identity provider, and no authorization-server framework

**This decision is struck and replaced.** What it said — that the plan is provider-agnostic, that
any OIDC authorization server passes three must-haves, that the licence table filters six
candidates, and that self-hosted Logto is the recommended default — is gone in one line: the
operator ruled out depending on a vendor identity product in any edition, hosted or self-hosted, on
the ground that willikins is software other people self-host and a dependency on someone else's
identity product is one those people would inherit. The three must-haves die with it, because
willikins satisfies all three by construction. The six-provider survey and its licence table
survive **only as prior-art reading**, not as a recommendation.

What replaces it, in three parts:

**willikins is its own authorization server.** It publishes RFC 8414 metadata, runs `/authorize`
and `/token`, mints RFC 9068 access tokens for one audience, serves its own JWKS, and
authenticates its own humans. Decisions 19 to 24 are that server.

**No hosted or vendor identity product is a dependency, in any edition.** Reading one for its
source or documentation as prior art is encouraged; depending on one is not.

**No authorization-server framework is adopted either**, which is the less obvious half.
`oxide-auth` is the only established general-purpose one in Rust and it costs 18,732 lines across
three crates, plus three duplicate majors, to obtain roughly 1,500 wanted lines — while supplying
none of RFC 8414, JWKS, RFC 7591, RFC 8707 or `at+jwt`, and with a `Grant` that has no audience
field at all. `oauth-as` 0.9.4 is the closest fit in the ecosystem and is **read as prior art, not
depended on**: six weeks old, one author, one star, no audit, and 36,230 lines of
security-critical source, which is a larger trust surface than the product the operator rejected.
Both rejections, and the dated re-evaluation `oauth-as` gets, are in the pinned-dependencies
section.

**One thing survives out of the old decision 10, and it is its best structural idea.** Everything
that used to be "provider-specific" lives in **one configuration block** — the issuer identifier,
the key-set source, and the RFC 9728 document's `authorization_servers[]` value. That block does
not disappear now that willikins fills it with its own values; it becomes the seam's data layer.
See "The pluggable seam" below. No provider name appears anywhere in `crates/`, in a fixture, or in
an error message, which is exactly as true as it was before and now for a better reason.

*Pinned by test 3, which runs the whole validation matrix against a token willikins did not mint;
by test 20, which is the metadata document a stranger client reads; and by test 18, which drives a
stock client through the whole flow against no provider at all.*

### Eight smaller decisions the research left open

**11. The revision anchor is 2026-07-28's resource-server obligations.** willikins already
advertises `V_2026_07_28` in `get_info` while rmcp 3.3.0 negotiates `2025-11-25`; targeting the
later revision's rules satisfies both, because all three deltas are a tightening: the
scope-hierarchy MUST, the `offline_access` SHOULD NOT, and a 403 `scope` that names only what is
needed (2025-11-25 merely recommended naming more). **Its one condition is discharged.** The
delegating draft made task 4's first step "fetch the three 2026-07-28 sub-pages and the
`ext-auth` extensions", because `/authorization-server-discovery`, `/client-registration` and
`/security-considerations` had not been read and the index suggested the last covered "mix-up"
attacks. All four were fetched for the authorization-server research note, which quotes them
verbatim: the obligations they carry are folded into decisions 19 to 23, both `ext-auth` extensions
are OPTIONAL and additive so neither adds an obligation, and the mix-up worry is answered by
emitting `iss` on every authorization response (decision 20). This is no longer a verify item and
no longer a first step of any task.

**12. The resource identifier is `<public base URL>/mcp`, not the bare origin — and it now has two
siblings.** Both forms are legal and picking one freezes the other. `/mcp` is picked because RFC
9728 §3.3 says that when the client reached the document through the `resource_metadata` challenge,
the returned `resource` must be identical to the URL the client used to request the resource — and
that URL is `/mcp`. The origin also hosts `/approvals`, `/healthz` and now the authorization
server, which are not the protected resource. The RFC 9728 document is therefore served at
`/.well-known/oauth-protected-resource/mcp`, built by **inserting** the well-known string between
host and path, never by appending it to the path — the single most likely implementation mistake in
the milestone, and one a real MCP client has been observed probing for.

**It is derived from `WILLIKINS_PUBLIC_URL` and is not separately configurable.** An earlier draft
made the audience its own optional variable and the redirect URI another. Both were removed: RFC
9728 §3.3 requires the published `resource` to be byte-identical to the URL the client used, and two
values that can be set independently are two values that can disagree. Deriving makes that rule
hold **by construction** rather than by a startup comparison nobody wrote. Startup refuses a
`WILLIKINS_PUBLIC_URL` that carries a path, a query or a fragment, or that is not `https://`
(decision 7's loopback carve-out excepted), because each of those makes a derivation ambiguous or
an identifier insecure.

**It must also be canonical, or startup refuses it** (2026-10-20,
`HttpConfigError::NonCanonicalUrl`, which also covers the path, query and fragment cases above).
Anthropic says Claude sends `resource` "set to the canonical form of your MCP server URL ... The
canonical form has a lowercase scheme and host, no trailing slash, no fragment, and no default
port". OpenAI says ChatGPT copies the protected-resource document's `resource` exactly. The
derived issuer, audience and `resource` are compared byte for byte (checks 4 and 5, and decision
20's `invalid_target`). So a `WILLIKINS_PUBLIC_URL` with an uppercase scheme or host, an explicit
default port (`:443` for `https`, `:80` for `http`), or a trailing `/` would make every Claude
connection fail with `invalid_target`. Refusing it at startup, rather than canonicalising it
silently, keeps the operator's configuration identical to what is published. The check runs on
the **raw string**. A URL parser such as the `url` crate serialises `https://h` as `https://h/`,
so every derived identifier is built from the validated raw string, never from a parsed value's
serialisation. MCP's "implementations SHOULD accept uppercase scheme and host components" is about
what a client sends, which this refusal does not affect: an incoming `resource` is compared with
the one canonical value.

**Sibling one: the issuer identifier is derived too**, and is `WILLIKINS_PUBLIC_URL` exactly, with
no path. Two things follow. The RFC 8414 well-known URL's insertion and append forms **coincide**
for a bare-origin issuer, which removes by construction a three-route trap a shipped server has hit;
and the metadata document's `issuer` is byte-identical to the identifier the URL was built from,
which is MCP's MUST and which no startup check has to enforce.

**Sibling two: the WebAuthn RP ID is derived from `WILLIKINS_PUBLIC_URL`'s host, and moving it
costs every credential.** `webauthn-rs` 0.5.5 implements no Related Origin Requests, so a
credential enrolled against one RP ID cannot be presented to another: change the host and every
enrolled passkey stops working. That is a migration with a price, not a law — the operator's
correction, 2026-09-16: "All it takes for to stop being permanent is wiping accounts and changing
the domain." So the price, stated plainly so nobody has to rediscover it: each human re-enrols a
passkey at the new host, which means someone must be able to authenticate *to enrol*, which is
what decision 24's recovery codes (test 25) and the enrolment CLI are for. Task 10's break-glass path is
therefore also the domain-migration path, and it is tested as both. What is genuinely lost is the
old credentials, and nothing else: the journal, the plans, the subject allowlists and the issued
tokens' claims are unaffected, because a principal derives from issuer and subject, not from the
authenticator that proved the subject.

**Stable is not permanent, and the plan says how to move the audience.** `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`
is a comma-separated list of resource identifiers this server **also accepts** in `aud` during a
move. It is never published — the RFC 9728 document's `resource` is always the one derived value —
and it is never what a challenge names. While it is non-empty, startup logs a named warning saying
which identifiers are being accepted beyond the current one, so a half-finished migration is
visible in the first log line rather than a year later. **One consequence of the derived issuer has
to be stated rather than left for someone to trip over:** on the in-house path a host move also
moves the `iss`, and check 4 refuses an old-host token before check 5 ever reads `aud` — so the
variable does *not*, on its own, keep old tokens working across a willikins-to-willikins host move.
It does not need to: a host move orphans every passkey anyway (sibling two), and with no refresh
tokens every outstanding access token expires within `WILLIKINS_ACCESS_TOKEN_TTL_SECONDS`. Where
the variable does its work is the **adapter** path, where the issuer is somebody else's and does
not move when willikins does. Pass 3 attacks it in that shape: a token for a previous audience is
accepted only while the variable lists it, and rejected the moment it does not. A second issuer
identifier is not invented here for a case that does not exist; issuer-qualified configuration is
milestone 3's, beside the multi-issuer allowlists.

**13. Introspection and opaque tokens are absent; AS-metadata discovery reverses in exactly one
direction.** For the first two, see the out-of-scope list. For the third: willikins now
**publishes** RFC 8414 authorization-server metadata, because that is MCP's discovery MUST and it
is unavoidable (decision 19). It still **fetches** none, because there is no external authorization
server to fetch from — so the decision's actual sentence survives and the decision gains its other
half. What changes is the consequence: "a metadata document that lies about its issuer" now has no
consumer at all, because the callback it used to attack is gone. The attack that replaces it is
against *willikins' own* published document, and it is that the document's `issuer` must be
byte-identical to the URL it was fetched from — which is test-20-shaped and holds by construction
under decision 12's first sibling.

**14. The accepted `typ` set is frozen to `at+jwt` and `application/at+jwt`, with no knob.** RFC
9068 §4 requires rejecting any other value. An earlier draft made this configuration —
`WILLIKINS_OAUTH_ACCEPTED_TYP`, widenable by the operator with a startup warning — on the argument
that no fetched sentence said any surveyed provider actually sets `typ` to `at+jwt`. That variable
is gone, and so is the clause that replaced it: "the provider emits `at+jwt`" is not a provider
precondition checked by a live test any more, because **willikins emits it by construction**
(decision 22 sets `typ` explicitly, since `Header::new(alg)` defaults it to `"JWT"`). The set stays
**two-valued** rather than collapsing to one, and that is deliberate: an adapter's issuer may emit
the long form, and a validator that accepted only the short one would refuse it.

**15. The algorithm allowlist is the validator's, defaults to `ES256`, may hold only asymmetric
families, must contain the issuing algorithm, and is intersected per key.**
`WILLIKINS_OAUTH_ALGORITHMS` becomes **optional with default `ES256`**, because there is now a
known issuer whose algorithm the plan chooses (decision 22) rather than an unknown provider's to
guess at.

Startup refuses a symmetric family (`HS*`) with `HttpConfigError::SymmetricAlgorithm`. **The reason
has to be re-founded, because the old one is now false**: it used to be that a resource server
verifying an HMAC would hold the signing secret and could therefore mint its own tokens, breaking
trust boundary 1. willikins mints its own tokens. What survives, and is stronger, is boundary 1a:
the **validator** verifies with public keys only and is never the thing that mints, so a symmetric
algorithm would put a minting-capable secret on the validation path — reachable by every anonymous
request — where boundary 1b keeps the signing key on the issuing path and nowhere else. An
adapter's tokens land on the same validation path, which is the second reason.

Startup also refuses an allowlist that does **not** contain the issuing algorithm, with
`HttpConfigError::CannotVerifyOwnTokens`. This is a consequence of the two halves sharing a process
rather than a fetched rule: an operator who narrows the allowlist to `RS256` alone gets a server
that refuses every token it mints, and the refusal is a startup line instead of a mystery at
runtime.

**RS256 stays permissible in the allowlist and is never used to sign.** RFC 9068 §2.1 says a
conforming server "MUST include RS256 among their supported signature algorithms", and the way to
satisfy that without exposing RUSTSEC-2023-0071 is to keep RS256 on the **verifying** side — where
the original public-key-only reasoning genuinely holds — while the issuer signs ES256 only
(decision 22).

**The allowlist is applied per key, never per token header.** For each key in the set, the
`Validation` the verification runs under carries **the allowlist intersected with that key's own
family** — so an RSA key is only ever asked to verify an `RS*`/`PS*` algorithm and an EC key only
an `ES*` one. The token's `alg` header selects nothing: it is compared against that intersection
and refused if it is outside. `jsonwebtoken` refuses a verifier whose key family differs from an
allowed algorithm's family, which means a single `Validation` carrying a mixed allowlist fails
*every* verification rather than choosing correctly — so the intersection is what makes a
two-family allowlist work at all. This was a migration nicety in the delegating draft; it is now
**load-bearing**, because it is the only way the "sign ES256, verify RS256 too" split above is
implementable.

**16. The key set comes from a `JwkSource`, and in 2c there is exactly one implementation.** The
fetch-and-cache the delegating draft specified has **no production consumer** now: issuer and
validator share a process, and the resource-server half must **not** fetch its own `jwks_uri` over
the loopback. So the boundary is introduced and only the in-process implementation is shipped — one
method, "where does the key set come from", with exactly two implementations it will ever have.

That removes `WILLIKINS_OAUTH_JWKS_URI`, `WILLIKINS_JWKS_TIMEOUT_SECONDS`,
`WILLIKINS_JWKS_REFRESH_SECONDS`, `WILLIKINS_JWKS_MIN_REFETCH_SECONDS`,
`StartError::JwksUnavailable`, `ureq`'s move into `willikins-server`'s `[dependencies]`, and test
5's hang and HTTP-rollover cases. **What it costs, stated rather than discovered:** the seam is
asserted by a second **in-process** issuer — decision 7's foreign-issuer fixture — rather than by an
HTTP one. The day an adapter lands, the fetch-and-cache is *new code written against a boundary
that already exists*: an addition, not a rewrite. Test 5 shrinks to in-process key rollover, the
two-key overlap window, and the assertion that no outbound request happens on the validation path
at all.

**The three rules the removed fetcher carried are written down here rather than re-derived later**,
marked as the adapter path's: a failed background refresh keeps the previous key set and logs a
warning at **every** failed interval, because emptying the cache turns one unreachable provider
into a total outage of a service whose keys are still good; the accepted staleness bound is one
refresh interval past the first successful fetch that no longer carries a removed key, and
indefinitely while the provider is unreachable; and a token whose `kid` is unknown triggers at most
one refetch per floor interval, so a stream of forged `kid`s cannot make willikins hammer an
issuer. **The rejected branch, recorded so it is not re-proposed:** keeping the fetcher and pointing
the resource server at its own `jwks_uri` would preserve every variable and test 5 whole, at the
cost of a loopback HTTP round trip on the validation path, a cache of the server's own keys, and a
rule the seam explicitly forbids.

One property survives from the delegating draft and is now the issuer's problem as well as the
validator's: a token whose header carries **no `kid`** never matches, because `JwkSet::find` only
matches keys that have one. Decision 22 is what makes that safe rather than fatal — willikins sets
a `kid` on both the header and the published JWK — and test 21 is where it is asserted instead of
assumed. A token that cannot be verified for any of these reasons is 401 `invalid_token`: the
specification's status table has no code for "the key is not available", and a token willikins
cannot verify is not a token it may honour.

**17. Clock skew is bounded at `WILLIKINS_OAUTH_LEEWAY_SECONDS`, default 60**, which is
`jsonwebtoken`'s own default `leeway`, made explicit rather than inherited so that a future version
changing its default cannot change willikins' behaviour silently. `validate_nbf` is turned on, and
`required_spec_claims` is set to `exp`, `aud`, `iss`, `sub`.

**One asymmetry is deliberate and must not be "fixed".** The **issuer** emits all seven of RFC
9068's REQUIRED claims (decision 22) while the **validator's** required set stays at four. That is
not an oversight: a validator demanding seven would refuse an adapter's token, decision 2 already
tolerates a missing `client_id`, and the two halves are allowed to be stricter and laxer in that
direction and only that direction.

**18. The rmcp 3.4.0 bump is its own task and is conditional.** 3.4.0 does not move any
resource-server work into rmcp, so nothing here needs it. **`allowed_origins` itself is not the
reason to bump**: it already exists at 3.3.0, as a `StreamableHttpServerConfig` field with a
`with_allowed_origins` builder (`rmcp-3.3.0/src/transport/streamable_http_server/tower.rs:119` and
`:212`), so **task 5 sets it unconditionally** at the version the lock holds. What 3.4.0 adds is
only `enforce_origin_validation()`: at 3.3.0 an **empty** allowlist skips origin validation entirely
(`tower.rs:884`), and that method makes an empty list enforce rather than exempt. Since task 5 sets
a non-empty list, the delta is a defence against a future empty one, which is worth having and is
not worth blocking on. It is not a lock bump on its own: `ServerInfo` is deprecated at 3.4.0 and
used at three sites in `mcp.rs` under `-D warnings`. *Conditional*: no cargo command was permitted
during the research, so whether `cargo update -p rmcp --precise 3.4.0` resolves and the four gates
pass is unverified, as is the effect of 3.4.0's cancellation and pre-init changes on willikins'
stateless configuration.

**What the `allowed_origins` list holds** (2026-10-20; the plan never said). It holds exactly
one entry: the origin of `WILLIKINS_PUBLIC_URL`, derived like every other identifier (decision 12)
and not separately configurable. rmcp 3.3.0's `validate_origin_header` (around `tower.rs:884`)
does three things. It skips the check for an empty list. It accepts a request with **no** `Origin`
header. And it answers `Forbidden: Origin header is not allowed` for a present but unlisted one.
That list therefore refuses foreign *browser* origins and leaves header-less server-to-server
callers alone. **This is conditional on a measurement.** Anthropic lists "Strict `Origin`-header
validation: an overly strict check rejects Anthropic's requests" among the common causes of an
`initialize` timeout. MCP 2026-07-28 says "If the `Origin` header is present and invalid, servers
MUST respond with HTTP 403 Forbidden". No primary source says which `Origin`, if any, Anthropic's
or OpenAI's backend sends (verify item 26). Test 19 records it on a real Claude and a real ChatGPT
`initialize` before go-live. If either sends one, it is listed by exact value before the list
freezes; the list is never emptied to make it pass. Pass 3 gains the row "a vendor-backend
`Origin` is not 403".

**If task 1b is skipped, nothing changes.** No task and no acceptance test depends on it: the
origin value is set at 3.3.0, the scope check's placement is settled at 3.3.0 (decision 3), and the
milestone ships on the lock as it stands. Recording why it was skipped is the whole of the
obligation.

### Six decisions the authorization-server half adds

**19. Two metadata documents and a JWKS route, all anonymous.** MCP requires an authorization
server to provide at least one of RFC 8414 metadata or OIDC Discovery. **RFC 8414 alone is
conformant and is the cheaper branch** — no OIDC provider metadata and no ID tokens — so that is
what willikins serves, at `/.well-known/oauth-authorization-server`, built by insertion like its
sibling. Both documents and `/jwks.json` are registered on the root router, outside the bearer
middleware and outside rmcp's nest, exactly as `/healthz` is: a client must reach all three before
it holds anything.

The RFC 8414 document is a `#[derive(Serialize)]` struct, not a crate. Five of its fields are not
optional in practice, each for a fetched reason:

- **`issuer`** byte-identical to the identifier the well-known URL was built from. If they differ,
  a client MUST NOT use the metadata. It holds by construction (decision 12).
- **`code_challenge_methods_supported: ["S256"]`.** If it is absent, "the authorization server does
  not support PKCE and MCP clients MUST refuse to proceed". There is no runtime PKCE discovery;
  this field is the only signal there is, and omitting it breaks every conformant client silently.
- **`grant_types_supported` and `token_endpoint_auth_methods_supported`, emitted explicitly.**
  Their RFC 8414 defaults describe a server willikins is not: omitting the first defaults to
  `["authorization_code", "implicit"]`, a grant OAuth 2.1 deletes; omitting the second defaults to
  `client_secret_basic`, which makes every public client appear unsupported. willikins emits
  `["authorization_code"]` and `["none"]`.
- **`authorization_response_iss_parameter_supported: true`**, which RFC 9207 §2.3 makes a MUST for
  any server that supports the parameter, and decision 20 does.
- **`jwks_uri`**, published and served. RFC 8414 makes it OPTIONAL and MCP requires no JWKS at all
  from a co-hosted authorization server, and it is published anyway. **This is the seam**, and it
  is the row in this plan a reader is most likely to cut as unnecessary: a resource server that
  only ever knew how to read a key out of its own process *is* the rewrite the pluggable-auth todo
  exists to avoid.

**Array claims with zero elements are omitted, never serialised as `[]`** — RFC 8414's own MUST,
which bites a derived struct directly and is answered by `skip_serializing_if = "Vec::is_empty"` on
every array field. `client_id_metadata_document_supported` is **absent**, because advertising it
without implementing it sends clients to a dead end (decision 23). The RFC 9728 document keeps
everything decision 1 already specifies; only its `authorization_servers` value changes, to
willikins' own issuer identifier.

**RFC 8414's REQUIRED members are pinned as well, not left to the struct** (2026-10-20). RFC 8414
§2 lists three: "response_types_supported REQUIRED"; `authorization_endpoint`, "REQUIRED unless no
grant types are supported that use the authorization endpoint"; and `token_endpoint`, "REQUIRED
unless only the implicit grant type is supported". willikins emits
`response_types_supported: ["code"]` and both endpoint URLs, derived from `WILLIKINS_PUBLIC_URL`.
**The document also publishes `scopes_supported`**, with the same three MCP scopes as the `/mcp`
document and never `offline_access` or `willikins:approve` (decision 3). Two vendor rules make that
worth stating. Claude's 403 step-up falls back to "the authorization server metadata's
`scopes_supported`". And OpenAI says that when a provider advertises scopes "in `scopes_supported`
of its `.well-known/oauth-authorization-server` ... ChatGPT requests those scopes by default". So
the two documents must agree, and test 20 asserts that they do.

*Pinned by tests 1, 20.*

**20. `/authorize`, PKCE, and the consent step.** `GET /authorize` accepts `response_type=code` and
nothing else, with the implicit grant **structurally absent** rather than branched on — the same
discipline willikins already applies with domain newtypes, and the shape prior art uses.

- **PKCE is mandatory and `S256` is the only method.** draft-16 §4.1.1 makes `code_challenge_method`
  REQUIRED with value `S256` or a future extension: there is no `plain` and no default to fall back
  to. A request with no `code_challenge` is refused, and an unsupported transformation answers
  `invalid_request`. The "defaults open" hazard a framework would have carried does not exist here
  because `plain` is not representable.
- **Errors are two-phase, and phase 1 never redirects.** A bad `client_id` or a `redirect_uri`
  outside the registered set answers **400 directly**. Redirecting a phase-1 error is an open
  redirect, which is the whole reason the split exists.
- **Redirect URIs are matched exactly against the registered set, with the loopback port
  exception.** MCP requires exact validation against pre-registered values; draft-16 §8.4.2
  requires that "the authorization server MUST allow any port to be specified at the time of the
  request for loopback IP redirect URIs", because a desktop MCP client binds an ephemeral port at
  request time. A literal string-equality matcher is non-conformant and a matcher that ignores the
  port everywhere is a vulnerability, so the exception is scoped to loopback hosts and to the port
  alone. **`localhost` joins `127.0.0.1` and `[::1]` in that set** (verify item 8, settled
  2026-10-20). Claude Code registers both `http://localhost/callback` and
  `http://127.0.0.1/callback` and needs both matched with the port ignored. The relaxation never
  cross-matches between the three hosts, which is the TypeScript SDK's rule. **The two hosted
  apps' redirects are HTTPS and matched exactly, port included**:
  `https://claude.ai/api/mcp/auth_callback` and
  `https://chatgpt.com/connector_platform_oauth_redirect`. These are the redirects that matter
  most for this deployment, and test 22 has a case for them.
- **`state` is echoed byte for byte on every redirect, success and error alike** (2026-10-20;
  the earlier text never said so). RFC 6749 §4.1.2: "state REQUIRED if the "state" parameter was
  present in the client authorization request. The exact value received from the client." MCP's
  security considerations: clients "SHOULD use and verify state parameters ... and discard any
  results that do not include or have a mismatch with the original state". Without it, both
  target apps would throw the callback away. `state` is stored with the pending authorization,
  beside `client_id`, `redirect_uri`, `code_challenge` and `resource`. It is returned unchanged
  next to `iss`, and omitted when the request had none. It is opaque, bounded like every other
  stored request value, never interpreted, and never journaled. (Decision 4 deleted the approvals
  page's *own* `state`, which existed because that page was a client. This is the server-side
  half, which every authorization server owes its clients.)
- **`iss` is on every authorization response, error responses included.** RFC 9207 §2 makes it a
  MUST for any server supporting it and draft-16 §4.1.2 already lists it REQUIRED where MCP still
  says SHOULD. It is one query parameter and one metadata boolean now, and expensive to retrofit;
  it is also the mix-up countermeasure that discharges decision 11's last worry.
- **The consent screen clearly displays the redirect URI's hostname**, with an additional warning
  for a `localhost`-only redirect. Both are MCP security-considerations requirements, and the
  hostname display is the only thing standing between a human and an attacker-registered redirect.
  The screen presents the three MCP scopes of decision 3, read from the same table the middleware
  uses, and requires a session, which is the second consumer decision 4 gives the human session.
  Its CSP widens `form-action` to the one validated redirect origin (decision 4).
- **An `/authorize` with no session parks the request, logs in, and resumes** (2026-10-20; the
  earlier text did not say how). On a phone, the first `/authorize` comes from the Claude or
  ChatGPT app in a fresh browser context with no willikins session. In order:
  1. Phase 1's checks run first: client, redirect, PKCE, `resource` and `state` bound.
  2. The validated request is parked server-side, in a store shaped like the ceremony store (300 s
     TTL, pruned on insert, capped at 1,024, 503 when full).
  3. The browser goes to `/approvals/login` carrying only an opaque id, with the parked request
     bound to it by a short-lived `__Host-willikins-authorize` cookie, as the ceremony is bound by
     `__Host-willikins-login`. A resume link opened in another browser therefore resumes nothing.
  4. After the passkey ceremony, or the recovery-code path, the browser returns to the consent
     screen for exactly the parked request.
  5. The parked entry is claimed once, by the same atomic get-and-remove as a code.

  A request that expires while parked ends in an error redirect, which carries `state` and `iss`
  like any other. Which browser context each iOS app opens, and whether `navigator.credentials`
  works there, is verify item 13.
- **`resource` is accepted, bound into the code, and narrowed.** Anything that is not the one
  canonical resource identifier is refused `invalid_target`. This is the hinge between the two
  halves in one process: the `aud` the issuer writes here is the `aud` the validator already
  checks.
- **Both endpoints carry their own bounds**, because both are anonymously reachable on a public
  domain with no edge rate limit: a default rate limit on `/authorize` and `/token`, and
  `Cache-Control: no-store` set before anything else on the response. **The numbers are this
  plan's choice and no specification sets them**; the prior art they are taken from is
  Cloudflare's shipped MCP authorization server, which defaults to 100 requests per 15 minutes at
  `/authorize` and 50 at `/token`.
- **Every refusal is journalled**, through the machinery decision 1 already specifies rather than a
  second one: new additive `AuthFailedReason` variants, with the refusals of an unauthenticated
  request passing through the same per-reason token bucket and carrying the same `suppressed`
  field. The variants are listed in the journal contract below.

**The design doc's "no raw URL" invariant governs tool ports, not protocol machinery**, and that is
stated here rather than left to a reviewer. `/authorize` takes a client-supplied `redirect_uri`,
which reads as a violation until the narrowing is written down: **a `redirect_uri` is matched
against the pre-registered set and is never dereferenced, fetched or followed by the server.** If
Client ID Metadata Documents are ever added, their outbound fetch needs a narrowly typed fetcher
with private-address and loopback refusal, HTTPS only and a 5 KB cap — not a general HTTP client.

*Pinned by tests 7, 22.*

**21. `/token`, the authorization code, and its four stored bindings.** `POST /token` accepts
`grant_type=authorization_code` and nothing else: each grant not implemented is one that cannot be
attacked.

- **The code is single-use, claimed by an atomic get-and-remove**, with a lifetime capped at
  draft-16's RECOMMENDED maximum of 10 minutes and **its own expiry checked independently of any
  store TTL**. A find-then-delete leaves a window in which two concurrent token requests redeem one
  code — a bug that appears only under concurrency, which is where a test written later will not
  find it. A shipped server claims with `get_remove` and still re-checks the code's own expiry
  against the clock, and willikins does both.
- **The code record stores its bindings — `client_id`, `code_challenge`, `redirect_uri`,
  `resource` — and each is re-checked at redemption, from the store.** draft-16 §4.1.2 binds the
  code to the client identifier, the code challenge and the redirect URI, and RFC 6749 §4.1.3
  requires the token request's `redirect_uri` and the authorization request's values to be
  identical, which is a separate obligation from re-checking the registered set. **In the store,
  not only in a claim**, and that phrasing is deliberate: the one fetched post-mortem of a
  comparable Rust server is a rewrite that deleted the last copy of a binding while keeping the
  comment that claimed it happened, and a professional audit running in the same window missed it.
  Task 11 ships a negative fixture per binding for that reason.
- **`resource` at the token request may only match what the code granted** — never widen, never
  introduce.
- **Clients are public; `token_endpoint_auth_method` is `"none"`; there is no client secret
  anywhere in the deployment.** Every client this deployment serves is a public client. That
  covers local MCP clients with loopback redirects, and also the two hosted apps the operator
  named. Anthropic: "Leave the secret blank unless your authorization server requires one".
  OpenAI says token-endpoint method `none` applies to "CIMD, DCR, and predefined OAuth clients"
  (whether ChatGPT's static-credential form accepts a blank secret is verify item 27). draft-16
  accepts a public client given mandatory PKCE. (The earlier text justified this with loopback
  redirects alone. The conclusion stands, but that reason covered neither target app.) `/token`
  accepts `Content-Type: application/x-www-form-urlencoded`, the only body RFC 6749 defines and
  the one Anthropic requires, and answers well inside Claude's 10-second wait. The one confidential client the delegating
  draft had — the approvals login — stops being an OAuth client at all (decision 4), so the whole
  client-secret surface disappears rather than moving.
- **A code redeemed by the wrong client answers `invalid_grant` indistinguishably from an unknown
  code**, which turns a wrong-client redemption into a non-oracle.
- **No refresh token is issued, and `offline_access` is never advertised.** MCP says clients "MUST
  NOT assume refresh tokens will be issued; the authorization server retains discretion", and
  protected resources "SHOULD NOT include `offline_access`". Not issuing them is conformant and it
  **deletes the only stateful authorization-server requirement there is** — rotation with reuse
  detection and family revocation — which is also where every one of the fetched corpus's worst
  problems lived. The cost is named rather than hidden: the access-token lifetime becomes exactly
  how often a human re-consents at `/authorize` (open decision 4), and an operator who believes a
  token leaked waits that lifetime out, because there is no revocation endpoint either. **On the
  phone use case that put 2c on the critical path, that means a passkey round trip every 3600 s.**
  Both apps refresh reactively on a 401 (Anthropic: "Claude refreshes tokens reactively on a `401`
  response, and proactively up to five minutes before the stored expiry"), and with nothing to
  refresh they must re-run OAuth. Whether either app does that inline or flips the connector to
  "Reconnect" is verify item 32, recorded by test 19. The decision is the operator's and is not
  changed here (open question 1). The ways out are those of
  `todos/2026-09-16-unattended-agents-token-longevity.md`. If refresh is ever added, Anthropic
  requires rotation for public clients: "Rotate refresh tokens for public-client connections".

*Pinned by test 23.*

**22. The signing key: ES256, a thumbprint `kid` on both sides, persistence, and a two-key
overlap.** The token-minting half costs **zero new crates**: `jsonwebtoken` 11 signs, derives the
public JWK from the private key, and computes the `kid`. **It is not even the first ES256 signer in
the tree.** `crates/willikins-providers-http/src/apple_credential.rs` already mints App Store
Connect JWTs: it calls `EncodingKey::from_ec_pem` at `:175` and `Header::new(Algorithm::ES256)` at
`:213`, under the workspace's `jsonwebtoken = { version = "11", features = ["rust_crypto",
"use_pem"] }`. `crates/willikins-types/src/appstore.rs` canonicalises SEC1 or PKCS#8 input to
PKCS#8 with `p256` for exactly that call. The issuer copies that shape. The key is a
`#[domain(secret)]` type in `willikins-types`, modelled on `AppleSigningKey`, whose own
`reveal_for_signing` is the sixth token-less `expose_secret` exception (trust boundary 2), called
only by `issuer::mint`.

- **The token carries all seven of RFC 9068's REQUIRED claims** — `iss`, `exp`, `aud`, `sub`,
  `client_id`, `iat`, `jti` — with `jti` REQUIRED here even though it is OPTIONAL in plain RFC
  7519, and with **no single-party carve-out** for a co-hosted authorization server.
- **`typ` is set to `at+jwt` explicitly**, because `Header::new(alg)` defaults it to `"JWT"` and
  the validator's frozen set (decision 14) would refuse that. This is the cheapest possible way to
  fail to validate one's own tokens.
- **`iss` and `aud` are two different strings even though one process plays both roles.** RFC 9068
  §5: an authorization server MUST use a distinct identifier as an `aud` value to identify tokens
  issued for distinct resources. `iss` is the RFC 8414 issuer identifier; `aud` is always a
  resource identity.
- **Signed asymmetrically, never `none`, and never with the RSA family.** The first two are RFC
  9068 §2.1. The third is this deployment's own normative constraint and it replaces a dismissal
  that is now void: `jsonwebtoken`'s `rust_crypto` feature drags `rsa` in unconditionally, and
  RUSTSEC-2023-0071 (`patched = []`) is a private-key signing-timing leak observable over the
  network — which did not apply to a server that only verified and **does** apply to one that
  signs. The issuing algorithm is **ES256**, with RS256 kept in the validator's allowlist only
  (decision 15).
- **`kid` is set on both the JOSE header and the published JWK, to the same value, an RFC 7638
  thumbprint.** `Jwk::from_encoding_key` leaves `kid` as `None` and `JwkSet::find` skips keys
  without one, so a JWKS built the obvious way contains a key the validating half can never select
  and willikins fails to validate its own tokens. `jsonwebtoken` 11 computes the thumbprint
  natively, in RFC 8037's lexicographic member order, so the two halves agree on a name with no
  registry. This is a self-inflicted MUST and it does not degrade gracefully.
- **If Ed25519 is ever offered, PKCS#8 v1 only.** `Jwk::from_encoding_key` matches the Ed key
  length against exactly 48 bytes and every default generator examined emits v2. **ES256 has
  neither trap**, which is the quiet argument for choosing it and never reaching this row.
- **The private key is persisted**, has a redacted `Debug`, no `Display`, no `Serialize`, and is
  zeroized. Without persistence every outstanding access token dies silently on every restart: a
  new key has a different thumbprint, so lookup returns nothing for every older token and RFC 9068
  §4 requires `invalid_token`. Railway redeploys a volume-attached service with downtime anyway, so
  restarts are not rare. Redaction by construction is the codebase's own invariant, applied to a
  signing key — `EncodingKey` already derives `Zeroize, ZeroizeOnDrop` and prints
  `content: "[redacted]"`.
- **Rotation is a two-key overlap**: publish both with distinct `kid`s, sign with the new one from
  the moment it is published, keep the old until every token it signed has expired, then drop it.
  RFC 7517 §4.5 is the only normative statement and prescribes no overlap length; RFC 9068 §4 warns
  that publishing more keys widens the compromise surface, so **two live keys is a ceiling and not
  a steady state**. The overlap window is exactly the maximum access-token lifetime, which is why
  `WILLIKINS_ACCESS_TOKEN_TTL_SECONDS` and the rotation window are one number and one decision
  (open decision 4). The *schedule* is deferrable and the *mechanism* is not: retrofitting the
  ability to publish two keys at once means a flag day in which every outstanding token is refused.

*Pinned by tests 5, 21.*

**23. Clients are pre-registered, one configuration entry each.** No registration mechanism is a
server MUST. MCP grades Client ID Metadata Documents **SHOULD**, dynamic client registration **MAY
and deprecated**, and pre-registration carries no MUST at all — while its own client preference
order puts pre-registration *first*. For a handful of agent clients that is not a compromise; it is
what makes an in-house authorization server genuinely small.

The table holds a client id, a display name and a set of registered redirect URIs per client. It
lives on the volume at `/data/clients.yaml` (decision 25), outside `workflows/`, and it is written
by the operator by hand. At go-live it holds one entry per target app (go-live step 5), each with
the exact HTTPS redirect its vendor documents. **Pre-registration works for both apps, but only on
a non-default path, which the plan states so nobody discovers it at the dialog.** Claude's default
is its published identity, a Client ID Metadata Document, and Anthropic says Claude "falls back to
DCR" when the server advertises neither. willikins advertises neither, so that default fails. The
operator must pick "Use your own OAuth client", and "You can't change authentication settings after
you add a connector". ChatGPT uses static credentials when they are entered: "if static
credentials are provided, then they will be used".
`client_id_metadata_document_supported` is advertised **only if** CIMD is implemented, which it is
not in 2c: the draft makes advertising it a MUST for any server that implements it *and* publishes
RFC 8414 metadata, and the converse is that advertising it without implementing it sends clients to
a dead end.

**What deferring each alternative costs, so neither is re-argued from scratch.** Without CIMD, a
client with no prior relationship cannot self-register: every new client is one configuration entry
the operator adds by hand, and CIMD is also the **only** registration form whose client ids survive
a later change of authorization server — so deferring it makes a future adapter marginally more
expensive. Add it when a named client needs it. Dynamic registration costs nothing to defer and
should not be built at all: it is an anonymous write endpoint, a persisted registry, a garbage
collector and a rate limit.

*Recommended by open decision 3, and pinned by test 18's unregistered-then-registered pair.*

**24. Authenticating the human: passkeys, recovery codes, and a host-side break-glass.** MCP says
**nothing whatsoever** about this — no MUST, no SHOULD, no shape — and all three MCP reference
implementations declare the login out of scope. Everything here is willikins' own, and it is where
the real cost of the milestone is.

- **Passkeys through `webauthn-rs` 0.5.5** (open decision 1). The reason is not phishing
  resistance: this deployment has no email and no SMS, so a password path has **no reset**, and
  losing the reset also removes the account-lockout mechanism's only escape hatch.
- **A durable credential store**, on the Railway volume beside the journal. A `Passkey` record
  cannot be in-memory the way sessions and nonces are, and the journal is append-only and is not a
  credential store. This is the first durable identity state willikins has ever owned.
- **Ceremony state is server-side**, with the shape the pending-login store already had — 300 s
  TTL, pruned on insert, capped, 503 when full, which is also the crate's own
  `DEFAULT_AUTHENTICATOR_TIMEOUT`. The crate refuses to derive serde on that state by default,
  precisely so it cannot be put in a cookie and replayed.
- **Each ceremony is bound to the account it finishes against.** CVE-2026-69199 was WebAuthn state
  stored under a code but not bound to a user id, with the target user taken from a separate
  argument, so an attacker could sign a challenge for their own account and finish against a
  victim's. It is the same class as the two-approver attack decision 4 already closes, and the
  `__Host-willikins-login` cookie is the same answer in its WebAuthn form.
- **`UserVerificationPolicy::Required`**, `require_valid_counter_value`,
  `Passkey::update_credential` on every successful authentication, `exclude_credentials` at
  registration, and an assertion that a registered `CredentialID` has not previously been
  registered to any other account — the last being the caller's obligation, which the crate cannot
  enforce. UV Required is the crate's default and is what makes its own claim true that a passkey
  is self-contained multifactor authentication, so no password is needed beside it. The counter
  check aborts with `CredentialPossibleCompromise` and tolerates zero on both sides, which is
  normal for synced passkeys.
- **`backup_eligible` and `backup_state` are recorded, not refused.** Refusing cloud-synced
  passkeys is not reachable through the safe API anyway, and it would refuse the most likely
  recovery path for a single operator who loses a device. NIST treats the flag as policy input, not
  a blanket rejection.
- **Recovery codes**: at least 128 bits from a CSPRNG, Argon2id-hashed, single-use, and a new one
  issued after each use. NIST SP 800-63B-4 §4.2.1.1 requires at least 64 bits, storage hashed, and
  invalidation plus reissue after use. **This plan follows the reading that satisfies both
  §4.2.1.1 and §3.1.2.2** — at least 128 bits *and* Argon2id — and says so rather than claiming the
  documents settle it; §3.2.2's throttling numbers were not read verbatim and are verify item 10.
  This is what puts `argon2` in the tree **on the passkey branch too**.
- **Two credentials before enrollment is complete**, or the operator is nagged until a second
  exists. Prevention before recovery: a single-credential deployment is one lost device away from
  the break-glass, every time.
- **A host-side break-glass**: a CLI subcommand on the host that mints a one-time enrollment URL to
  stdout and journals it as loudly as an approval. That is consistent with the design doc's own
  trust model — the boundary is the network, and whoever holds host, environment and Doppler access
  already controls the deployment. What a deployment must **not** do is add a fourth layer that
  reintroduces a static shared secret, which trust boundary 5 forbids.
- **A username step that answers uniformly.** `start_passkey_authentication` takes the account's
  own credentials, so the server must know which account before it can build a challenge; the
  usernameless alternative needs a preview feature the crate's authors advise against. For a single
  operator that is one text field, and it is an account-existence oracle unless it answers
  uniformly.
- **The first `<script>` on the approvals page**, inlined under a `script-src` with a per-response
  nonce. The ceremony runs through `navigator.credentials.create()` and `.get()`, and the page must
  base64url-decode the challenge, the user id and every credential id before the call and re-encode
  five fields for the POST back — about a hundred lines. The approvals page contains no `<script>`
  today, so 2c's CSP grows a `script-src` it did not have.
- **The recovery path is not deferrable to a later milestone**, and the reason is in the risks: if
  any browser in the operator's environment refuses `navigator.credentials` on the deployment's
  domain (verify item 13), the recovery codes are the *only* mitigation the plan's shape offers.

*Recommended by open decision 1, and pinned by tests 10, 24, 25.*

### One decision the 2026-10-20 addendum adds

**25. The deployed server reads its workflow documents from the mounted volume, and the image
carries none.** The operator, 2026-10-20: "I'd go with a mounted volume." The operator's documents
are private: they name an organization, its apps and its layout. They must never ship in the public
repository, in its image, or in the build context. Today the image *is* the trust root. The
Dockerfile copies the public `workflows/*.yaml` into `/app/workflows` and sets
`ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows`, and
`crates/willikins-server/tests/it/image_contents.rs` pins both, plus "exactly the seventeen positive
documents". Nothing in the plan before this addendum said where a deployed server's documents come
from. Yet step 8 of the go-live sequence and test 19's real plan-approve-apply cycle both need the
private set on the server.

**The Railway facts it rests on**, fetched on 2026-10-20 as the `.md` twins `docs.railway.com/llms.txt`
lists. A fact that is not quoted here is not assumed.

- `docs.railway.com/volumes/reference`: "Each service can only have a single volume"; "Replicas
  cannot be used with volumes"; "we prevent multiple deployments from being active and mounted to
  the same service. This means that there will be a small amount of downtime when re-deploying a
  service that has a volume attached, even if there is a healthcheck endpoint configured"; "Volume
  files can be managed from the CLI with `railway volume browse` or `railway volume files`, and over
  `sftp` or `scp` through a service the volume is attached to".
- `docs.railway.com/volumes`: "Volumes are mounted to your service's container when it is started,
  not during build time"; "Volumes are mounted as the `root` user"; `RAILWAY_VOLUME_MOUNT_PATH` is
  the "Mount path of the volume".
- `docs.railway.com/cli/volume`: `files upload` takes a "Local file or directory to upload";
  `--overwrite` means "Replace the remote path if it already exists"; `--concurrency` "Defaults to
  `32`"; and "`railway volume files delete` refuses to run when invoked by an AI agent and must be
  run by a human."
- `docs.railway.com/cli/restart`: "restarts the service without rebuilding it. The existing
  deployment image is reused."
- `docs.railway.com/guides/roll-back-bad-deploy`: "A rollback on Railway restores a previous
  deployment's Docker image and its custom variables". Volume contents are not mentioned. The
  plan's inference, which is not a quoted fact, is that a rollback leaves the volume as it is.
- `docs.railway.com/volumes/backups`: daily backups are "kept for 6 days", "Wiping a volume deletes
  all backups", and "Backups can only be restored into the same project + environment."
- In the tree: `.railway/railway.ts:58` mounts `willikins-volume` at `/data` (5000 MB), and the
  README puts the journal at `/data/journal.jsonl`.

**Where the documents live.** There is one volume per service and the journal already holds it,
so the documents share it. `WILLIKINS_WORKFLOWS_DIR=/data/workflows` becomes a service variable,
kept by a `WILLIKINS_WORKFLOWS_DIR: preserve()` row in `.railway/railway.ts`. The layout of `/data`
is fixed here, so that no two tenants of the one volume can collide:

| Path | Holds | Written by |
| --- | --- | --- |
| `/data/journal.jsonl` | the journal (`WILLIKINS_JOURNAL_PATH`) | the server |
| `/data/workflows/` | the documents: flat, `.yaml`/`.yml` only, never descended into | the operator, by hand |
| `/data/workflows.next/` | staging for the next document set; the server never reads it | the operator, by hand |
| `/data/credentials/` | the passkey and recovery-code store (`WILLIKINS_CREDENTIAL_STORE_PATH`) | the server and the enrollment CLI |
| `/data/clients.yaml` | the pre-registered client table (`WILLIKINS_CLIENTS_PATH`) | the operator, by hand |

The credential store and the client table must never sit inside `workflows/`: `Butler::start` scans
that directory flat and refuses on the first YAML that is not a document. The path is **not**
derived from `RAILWAY_VOLUME_MOUNT_PATH`. Deriving it would give the server a second way of
choosing its trusted directory, and an unset `WILLIKINS_WORKFLOWS_DIR` should stay the
`ConfigError::Missing` it is today (`crates/willikins-server/src/config.rs`). Startup only *warns*,
by name, when `RAILWAY_VOLUME_MOUNT_PATH` is set and the workflows directory is not beneath it.

**The image carries no workflow document** (task 20). The `COPY workflows/*.yaml /app/workflows/` and
the `ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows` both go. `.dockerignore` gains `workflows`,
`private` and `crates/willikins-cli/tests/operator_*`. Railway's GitHub builds never see the last
two, because they are gitignored. But `railway up` "Respects your `.gitignore` file" only unless it
is given `--no-gitignore`, and a local `docker build` from the operator's checkout would put
`private/` into the builder's `COPY . .` layers. The public `workflows/*.yaml` stay in the
repository as examples and as positive fixtures. **Why the default must go rather than stay as a
fallback:** `.railway/railway.ts`'s own rule is that an omitted field is an instruction to delete
it. So one railway.ts edit that forgets the `preserve()` row deletes the service variable. With the
image's ENV still in place, the server would then quietly start on the seventeen public samples,
with real credentials. Without the ENV it refuses to start, which is the only acceptable outcome.

**The cutover for task 20 happens before its commit is pushed**, because a push to `main`
auto-deploys onto the live (private, fake-catalog) service. In order:

1. The operator creates `/data/workflows` on the volume and publishes the current public
   `workflows/*.yaml` set into it, by the procedure below.
2. The operator sets `WILLIKINS_WORKFLOWS_DIR=/data/workflows` on the service.
3. Task 20's commit lands with the `preserve()` row, after a read-only `railway config plan` that
   shows no delete.

Go-live step 5 later replaces that interim set with the operator's private one.

**What makes a volume directory trusted.** The invariant used to be "run only from a trusted ref",
and on the image the ref was the built commit. On a volume the documents have no ref, and the
process that reads them could also write them: it runs as root, and the volume is writable. So the
trust root is re-founded, and it holds by these rules together:

- **The source of truth is the operator's private checkout at a reviewed commit.** The volume is a
  copy of that commit's top-level documents, nothing else. That means no fake-state file, no
  fixture and no test.
- **Only the operator writes the volume, by hand, from their own machine.** Railway account access
  is the same trust level as setting a service variable or wiring Doppler's integration, which is
  consistent with the design doc's network trust boundary. **An agent never writes the volume.** No
  agent runs `railway volume files upload`, `rename` or `browse`, `railway ssh`, `scp` or `sftp`
  against the service. Railway's own guard covers only `files delete`, so this is a written rule
  (task 25), not a platform guarantee.
- **The server never writes `workflows/`, and there is no HTTP upload endpoint.** An upload endpoint
  would be a new privileged surface: it would need its own scope, its own approval and its own pass
  3 rows. It is not built, and a later milestone that wants one starts from that list.
  `validate` and `describe` bodies are never persisted.
- **Every plan pins the root document's sha256 and its `used` closure** (milestone 2b), and `apply`
  and `plan_rollback` refuse `DocumentChanged` on any difference. `uses:` children resolve by name
  in the same directory (`StartupError::Compose`), so the whole composition closure must be on the
  volume, and each child is pinned too. Templates are document literals, so this covers them as
  well.
- **The approvals page shows the root's short sha beside the used closure, and shows the document's
  description only while the file still has the planned bytes** (task 23). The approver can then
  compare the page with the tag's manifest. Today `document_says`
  (`crates/willikins-server/src/http/approvals.rs:107`) re-reads the *current* file at render time.
  So after a re-publish, a plan built from version A would display version B's description, and
  `record.document_sha256` is rendered nowhere.
- **`ServerStarted` journals the whole set's hashes at each start**, and every publish ends in a
  restart. So every document set that ever served appears in the journal.

**A file that changes after startup gets the same refusals as one present at startup, and the set
validated at startup is not authoritative.** That is today's behaviour, and it is kept on purpose.
There is no cache and no watcher. `Butler::list_workflows` rescans the directory on every call
(`crates/willikins-server/src/butler.rs:222`). `plan`, `validate`, `describe`, `apply` and
`plan_rollback` re-read by name and run `check`. A file added after start is therefore refused on
the first call that reads it, by exactly the rules startup would have applied (test 28 places a
negative fixture after start and asserts it cannot be planned). Treating the startup set as
authoritative would mean caching document bytes, which buys nothing over the pinning above. The
costs are recorded rather than designed around:

- One broken or half-uploaded file makes `list_workflows` fail for every document, and makes the
  next restart refuse to start. The service is shell-less, so that outage is fixed by re-publishing,
  never by hand-editing in place. The procedure's staging directory and checksum comparison exist
  to prevent it.
- A pending plan whose closure changed is refused at apply and must be re-planned.
- A rollback of a run planned on an older set is refused (`DocumentChanged`) until that tag's set
  is published again: publish the old set, roll back, then publish the current set.

**Three code changes harden the trusted directory now that it can change under a running server**
(task 21). Each is test-first.

- **The directory itself must not be a symlink.** `scan_directory` calls `std::fs::read_dir(dir)`,
  which follows `dir` when it is one, and only each entry's own file type is checked. So at start
  and on every scan, a `WILLIKINS_WORKFLOWS_DIR` whose `symlink_metadata` is not a plain directory
  is refused, reusing `StartupError::Symlink`. Each document is opened with `O_NOFOLLOW`, and
  `is_file` is checked on the opened handle's metadata, which closes the gap between
  `load_named_document`'s `symlink_metadata` and its open.
- **The startup scan hashes exactly the bytes it parsed.** `scan_directory` parses through
  `willikins_dsl::load_document` and then hashes a second `std::fs::read`
  (`crates/willikins-server/src/startup.rs`, the scan loop). `document.rs` already fixed that
  window for named loads in 2b's adversarial pass, and on an immutable image it was unreachable.
  On a volume it is reachable. The scan moves to `load_document_with_bytes`.
- **Startup logs each document's file name with its short sha**, and logs a named warning when the
  directory holds zero documents.

**The operator's procedure, human only.** The commands are the operator's; no agent runs any of
them.

1. Run `willikins documents check <dir>` (task 22, built from the *deployed* willikins commit) over
   the private checkout at the reviewed tag. It runs exactly `scan_directory` and prints a manifest
   in `shasum -a 256` format, which is the same data as `ServerStarted.workflow_hashes`. Commit the
   manifest beside the tag, privately.
2. Copy only the top-level documents into a clean local staging directory.
3. `railway volume files upload <staging-dir> /workflows.next`. `railway volume files` paths are
   relative to the volume root, so the container's `/data/workflows.next` is `/workflows.next` to
   it (verify item 24).
4. Download `/workflows.next` and compare `shasum -a 256` against the manifest.
5. `files rename /workflows /workflows.prev`, then `files rename /workflows.next /workflows`. If
   renaming a directory is unsupported or not atomic (verify item 24), upload file by file with
   `--overwrite` into `/workflows` and delete removed files by hand, which Railway's guard makes
   human-only anyway.
6. `railway restart`, then confirm that the startup log's names and shas match the manifest.
7. Re-plan anything that now refuses `DocumentChanged`.

The fallback transport is `scp` or `sftp` to the container path `/data/workflows`, addressed by
the service instance id while the service has no domain. Whether either works into a distroless
container is verify item 29.

**The binary and the documents now change version independently.** The binary deploys on a push to
`main`; the documents arrive by publish. A catalog or DSL change that invalidates a private document
makes the next start refuse, and under the `ON_FAILURE` restart policy that is a crash loop, on top
of the volume's redeploy downtime. So two checks become part of every change:

- Before a push that changes the catalog or the DSL, run that commit's `willikins documents check`
  over the private set.
- Before any publish, run the deployed commit's check over the new set.

**Backups.** A volume-backup restore is never the way to recover documents. It rewinds the journal
and the credential store to the backup point as well. Documents are recovered by re-publishing the
tag. Volume backups exist for the journal, the credential store and the client table (go-live step
9).

**The test and fake path.**

- Tests keep using temporary directories, and no test touches `/data`.
- A fake-catalog deployment starts with an empty `/data/workflows` (logging the warning), or with
  the public set published by the procedure above.
- A local container mounts `-v "$PWD/workflows:/data/workflows:ro"` with
  `-e WILLIKINS_WORKFLOWS_DIR=/data/workflows`, and only on a host that is allowed to build images.
- The CLI's `--workflows-dir` is unchanged and points at the private checkout.

**Must never happen**, gathered so a reviewer can check them in one place:

- A private document in the public repository, the image or the build context.
- An agent writing to the volume.
- The server following a symlink out of the volume, whether at the directory or at an entry, or
  descending into a subdirectory.
- A fallback document path of any kind, or any write by the server to `workflows/`.
- A volume-backup restore used to "fix" documents.
- A non-document YAML inside `workflows/`.
- Approving a plan during a publish without checking its shas against the manifest.

**The invariant's wording moves with it** (task 25, edited together in `CLAUDE.md` and `AGENTS.md`,
plus a design-doc addendum beside `docs/plans/2026-09-11-willikins-design.md`'s "a checkout of a
trusted ref"). The proposed text: "Workflow documents and templates are privileged content: they run
only from a source the deployment's operator controls — a checkout of a trusted ref locally, or on a
deployment the mounted directory the operator populates from one — and every plan pins their
sha256." `todos/2026-09-20-agent-authored-workflows.md` already records the operator's view that
"What counts as the trusted ref is a property of a deployment". The wording is still the operator's
to accept (open question 3).

*Pinned by tests 27, 28, 29 and 30, and by test 19's volume checks.*

## The pluggable seam

The operator banked pluggable authentication as a later improvement and this milestone builds one
thing: the in-house implementation. The seam exists so that adding an adapter later is an addition
rather than a rewrite, and it deliberately **adds no abstraction the in-house path does not need** —
no `dyn Trait` for authenticate-a-human, no adapter module, and no configuration switch. The todo is
`todos/2026-09-16-pluggable-auth-adapters.md`, and its rule is the one sentence this section exists
to enforce:

> The validator must never shortcut to the in-house signing key. It reads the configured issuer and
> JWKS like any other, even when both are its own.

Three layers, nesting rather than conflicting, in order of how much code they cost.

1. **A data seam that already exists and costs nothing.** The issuer identifier, the key-set
   location and the RFC 9728 document's `authorization_servers[]` array — the one configuration
   block decision 10 isolates. Today willikins fills all three with its own values; an adapter
   fills them with someone else's. RFC 9068 §4 confirms this is the mechanism the specification
   intends: authorization servers "SHOULD use OAuth 2.0 Authorization Server Metadata to advertise
   to resource servers their signing keys via `jwks_uri` and what `iss` claim value to expect via
   the `issuer` metadata value."
2. **A code seam one function wide.** `oauth::validate(token) -> Result<Claims, TokenRejection>`,
   returning subject, issuer, client id, scopes and expiry. `/mcp` and the approvals session both
   go through it and nothing downstream knows who issued. Beside it, `JwkSource` — one method,
   "where does the key set come from" — with the two implementations it will ever have. This is a
   shipped design and not a hopeful one: the MCP TypeScript SDK splits at precisely this line, with
   an `OAuthTokenVerifier` described in its own comment as a "slim implementation useful for token
   verification".
3. **The issuer's own input, as a function boundary and not a trait.** `mint(sub, scopes, aud)`.
   The in-house path needs that signature anyway; a `dyn` behind it is what an adapter adds, not
   what 2c ships. (The authorization-server research note proposed a trait here; the scope revision
   overrules it as premature, and this plan follows the revision.)

**Nine things must be true now, or the adapter is a rewrite.** Each is already a decision above;
they are gathered here so a later reader can check them in one place.

1. **Publish `jwks_uri` and serve a JWKS**, even though RFC 8414 makes it OPTIONAL for a co-hosted
   authorization server (decision 19). A resource server that only ever knew how to read a key out
   of its own process *is* the rewrite. This is the single most important row here that a reader
   would otherwise cut as unnecessary.
2. **Never let the validating half skip a check because it minted the token** (decisions 1, 7). The
   fixed-order RFC 9068 checks run identically for in-house and foreign tokens, and the
   foreign-issuer fixture is what *asserts* it — which is why that fixture is kept rather than
   deleted along with the external provider. Test 3 is the assertion.
3. **Abstract the key-set *source*, not the fetch** (decision 16). The overlap window differs by
   implementation and the decision says so: in-process it is exactly the maximum token lifetime;
   with an external provider the key-set cache TTL adds to it.
4. **Keep `iss` in the principal derivation** (decision 2), so an adapter's subjects cannot collide
   with willikins' own enrollment identifiers.
5. **Keep both subject allowlists** (decision 3). They are issuer-independent, and an external
   provider's `sub` needs them exactly as much as an in-house one does.
6. **Keep the validator's required claim set at four, not seven** (decision 17). The issuer emits
   seven; a validator demanding seven would refuse an adapter's token.
7. **Keep the accepted `typ` set two-valued** (decision 14). An adapter's issuer may emit
   `application/at+jwt`.
8. **Decide `SameSite` once, in writing, as a seam constraint rather than a cookie detail**
   (decision 4). An adapter's cross-site callback landing needs `Lax`; an in-house-only deployment
   might take `Strict`. Do not take `Strict` now and rediscover this later — and verify item 7 is
   the measurement that would have to come first.
9. **Budget one `skipLocalPkceValidation`-shaped flag** for the proxy adapter shape. In a
   production SDK that flag is the *entire* delta, so it is cheap to design in and annoying to
   retrofit.

**Two adapter shapes exist and prior art ships both**, and neither is designed now. Keep willikins
as the token issuer and add a grant that trusts an external assertion — which is MCP's own
standards-track enterprise story, splitting into a resource authorization server that issues the
tokens and an identity-provider authorization server used for single sign-on — or proxy the
endpoints upstream, the TypeScript SDK's `ProxyOAuthServerProvider` shape. **The first is the
todo's default**, because it leaves the resource-server half untouched, and
`draft-ietf-oauth-identity-assertion-authz-grant` is the document to read before freezing anything.

**What does not port, written down rather than discovered.** Passkeys are bound to the RP ID
(decision 12), so switching to an external provider orphans every credential and switching back
costs a re-enrolment each way — a price, not a barrier, and the same price a change of host costs. Pre-registered client ids are per-authorization-server — MCP says clients
"MUST maintain separate registration state per authorization server and MUST NOT assume that
credentials valid for one authorization server will be accepted by another" — so every client
re-registers the day the advertised server changes; CIMD ids would port, because they are
self-hosted URLs resolved on demand, which is the one real argument for CIMD that is not about
convenience. And the credential store, the signing key and their backups are history, not protocol:
the seam makes the protocol swappable and it does not make the history portable.

## Pinned dependencies (new)

Caret requirements on the major; `Cargo.lock` pins the rest. Every version, licence and MSRV cell
below is quoted in `docs/research/2026-09-16-m2c-own-authorization-server.md` §8 from a source
fetched on 2026-09-16. Each licence and MSRV was checked against
`crates.io/api/v1/crates/<name>/<version>` at the exact version pinned here, and every one was
measured against the workspace's **declared** MSRV of 1.88 (`Cargo.toml:21`), not against the 1.97
toolchain the Dockerfile builds with. **No cargo command was run during either research pass**, so
every claim about what the image builds is dependency-graph inference from a fetched manifest and
is task 1's to settle.

**Restated against the tree of 2026-10-20.** Since this table was written, the App Store Connect
provider has brought several of these crates in. The rows below say which crates are already
workspace dependencies and which are new, and the lock facts are read from the current
`Cargo.lock`.

**Already in the workspace, so 2c adds no requirement for them.**

| Crate | Where it is today | What 2c uses it for | Risk |
| --- | --- | --- | --- |
| `jsonwebtoken` | `Cargo.toml:75`: `{ version = "11", default-features = false, features = ["rust_crypto", "use_pem"] }`, locked at **11.1.0**, used by `willikins-providers-http` | Signs (`encode`), publishes the JWKS (`Jwk::from_encoding_key`), derives the `kid` (`thumbprint`) and validates: the whole issuing half for **zero new crates**. MSRV 1.88, the workspace floor, with zero headroom | "Two silent traps (§3.3): `kid` is `None` by default and `JwkSet::find` skips such keys; Ed25519 needs PKCS#8 **v1**." `rust_crypto` drags `rsa` in unconditionally, and **it already does in the shipped binary** |
| `p256` | `Cargo.toml:74`: `"0.13"` (0.13.2 locked; `p384` 0.13.1 beside it), a dependency of `willikins-types` | ES256 key generation into PKCS#8 (`SecretKey::random`, `to_pkcs8_pem`), the same `EncodePrivateKey` path `appstore.rs` already uses | Binds `rand_core` **0.6**, which the workspace's `rand = "0.9"` does not satisfy (next row) |
| `rand_core` | `crates/willikins-providers-github/Cargo.toml:47`: `{ version = "0.6", features = ["getrandom"] }`, used as `rand_core::OsRng` in `seal.rs` | The CSPRNG handed to `p256`'s key generation. The issuing crate declares the same requirement the GitHub provider does | None new: the lock already holds `rand_core` 0.6.4. **This replaces the earlier `rand = "0.8"` row**: a direct `rand` 0.8 was never needed, only its `rand_core` |

**New in 2c.** The risk column is the research note's own, carried across wherever it names a trap.

| Crate | Requirement | Licence / MSRV | What it does here | Risk |
| --- | --- | --- | --- | --- |
| `webauthn-rs` (+ `-core`, `-proto`) | `0.5.5` | MPL-2.0; MSRV 1.88 = the workspace floor | Registration and authentication ceremonies, `Passkey` persistence, UV Required, counter-regression detection. It is the only serious Rust relying party there is | "`webauthn-rs-core` depends **unconditionally** on `openssl` + `openssl-sys` (§4.2). No Related Origin Requests, so the RP ID can never change. Ceremony state must be server-side" |
| `webauthn-authenticator-rs` | `0.5.5`, **dev-dependency only** | MPL-2.0; MSRV 1.88 | `SoftPasskey` drives the ceremony tests with no browser; `new(falsify_uv)` and its own counter are what make the UV-lie and counter-regression cases testable | Pulls tokio, hex and serde_bytes into dev-dependencies only |
| `argon2` | `0.6.0` | MIT OR Apache-2.0; MSRV 1.85, edition 2024 | Hashes the stored recovery codes (decision 24). It is **not** a password-only dependency: it arrives on the passkey branch too | "`Argon2::default()` is already OWASP's config — pin it with a test rather than trusting the default to stay put. 19 MiB **per concurrent hash** makes an unauthenticated login endpoint a memory-exhaustion surface without a semaphore" |
| `password-hash` | **`0.6.1`** — 0.6.0 is **yanked** | MIT OR Apache-2.0; MSRV 1.85 | PHC string encoding, so a later parameter increase is a per-hash migration rather than a flag day | Low |
| `axum-extra` | `{ version = "0.12", default-features = false, features = ["cookie-signed"] }` (0.12.6) | MIT | The approvals session cookie and its jar | "The jar must be **returned** from the handler or `Set-Cookie` is silently dropped (§5.7) — assert the header in tests. A startup-generated `Key` makes sessions replica-local" |

**Carried unchanged from the delegating draft**, because tasks 14 and 17 are untouched by this
revision and still need them:

| Crate | Requirement | Why |
| --- | --- | --- |
| `hyper` | `{ version = "1", default-features = false, features = ["server", "http1"] }` (1.11.1 is already in the lock, pulled by axum) | Task 17's accept loop builds on `hyper::server::conn::http1::Builder` directly; it becomes a **direct** dependency of `willikins-server` rather than a transitive one |
| `hyper-util` | `{ version = "0.1", default-features = false, features = ["server", "http1", "tokio", "service"] }` (0.1.20 is already in the lock) | `rt::TokioIo`, `rt::TokioTimer` (decision 8: hyper 1.11 panics without a timer) and `service::TowerToHyperService`. **Not `server-auto`**, which would add HTTP/2 to a release binary that speaks only HTTP/1.1 |
| `tracing-subscriber` | already a workspace dependency at `0.3`; task 14 **adds the `env-filter` feature** to the existing `features = ["json"]` | `WILLIKINS_LOG` is a filter directive and `EnvFilter` is behind that feature |

**`ureq` stays a dev-dependency of `willikins-server`.** The delegating draft moved it into
`[dependencies]` for the JWKS fetch; decision 16 removes that fetch, so the move is withdrawn and
the crate keeps the position it has today.

**The `jsonwebtoken` backend is already chosen: `rust_crypto`** (corrected 2026-10-20; the earlier
text asked task 1 to "build both"). Since 10.0.0 `jsonwebtoken` requires exactly one crypto backend,
and the workspace already enables `rust_crypto` for App Store Connect. Cargo unifies features across
the workspace, so switching 2c to `aws_lc_rs` would switch it everywhere. That is not a 2c-local
choice. Its price, recorded so it is not re-proposed by accident:

- `apple_credential.rs` and `appstore.rs` would be re-tested on the other backend.
- The builder would need `g++` (the fetched README says CMake, bindgen and Go are never required
  for a non-FIPS build and a C/C++ compiler is; the builder installs `gcc` only, deliberately
  avoiding `build-essential`).
- The Dockerfile's "no `aws-lc-sys`" comment would become false.

What it buys is `rsa` leaving the tree. So task 1 **records** `rust_crypto` and does not build both.

**RUSTSEC-2023-0071 is a today problem, not a 2c one.** `rsa` 0.9.10 is already compiled into the
shipped binary through `rust_crypto`, with `patched = []`. The advisory is a private-key
signing-timing leak observable over the network. The delegating draft dismissed it because "a
resource server only verifies with public keys". That dismissal was void before 2c signed anything,
because the App Store Connect provider signs (ES256, so the advisory's RSA path is not exercised).
What justifies ignoring it, today and after 2c, is a normative constraint: willikins never signs
with the RSA family (decision 22). A future `cargo-audit` or `cargo-deny` gate therefore needs a
documented ignore with that justification, rewritten rather than reused.

**The Dockerfile questions that remain are OpenSSL's.** Two facts arrive with `webauthn-rs`:

- The builder needs **`libssl-dev` and `pkg-config`**.
- The Dockerfile's own comment claiming no `aws-lc-sys`, `openssl-sys` or `cmake` anywhere in the
  tree (`Dockerfile:41`) becomes **false** for `openssl-sys` and must be rewritten.

Whether the runtime image (`gcr.io/distroless/cc-debian12`) ships the `libssl.so.3` that
`openssl-sys` 0.9.114 links against is the weakest citation in either note, and it is
**build-blocking**. It is verify item 2, task 1's to settle, which is why task 1 runs first.

**Rejected**, each for a fetched reason:

- **`oxide-auth` 0.6.1** (with `oxide-auth-async` 0.2.1 and `oxide-auth-axum` 0.6.0) — the only
  established general-purpose Rust authorization-server framework, and it supplies none of RFC
  8414, JWKS, RFC 7591, RFC 8707 or `at+jwt`; its `Grant` has no audience field at all, which is
  the single filter that eliminated the hosted providers too. 18,732 lines across three crates and
  three duplicate majors to obtain roughly 1,500 wanted lines, with PKCE opt-in and `plain`-
  defaulting and semantic redirect matching by default. It was **not** disqualified on
  compatibility: `oxide-auth-axum` declares `axum ^0.8`.
- **`oauth-as` 0.9.4** — the closest fit in the ecosystem and the reason to read it rather than
  take it. It implements everything decisions 19 to 23 require, and it is six weeks old, one
  author, one star, no audit, 36,230 lines of security-critical source, with an ES256-only signer
  that would freeze the algorithm. **Rejected as a dependency and named as prior art**: 36,230
  lines of unreviewed security-critical source by one author is a larger trust surface than the
  product the operator rejected. Re-evaluate on 1.0, a second maintainer, or an audit.
- **`authkestra-op` 0.11.1** — non-optional dependencies on three sibling `authkestra-*` crates, so
  adopting it means adopting authkestra.
- **`mcp-oauth` 0.3.0** — declares `rust_version` 1.92, above the workspace MSRV of 1.88.
  Disqualified before any other consideration.
- **`tower-sessions` 0.15.0** — declined, and **not** for the reason the resource-server note gave:
  its `memory-store` *is* a default feature. The four reasons that survive are in
  `…-m2c-own-authorization-server.md` §5.1; its cookie defaults and its `create`-time
  id-collision check are copied instead.
- **`axum-login` 0.18.0** — no release in 14 months, and it pins `tower-sessions` 0.14 against a
  current 0.15 carrying a memory-ordering fix. Two ideas are copied and the crate is not:
  `cycle_id()` at login, and a constant-time `auth_hash` compare for credential-change revocation.
- **`passkey-authenticator` 0.5.0** — an alternative test authenticator that would keep openssl out
  of the *dev*-dependency graph but not out of the build, since `webauthn-rs-core` needs it anyway.
- **`reqwest` 0.13.5** — a second async HTTP+TLS stack in an image that has none.

Carried from the resource-server note, each still rejected for its own fetched reason: `oauth2`
5.0.0 (every bundled HTTP client mismatches this tree), `openidconnect` 4.0.1 (23 non-optional
dependencies duplicating four majors, and it pulls `rsa`), `jwt-simple` (BoringSSL by default,
audience unchecked by default), `jwks-client` (one release, 2020). One of that list needs its
reason restated rather than repeated: **`josekit` was rejected for its non-optional `openssl`**,
which is no longer a disqualifier on its own now that `webauthn-rs-core` brings openssl in anyway —
what survives is that it buys nothing `jsonwebtoken` 11 does not already do.

`oauth2`'s rejection does not rest on a version-unification claim. An earlier draft said its
`base64 >=0.21, <0.23` bound "cannot unify with the tree's 0.23.1"; that is not how cargo works —
two semver-incompatible majors coexist in one build, which this tree already demonstrates
(`Cargo.lock` holds `rand` 0.8.8, 0.9.5 **and** 0.10.2, and three majors of `getrandom`). What duplicate
majors actually cost is build time and audit surface, and that is recorded under Risks.

## Crate contracts

### willikins-server (the bulk of the milestone)

Two halves in one process, with one rule between them: **the issuing half hands the validating half
nothing but a key set and a token**. The validating half never learns that willikins minted what it
is checking.

**New module `src/http/oauth/`** — the validating half, replacing the body of
`http::auth::bearer_auth` at the seam that already exists. The order stays: timeout → tracing → the
auth middleware → rmcp's `StreamableHttpService` → rmcp's host check → dispatch. rmcp offers no
server-side authorization hook in 3.3.0 or 3.4.0 and needs none; `StreamableHttpServerConfig` has
ten fields and not one of them is an authorization point.

- `oauth::config::OAuthConfig` — the single configuration block of decision 10, built by
  `ServerConfig::from_vars` and validated at startup. It is the seam's data layer: an issuer
  identifier, a key-set source and the `authorization_servers[]` value, nothing provider-shaped.
- `oauth::jwks::JwkSource` — **one method**, "where does the key set come from", with exactly two
  implementations it will ever have: the in-process one this milestone ships, and the
  fetch-and-cache an adapter would add (decision 16). There is no `JwkCache` and no `ureq` on the
  validation path.
- `oauth::validate::validate(token, &OAuthConfig, &dyn JwkSource) -> Result<Claims, TokenRejection>`
  — checks 0 to 7 of decision 1, in order, each mapping to one `TokenRejection` variant and one
  `AuthFailedReason`. Pure over its inputs plus the key set, so the whole validation matrix is a
  unit test as well as an end-to-end one. **It has no "did I mint this" branch and never will.**
- `oauth::middleware::require_token` — extracts the header and nothing else (a query parameter is
  not read), validates, checks the `sub` against `WILLIKINS_AGENT_SUBJECTS`, derives the principal
  (decision 2), inserts the `Principal` and the granted scope set into the request extensions,
  **removes the `Authorization`, `Cookie` and `Proxy-Authorization` headers**, and calls
  `next.run`. Only then does it buffer and parse the body for the scope check (decision 3). On
  refusal it journals through `Butler::record_auth_failure` and answers decision 1's challenge.
- `oauth::scope` — the four scopes, the hierarchy, the `scope`/`scp` claim readers, and the
  tool-to-scope table of decision 3, shared with the consent screen so the two cannot disagree
  about what a scope admits. The table carries the rollback rows ahead of their tools, and test 7
  ties it to the snapshotted tool list, so a `#[tool]` without a row fails the gate.
- `oauth::metadata` — **both** documents (decision 19) as `#[derive(Serialize)]` structs with
  `skip_serializing_if = "Vec::is_empty"` on every array field, plus their routes and the
  `/jwks.json` route, registered on the root router outside both the auth middleware and rmcp's
  nest, exactly as `/healthz` is.

**New module `src/issuer/`** — the issuing half.

- `issuer::key` — ES256 generation, PKCS#8 loading, the RFC 7638 thumbprint that is the `kid`,
  the `JwkSet` assembly that sets that `kid` on the published JWK too, and the two-key overlap
  (decision 22). The private key is the `willikins-types` secret type described below: redacted
  `Debug`, no `Display`, no `Serialize`, zeroized, and readable only through its own
  `reveal_for_signing`.
- `issuer::mint(sub, scopes, aud) -> Jwt` — the issuer's whole input surface, **a function
  boundary and not a trait**. It emits all seven RFC 9068 REQUIRED claims and sets `typ` explicitly
  (decision 22).
- `issuer::clients` — the pre-registered client table and the redirect matcher of decision 23,
  including the loopback port exception.
- `issuer::codes` — the authorization-code store of decision 21: the four stored bindings, the
  atomic get-and-remove claim, and the independent expiry check.
- `issuer::authorize` and `issuer::token` — the two endpoints, their two-phase error split, the
  consent screen and its own CSP, the parked-request store for an `/authorize` with no session,
  and `state` echoed beside `iss` on every response including errors (decisions 20, 21).

**`http::approvals`** gains the human half: the passkey enrollment and authentication ceremonies,
the durable credential store, the ceremony-to-account binding, the uniform username step, the
inline script under a `script-src` nonce, logout, and the mint-then-validate hop that turns a
finished ceremony into a session (decision 4). The session itself lands first, as library code, in
its own task: the signed `__Host-` cookie and jar, both bounded stores, unconditional id rotation,
the absolute and idle timeouts, `no-store`, 128-bit ids, the non-persistent cookie,
credential-change revocation, `X-Frame-Options`, the CSP, `Sec-Fetch-Site` and `Vary`, and
pinning tests for the origin check's exact-match and both-absent behaviour, which already ship
(decision 4). The nonce store, the Origin/Referer check and the body cap are untouched apart from
task 13. Two additions from the 2026-10-20 addendum also land here. Pending **teardowns** get
their own section and nonces (task 24, decision 4). And each plan section shows the root
document's short sha, and shows "document says" only while the current file still has the
planned sha (task 23, decision 25). **There is no `/approvals/callback`, no `state` store, no `code_verifier` and
no token exchange**: the approvals page is no longer an OAuth client of anyone, willikins included.

**`startup` and `document`** gain decision 25's hardening (task 21). `scan_directory` refuses a
`WILLIKINS_WORKFLOWS_DIR` that is itself a symlink, reusing `StartupError::Symlink`. It hashes
exactly the bytes it parsed, through `load_document_with_bytes`. And it logs each file name with
its short sha, plus a named warning for an empty directory, or for one outside
`RAILWAY_VOLUME_MOUNT_PATH` when that is set. Named loads open with `O_NOFOLLOW` and check the
opened handle. Nothing in the server writes under `WILLIKINS_WORKFLOWS_DIR`.

**`http::auth`** loses `bearer_auth`'s hash comparison and `basic_auth` entirely. `TokenHash`,
`matches_any` and the constant-time compare go with them; nothing else in the crate uses them.
They leave in two pieces, in the tasks that replace what each half fed — see decision 6.

**`http::config`** gains the `HttpConfigError` variants of decisions 6, 7 and 15. `cli.rs`'s
`StartError` gains the **runtime** refusals, for the reason decision 6 gives: `HttpConfigError` is
`Copy` and is produced by the pure `HttpConfig::build` before the tokio runtime exists, so a
refusal that needs the runtime or the filesystem cannot live there. Those refusals are **no signing
key configured and none can be generated or written**, and **a credential store that cannot be read
or written**. `StartError::JwksUnavailable` is **not** among them: decision 16 removes the startup
JWKS fetch, so the variant the delegating draft introduced has no producer and is not added.
`startup::StartupError` gains no variant: its eight variants (2b added `Compose`) are the
trusted-directory refusals, and decision 25's symlinked-directory refusal reuses `Symlink`.

**`mcp.rs`.** `principal_for` reads the `Principal` the OAuth middleware inserted — id plus claims,
not a bare `PrincipalId` — and every `#[tool]` method passes it on to the `Butler` method it calls,
which is the signature change decision 2 describes. It does **not** gain a scope refusal, because
an authorization refusal is a transport-level status in the specification's table rather than a
domain error, and a tool result cannot carry one (decision 3).

### willikins-types

One secret type and two plain ones. The **signing key** is a `#[domain(secret)]` type modelled on
`AppleSigningKey`. It accepts PKCS#8 PEM, as `appstore.rs` already canonicalises for
`EncodingKey::from_ec_pem`, and its bytes are readable only through its own `reveal_for_signing`.
That is the sixth token-less `expose_secret` exception, named in `clippy.toml` and in
`crates/willikins-core/tests/it/expose_secret_guard.rs`, and called only from `issuer::mint`
(trust boundary 2). It never implements `Display` or `Serialize`, and the type registry refuses it
as a literal or input like every other secret type. `OAuthSubject` and `ClientId` are plain,
non-identifier domain types, for the reason decision 2 gives. Because `willikins-types` enables its
own `executor` feature through a self dev-dependency, the fourth gate (`cargo check -p
willikins-types -j 2`) is what proves the new type builds the way its dependents see it.

### willikins-journal

The new `AuthFailedReason` variants land in two batches, both additive. The validating half's
variants land in **task 4**, with the middleware that produces them, because tests 3 to 5 assert
them; the claim fields land in task 6. The issuing half's variants land with the endpoints that
produce them, in tasks 9, 10 and 11:

| Refusal | Produced by |
| --- | --- |
| `UnknownClient` | `/authorize` phase 1 (decision 20) |
| `RedirectUriMismatch` | `/authorize` phase 1 (decision 20) |
| `MissingCodeChallenge` | `/authorize` (decision 20) |
| `UnsupportedCodeChallengeMethod` | `/authorize`, including `plain` (decision 20) |
| `PkceVerifierMismatch` | `/token` (decision 21) |
| `CodeReplayed` | `/token` (decision 21) |
| `CodeExpired` | `/token` (decision 21) |
| `ResourceWidened` | `/token` (decision 21) |
| `CeremonyAccountMismatch` | the passkey ceremonies (decision 24) |
| `RecoveryCodeReplayed` | the recovery path (decision 24) |

Every one of them is reachable by an **unauthenticated** request, so every one passes through
decision 1's per-reason token bucket and carries its `suppressed` field when coalesced. That is the
same machinery, not a second one.

Three new **optional** fields — `subject`, `issuer`, `client_id` — on every event that carries a
`principal`, each bounded and escaped before it is written (decision 2), plus two more optional
fields on `AuthFailed` alone: `subject`, written only for the refusal of a token that validated,
and `suppressed`, written only on a coalesced line (decision 1). No existing field changes type,
**no variant is removed** — `InvalidCredential` and `MalformedUsername` stay in the enum and stop
being produced, because `pre-pass-2-every-event.jsonl` carries both and removing a variant would
break the replay the whole discipline exists to guarantee — and no field becomes required.

**The events that carry a principal**, by name, so the claim fields cannot miss one. `ToolCalled`,
`PlanRecorded`, `ApprovalGranted` and `ApprovalRejected` (field `approver`), `ApplyRefused`,
`RunStarted`, and 3m's `TeardownPlanned` and `TeardownStarted`. The three claim fields land on all
eight.

**One read-side view is added**: `pending_teardowns`, beside `pending_approvals_view`
(`crates/willikins-journal/src/journal.rs:295`), folding teardowns that need a decision the same way
that view folds plans (decision 4, task 24). It is a fold over existing events, with no new
event and no new field.

Three fixtures replay unchanged through `Journal` and through `willikins_journal::replay`: both
existing ones and the new `pre-2c-every-event.jsonl`. `post-pass-2-new-shapes.jsonl`, which pass 2
froze as "the next pass's baseline", is that baseline. `pre-2c-every-event.jsonl` supplements it:
it is cut from the current binary before task 3 changes anything, and it carries **every** `Event`
variant, including `TeardownPlanned`, `TeardownStarted`, `TeardownStepFinished` and
`TeardownFinished`.

**How the new fixture is produced.** Both existing fixtures came from an `#[ignore]`d generator
test in `willikins-journal` — `regenerate_the_frozen_fixture`, one in
`crates/willikins-journal/tests/it/pre_pass_2_replay.rs:311` and one in
`crates/willikins-journal/tests/it/post_pass_2_shapes.rs:217` (both moved under `tests/it/` by 3k),
each writing to
`$WILLIKINS_FIXTURE_OUT` and each kept in the tree as the fixture's provenance rather than as a way
to refresh it. `pre-2c-every-event.jsonl` is generated the same way, by the same shape of test, and
carries the same "never point this at the committed file" warning.

### willikins-cli

`hash-token` is removed. **Two subcommands arrive.** The second is `willikins documents check
<dir>` (task 22, decision 25). It runs exactly the server's `scan_directory` over a directory and
prints a `shasum -a 256`-format manifest that equals `ServerStarted.workflow_hashes`, so the
operator can check a document set before publishing it. It reads only that directory and writes
nothing. The first is the enrollment and break-glass command of decision 24, which mints a one-time enrollment URL to stdout on the host and journals the act as
loudly as an approval. It is a host-side command by design — the design doc's trust boundary is the
network boundary, and whoever holds host, environment and Doppler access already controls the
deployment — and it is explicitly **not** a fourth authentication layer: it introduces no static
shared secret, which trust boundary 5 forbids. `serve --http` reaches the same configuration path
as the binary, so it inherits every new refusal. The CLI's commands in
`crates/willikins-cli/src/commands.rs` pass `Principal { id, claims: None }` where they pass a bare
`PrincipalId` today. The CLI's own flow (`plan`, `apply`, `approve`, `reject`, `runs`, `run`, and
3m's `rollback`, which calls `Butler::plan_rollback` and `apply_rollback`, against a journal file)
is unauthenticated and unchanged: it is the operator's local flow, on the
machine that holds the credentials, and decision 5's argument for removing loopback bearer tokens
rests on it staying that way.

### Dockerfile and deployment

Four build inputs change.

- The `jsonwebtoken` backend does **not** change: it is already `rust_crypto` (see the pinned
  dependencies).
- The builder stage gains **`libssl-dev` and `pkg-config`** for `webauthn-rs-core`'s
  unconditional `openssl-sys`. The Dockerfile's own comment (`Dockerfile:41`) asserting no
  `aws-lc-sys`, `openssl-sys` or `cmake` anywhere in the tree becomes **false** and is rewritten
  in the same commit that makes it false.
- **The runtime stage stops carrying workflow documents** (task 20, decision 25). The
  `COPY workflows/*.yaml /app/workflows/` and the `ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows`
  (`Dockerfile:92`, `:93`) go, along with their comment (which still says "Only the two real
  workflow documents"). The runtime stage then copies the binary and nothing else.
- `.dockerignore` gains `workflows`, `private` and `crates/willikins-cli/tests/operator_*`.

`crates/willikins-server/tests/it/image_contents.rs` flips with the image. It currently pins the
COPY (`:92`), the ENV (`:149`, `:166`) and
`the_image_workflows_directory_holds_exactly_the_seventeen_positive_documents` (`:250`), and its
module doc still says "three". It is rewritten to assert three things: the runtime stage copies
only the binary; no `ENV` sets `WILLIKINS_WORKFLOWS_DIR`; and `.dockerignore` excludes the three
paths (test 27). Whether the distroless runtime image carries the matching `libssl.so.3` is
build-blocking and is verify item 2.

Doppler's Railway integration carries every provider credential the README names, plus, on open
decision 2's branch, the signing key and its retiring sibling (go-live step 7).
`WILLIKINS_OAUTH_CLIENT_SECRET` does not exist. `.railway/railway.ts` changes three times:

- In **task 20**, it gains `WILLIKINS_WORKFLOWS_DIR: preserve()`, after the operator has set the
  variable live and published the interim document set (decision 25's cutover).
- In **task 19**, it gains a `domains` entry on the custom-domain branch of decision 9 step 2.
- Also in task 19, at step 6, its `env` block drops the two retired `preserve()` rows and gains one
  per new `serve --http` variable.

Each change happens only after a read-only `railway config plan` that shows no delete, because the
file's own rule is that an omitted field is an instruction to delete.

**One volume now holds identity state, audit state and privileged content together.** The volume
is mounted at `/data` (`.railway/railway.ts:58`, 5000 MB), and Railway allows one per service. So
the journal, the credential store, the client table and the documents all live on it, in decision
25's layout:

| Path | Variable |
| --- | --- |
| `/data/journal.jsonl` | `WILLIKINS_JOURNAL_PATH` |
| `/data/credentials/` | `WILLIKINS_CREDENTIAL_STORE_PATH` |
| `/data/clients.yaml` | `WILLIKINS_CLIENTS_PATH` |
| `/data/workflows/` | `WILLIKINS_WORKFLOWS_DIR` |

The credential store, the client table and the journal are the first state this project has had
to back up. Losing the credential store means every human re-enrols, and losing the signing key
(in Doppler) refuses every outstanding token. Documents are not backed up from the volume: they
are re-published from the operator's checkout. Go-live step 9 is the backup step. Verify item 12's
mount-path half is settled; the volume's backup and restore behaviour is verify item 36.

## Environment variables

**Fifteen added**, six of them required in http mode and nine optional. Every tunable names the
component that reads it, so a value with no consumer cannot survive review. Four values that look
like they should be here are not, and deliberately: the **audience**, the **issuer identifier**,
the **redirect URI** and the **WebAuthn RP ID** are all derived from `WILLIKINS_PUBLIC_URL` and are
not configurable at all (decision 12), and the accepted `typ` set is frozen in code (decision 14).

Three variable **names** below are this plan's own choice rather than a research note's: the
signing key's two, and the credential-store and client-table paths. The scope revision names the
values and their shape, not their spelling.

| Variable | Default | Required | Value and who reads it |
| --- | --- | --- | --- |
| `WILLIKINS_PUBLIC_URL` | none | `serve --http` | The canonical `https://` origin the service is reached at: lowercase scheme and host, no default port, no trailing `/`, no path, no query, no fragment. Anything else is refused with `HttpConfigError::NonCanonicalUrl`, checked on the raw string (decision 12). The **issuer identifier** is this value exactly; the audience is `<it>/mcp`; the RP ID is its host. Read by `oauth::config`, `oauth::metadata`, `issuer::mint`, the client-redirect matcher and the ceremonies. |
| `WILLIKINS_SIGNING_KEY_PEM` | none | `serve --http` | The current ES256 signing key, PKCS#8 PEM, a secret with a redacted `Debug` and no `Display` or `Serialize`. Doppler only (open decision 2). Read by `issuer::key`. |
| `WILLIKINS_SIGNING_KEY_RETIRING_PEM` | empty | optional | The previous signing key during a rotation overlap. Published in the JWKS, never used to sign. Two variables exist because one cannot express an overlap (decision 22). Read by `issuer::key`. |
| `WILLIKINS_CREDENTIAL_STORE_PATH` | none | `serve --http` | The durable passkey and recovery-code store, on the volume at `/data/credentials/`, beside the journal and never inside `workflows/` (decision 25). Read by the ceremonies and the enrollment CLI. |
| `WILLIKINS_CLIENTS_PATH` | none | `serve --http` | The pre-registered client table: client id, display name, registered redirect URIs (decision 23). On the volume at `/data/clients.yaml`, written by the operator by hand, never inside `workflows/`. Read by `issuer::clients`. |
| `WILLIKINS_AGENT_SUBJECTS` | none | `serve --http` | Comma-separated `sub` values allowed to hold any MCP scope. Empty or unset refuses startup (decision 3). Read by `oauth::middleware::require_token`. |
| `WILLIKINS_APPROVER_SUBJECTS` | none | `serve --http` | Comma-separated `sub` values allowed to approve. Empty or unset refuses startup. Read by the approvals decision handler. |
| `WILLIKINS_OAUTH_ALGORITHMS` | `ES256` | optional | Comma-separated JWA names for the **validator's** allowlist. Asymmetric families only; intersected per key; must contain `ES256` (decision 15). Read by `oauth::validate`. |
| `WILLIKINS_OAUTH_LEEWAY_SECONDS` | `60` | optional | Whole seconds of clock skew. Read by `oauth::validate`. |
| `WILLIKINS_ACCESS_TOKEN_TTL_SECONDS` | `3600` | optional | The access token's lifetime, which with no refresh tokens is exactly how often a human re-consents at `/authorize`, and which is also the key-rotation overlap window (decisions 21, 22; open decision 4). Read by `issuer::mint` and by the rotation check. |
| `WILLIKINS_AUTHORIZATION_CODE_TTL_SECONDS` | `60` | optional | The authorization code's own lifetime, checked independently of any store TTL. A value above **600** refuses startup, which is draft-16's RECOMMENDED maximum; the default itself is this plan's choice and no specification sets it. Read by `issuer::codes`. |
| `WILLIKINS_SESSION_TTL_SECONDS` | `3600` | optional | The session's **absolute** lifetime, capped below `WILLIKINS_APPROVAL_WINDOW_SECONDS`; startup refuses a larger value. Read by the session store. |
| `WILLIKINS_SESSION_IDLE_TIMEOUT_SECONDS` | `300` | optional | The session's **idle** lifetime, bumped on writes and **not** on reads (decision 4). The default sits inside OWASP's fetched 2-to-5-minute range for high-value applications; the exact number is unpinned (verify item 11). Read by the session store. |
| `WILLIKINS_LOG` | the current INFO behaviour | optional | A `tracing_subscriber` filter directive (task 14). Read once, when the subscriber is built. |
| `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES` | empty | optional | Comma-separated resource identifiers this server also accepts in `aud` during a move to a new host (decision 12). Never published, never named in a challenge; a non-empty value logs a named startup warning. Read by `oauth::validate`. |

**Removed by this revision**, and not retired — they never shipped, so a deployment cannot have
them set: `WILLIKINS_OAUTH_ISSUER` (now derived), `WILLIKINS_OAUTH_CLIENT_ID`,
`WILLIKINS_OAUTH_CLIENT_SECRET`, `WILLIKINS_OAUTH_AUTHORIZE_URL`, `WILLIKINS_OAUTH_TOKEN_URL`,
`WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS`, `WILLIKINS_OAUTH_JWKS_URI`,
`WILLIKINS_JWKS_TIMEOUT_SECONDS`, `WILLIKINS_JWKS_REFRESH_SECONDS` and
`WILLIKINS_JWKS_MIN_REFETCH_SECONDS`.

Retired, and refused at startup: `WILLIKINS_AGENT_TOKEN_HASHES`, `WILLIKINS_APPROVER_TOKEN_HASH`.

**Changed, not added:** `WILLIKINS_WORKFLOWS_DIR` (task 20, decision 25). Its name and meaning are
the same, but the image no longer sets it. On the deployment it becomes a service variable set to
`/data/workflows` and kept by a `preserve()` row in `.railway/railway.ts`. Unset, it is
`ConfigError::Missing`, never a fallback to a baked-in directory. The README's row, which today says
"Always. The image sets this; do not set it again.", changes in task 20, and
`crates/willikins-server/tests/it/readme_variables.rs` is what holds the README table to the code.

Unchanged: `WILLIKINS_JOURNAL_PATH` (`/data/journal.jsonl`), `WILLIKINS_ALLOWED_HOSTS` (which
gains the measured public host at go-live), `WILLIKINS_PLAN_TTL_SECONDS`,
`WILLIKINS_APPROVAL_WINDOW_SECONDS`, `WILLIKINS_PLAN_RATE_PER_MINUTE`,
`WILLIKINS_READ_RATE_PER_MINUTE`, the provider credentials `WILLIKINS_GITHUB_TOKEN`,
`WILLIKINS_DOPPLER_TOKEN`, `WILLIKINS_BUILDKITE_TOKEN` and `WILLIKINS_SIGNOZ_API_KEY`,
`WILLIKINS_SIGNOZ_HOST`, `WILLIKINS_FAKE_CATALOG`, and `PORT`. The App Store Connect keys are not
deployment variables: documents read them from Doppler at run time. The README's variable reference is updated in the same task as
the code that reads each one.

## Acceptance tests

Thirty-one. The milestone cannot ship without every one of them, and each names the exact status
or error kind it expects. Tests 1 to 19 keep the numbers the 2026-09-16 review left them with, even
where their content changed, so that every cross-reference in the resolutions below still points
at the test it was written about. The authorization-server half is tests 20 to 26, and the
2026-10-20 addendum's are tests 27 to 31.

**Where they live, after 3k.** Each crate's default-built integration tests compile as one binary,
`tests/it/main.rs`, with one module per file. So every new acceptance-test file gets a
`mod <stem>;` line in its crate's `tests/it/main.rs`, or it is silently never compiled.
`crates/willikins-core/tests/it/test_layout_guard.rs` catches a missing line. Test 18's
`required-features = ["live-tests"]` target stays its own top-level binary, which 3k allows.

1. **The protected-resource-metadata document.** `GET /.well-known/oauth-protected-resource/mcp`
   with no credential answers **200** and `content-type: application/json`; `resource` is
   byte-identical to the derived audience `<WILLIKINS_PUBLIC_URL>/mcp` — which holds by
   construction now that the audience is not separately configurable (decision 12);
   `authorization_servers` has exactly one entry and it is willikins' own issuer identifier,
   byte-identical to `WILLIKINS_PUBLIC_URL`; `scopes_supported` is exactly the three MCP scopes of
   decision 3 and never contains `offline_access` or `willikins:approve`; `bearer_methods_supported` is `["header"]`; no parameter
   with zero values is present. The route answers with an unknown `Host` (it is outside rmcp's host
   check) and a bearer token is not required and not read. A companion test asserts the URL is
   built by insertion: a public URL of `https://h` publishes at
   `/.well-known/oauth-protected-resource/mcp`, and `GET /mcp/.well-known/oauth-protected-resource`
   answers **401**. Not 404 and never 200: that path sits *inside* the `/mcp` nest, so it meets the
   bearer middleware first, and the assertion is that it is refused as an unauthenticated MCP
   request rather than serving the document at the wrong URL. A second companion asserts the root
   `/.well-known/oauth-protected-resource` serves **no** document, which is decision 1's deliberate
   non-route.
2. **The 401 challenge.** No `Authorization` header → **401**, `WWW-Authenticate` containing
   `Bearer`, `resource_metadata="<the exact URL from test 1>"` and the fixed
   `scope="willikins:read willikins:plan willikins:apply"`. It is fixed because the token is
   validated before the body is read at all (decision 3), so an unauthenticated challenge cannot
   know what the call would have needed, and it names the three MCP scopes for decision 1's reason.
   The request is
   journaled `AuthFailedReason::MissingCredential`. A header that is not a `Bearer` credential
   (`Basic …`, say) → **401** `MissingCredential`, which is what ships today and what
   `crates/willikins-server/tests/it/adversarial_10b.rs` pins; whether RFC 6750 §3.1 calls that
   malformed, and therefore **400**, is verify item 22, and the task that ever flips it names
   every pinned test it flips. A well-formed but rejected token → **401** with the same header
   plus `error="invalid_token"`. `?access_token=` in the query string with no header → **401**
   `MissingCredential` (the parameter is never read). An **authenticated** request with a body over
   `max_body_bytes` → **413** from the middleware; an authenticated request whose body
   `ClientJsonRpcMessage` cannot parse → **400** from the middleware, before rmcp; an authenticated
   `GET` and `DELETE` at `/mcp` → **405** from rmcp with `Allow: POST`.
3. **The validation matrix, run twice — once against a willikins-minted token and once against the
   foreign-issuer fixture — and asserted identical.** One case per check of decision 1: not a
   parseable JWS, wrong `aud`, missing `aud`, wrong `iss`, missing `iss`, missing `sub`, `exp` in
   the past, `alg: none`, a symmetric `alg`, an `alg` outside the allowlist, a `typ` outside the
   frozen set, a `kid` absent from the key set, a header with no `kid`, a signature from a foreign
   key. Every one is **401** `error="invalid_token"` with the matching `AuthFailedReason` from
   decision 1's table; a token whose `aud` is an array containing this resource among others is
   **200**. Two cases exist only because willikins now mints: **a token signed with willikins' own
   key carrying a foreign `iss`** is refused `InvalidIssuer`, and **a token carrying the configured
   `iss` signed with a foreign key** is refused — with the reason split by case, because a test that
   accepts either is a test that asserts neither: a foreign key whose `kid` is not in the set is
   `UnknownKey`, and a foreign key **reusing willikins' own `kid`** is `InvalidSignature`. The
   second is the sharper case. Neither token is waved through on the strength of recognising half
   of itself. One further case for decision 15:
   with an allowlist spanning **two key families** (an `RS*` and an `ES*` entry) and a key set
   holding one key of each, a token signed by either verifies, and a token whose `alg` names the
   other key's family is refused — which is what proves the per-key intersection rather than a
   single mixed `Validation`. **The assertion that makes this seam rule 2 rather than prose:** for
   every case above, the sequence of checks that ran and the variant produced are the same for the
   in-house and the fixture token. Each case also runs as a unit test against `oauth::validate` so
   the reason is asserted, not just the status.
4. **Clock skew.** A token expired by less than `WILLIKINS_OAUTH_LEEWAY_SECONDS` is accepted; one
   expired by more is **401** `AuthFailedReason::TokenExpired`. A token whose `nbf` is in the
   future is **401** — `jsonwebtoken`'s `validate_nbf` defaults to false, so this test is what
   proves the override is in place.
5. **The key source and rollover, in process.** `oauth::validate` reads its key set through
   `JwkSource` and **never over HTTP**: a test asserts no outbound request is made on the
   validation path at all, which is the concrete form of decision 16's rule. After the issuer
   rotates, a token signed by the new `kid` validates and a token signed by the retiring `kid`
   **also** validates for as long as the overlap window holds it in the set; once the retiring key
   is dropped, the same token is **401** `AuthFailedReason::UnknownKey`. A key set holding two keys
   of different families exercises the per-key intersection of decision 15 from the source side.
   The refresh-failure policy, the staleness bound and the unknown-`kid` refetch floor are not
   tested here because they have no producer in 2c; decision 16 records them as the adapter path's.
6. **The inbound token neither leaves nor lands.** A probe that reads the headers rmcp hands a
   handler sees **no** `Authorization`, `Cookie` or `Proxy-Authorization` header. The probe is an
   **axum layer the test wraps around `router()`'s output**, not a test-only MCP tool and not a
   crate feature: an integration test under `tests/` cannot see a `#[cfg(test)]` library item, so a
   "test-only tool" would have to be compiled into the shipped binary or hidden behind a feature
   that nothing else needs. A tool call that makes a provider request against the mock HTTP server
   asserts the outgoing `Authorization` equals the operator credential and contains no substring of
   the inbound token. Three guards are extended to cover the new modules, **including the issuing
   half, whose signing key is the one new secret in the process**: the `expose_secret` guard
   (`crates/willikins-core/tests/it/expose_secret_guard.rs`), the `SinkToken::new` guard
   (`sink_token_guard.rs`) and 3m's `TeardownToken::new` guard (`teardown_token_guard.rs`). A sweep
   over the journal, every log line and every error body produced during a full
   plan-approve-apply cycle asserts that neither the token's bytes nor any byte of the private key
   appears anywhere. The guards gain **exactly one new allowed `expose_secret` site**: the signing
   key's own `reveal_for_signing`, called only from `issuer::mint`. That is the sixth token-less
   exception, beside the five `clippy.toml` already names (trust boundary 2). A second call site
   anywhere fails the guard. `Credential::authorize_basic` does not exist.
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
   notification from a listed subject → forwarded, no scope required. The consent screen of
   decision 20 offers exactly the same three MCP scopes, read from the same table, so the two
   cannot drift apart, and an `/authorize` asking for `willikins:approve` is granted only the MCP
   scopes it asked for. **The table is tied to the tool list:** every tool in
   `tests/it/snapshots/it__mcp_server__the_tool_list_and_every_schema_is_snapshotted.snap` has
   exactly one row in `oauth::scope`'s table, the table names no tool beyond the list and the
   pre-assigned rollback rows (`rollback_plan` → `willikins:plan`, the teardown apply →
   `willikins:apply`), and a fixture tool list carrying an unknown tool fails the assertion.
8. **Principal and journal.** The same `(iss, sub)` always derives the same
   `oauth-<12 hex>` principal and two different subjects never collide; the principal parses
   under `PrincipalId`'s grammar for a `sub` containing `|`, `:` and non-ASCII characters, and for
   the in-house UUID subject, which would have fitted the grammar directly and is derived anyway.
   Every event carrying a principal also carries `subject`, `issuer` and `client_id` when present,
   each bounded and escaped; a token with no `client_id` claim journals without it and does not
   fail. `TeardownPlanned` and `TeardownStarted` carry the claim fields like every other
   principal-carrying event. `pre-2c-every-event.jsonl` holds every `Event` variant, the four
   teardown variants included, and it and both pass-2 fixtures replay through `Journal` and through
   `replay` unchanged. The `WrongRole` page and the enrollment CLI print `sub` whole, because
   `OAuthSubject` is not an identifier type, while every plan or teardown rendering on the page is
   still `mask_json`-masked.
9. **Transport separation, asserted as identity of response.** `GET /approvals` with a valid
    access token in `Authorization` produces a response **byte-identical** to the same request with
    no header at all: **302** to `/approvals/login`, no session created, no `WWW-Authenticate`. The
    same for a `Basic` header, and the same for `POST /approvals/{plan_id}`, which is **403** in
    all three cases. A valid session cookie presented at `/mcp` with no `Authorization` header →
    **401** `MissingCredential`. Neither surface reads the other's credential in any code path.
10. **The approvals login.** `GET /approvals` with no session → **302** to `/approvals/login`.
    The login page's username step answers **uniformly** whether or not the account exists, so it
    is not an account-existence oracle. The ceremony-start endpoint — whose path is this plan's to
    pick, and is named nowhere in the research — mints a ceremony, stores its state
    **server-side** (the crate refuses to derive serde on it, which is the reason), and sets
    `__Host-willikins-login` with `Secure`, `HttpOnly`, `SameSite=Lax` and a 300 s lifetime; the
    finish step requires that cookie to be present and to name the same ceremony, and a mismatch or
    an absent cookie is **400** — the two-approver attack of decision 4, in its WebAuthn form. A
    completed ceremony mints a token carrying the enrolled `sub`, `aud = <WILLIKINS_PUBLIC_URL>/mcp`
    and `willikins:approve`, runs it through `oauth::validate`, and only then sets
    `__Host-willikins-session` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain`, no
    `Max-Age` and no `Expires`; the response carries `Cache-Control: no-store`,
    `X-Frame-Options: DENY` and a CSP with `frame-ancestors 'none'`, `form-action 'self'` and a
    per-response `script-src` nonce. A handler that fails to return the jar sets no cookie —
    asserted, because `axum-extra` documents that footgun and a silent failure here is an open
    approvals page. A session older than `WILLIKINS_SESSION_TTL_SECONDS`, one idle past
    `WILLIKINS_SESSION_IDLE_TIMEOUT_SECONDS`, and one used after `POST /approvals/logout`, → **302**
    to `/approvals/login` on a GET and **403** on a POST. Basic credentials presented → the
    identical response to none, with no `WWW-Authenticate: Basic` anywhere. The per-plan nonce is
    still required and is bound to the session: a `POST` without it, or with one minted for another
    session, is **403** and journals `AuthFailedReason::InvalidNonce`. Filling the ceremony store →
    **503**; filling the session store → **503**.
11. **The approver role.** A session whose `sub` is not in `WILLIKINS_APPROVER_SUBJECTS` → the
    approve and reject POSTs are **403** `AuthFailedReason::WrongRole` and nothing is journaled
    as a grant; the pending list is not rendered; and the 403 page **displays that session's own
    `sub` and `iss`**, escaped, so the operator can copy them into the allowlist. A session whose
    token lacked `willikins:approve` but whose `sub` is listed → **403** as well. That second case
    is **only reachable through the foreign-issuer fixture**, because willikins' own ceremony
    always mints `willikins:approve` (decision 4) — which is worth stating rather than deleting,
    since the check is what an adapter's token will meet. Both together → the decision is journaled
    with the derived principal and the approver's `subject`.
12. **Startup refusals and the retired variables.** `WILLIKINS_AGENT_TOKEN_HASHES` set (even
    empty-valued) → `HttpConfigError::RetiredVariable { name }`, and the same for
    `WILLIKINS_APPROVER_TOKEN_HASH`; the message names what replaces it. Each missing required
    variable → `ConfigError::Missing` naming it. An empty or unset `WILLIKINS_AGENT_SUBJECTS` or
    `WILLIKINS_APPROVER_SUBJECTS` → `HttpConfigError::EmptyAllowlist` naming which. A symmetric
    algorithm in the allowlist → `HttpConfigError::SymmetricAlgorithm`; an allowlist that does not
    contain `ES256` → `HttpConfigError::CannotVerifyOwnTokens` (decision 15).
    `WILLIKINS_SESSION_TTL_SECONDS` larger than the approval window →
    `HttpConfigError::SessionOutlivesApprovalWindow`; `WILLIKINS_SESSION_IDLE_TIMEOUT_SECONDS`
    larger than it → the same refusal. `WILLIKINS_AUTHORIZATION_CODE_TTL_SECONDS` above 600 →
    refused. A `WILLIKINS_PUBLIC_URL` carrying a path, a query or a fragment →
    `HttpConfigError::NonCanonicalUrl`, and so is one with an uppercase scheme or host
    (`HTTPS://h`, `https://H`), an explicit default port (`https://h:443`), or a trailing `/`
    (`https://h/`). A canonical value derives an issuer, an audience and a PRM `resource` that are
    byte-identical to what Claude sends as `resource`. An unset `WILLIKINS_WORKFLOWS_DIR` →
    `ConfigError::Missing`, with no fallback directory (decision 25). A
    `WILLIKINS_PUBLIC_URL` that is `http://` on a **non-loopback** host →
    `HttpConfigError::InsecureUrl`; the same URL on `127.0.0.1`, `[::1]` or `localhost` →
    **starts**, with a startup warning naming it (decision 7, both readings). An **unset**
    `WILLIKINS_SIGNING_KEY_PEM` → `ConfigError::Missing`, because at open decision 2's recommended
    default the key is a required variable like any other; a key that is **present and cannot be
    loaded** — the shape verify item 4's PKCS#8 round trip would fail in — and a credential store
    that cannot be read or written → `StartError`, because both need the filesystem or the runtime
    (decision 6). On the volume branch of open decision 2, "no key and none generatable" joins the
    second group rather than the first. A loopback `--bind` with no OAuth configuration → the same refusals as any other
    bind (decision 5). `hash-token` is gone from both binaries: invoking it exits non-zero with a
    usage error.
13. **Bounded header read.** The pass-2 slowloris test, un-`#[ignore]`d: a client that trickles
    headers is disconnected within the configured header-read timeout, the server keeps serving
    other connections, and graceful shutdown still drains an in-flight run.
14. **IPv6-aware origin check.** An `Origin` of `https://[::1]:8080` against an allowed host of
    `[::1]:8080` is accepted; `https://[::2]` is **403** `AuthFailedReason::ForeignOrigin`; a
    bracketed literal with no port (`https://[::1]`) is matched as `[::1]` and never cut to `[:`
    by `rsplit_once(':')` (`approvals.rs:276-284`). Two **pinning** cases for behaviour that already
    ships (decision 4): `https://willikins.example.com.attacker.com` is **403**, because the match
    is exact, and a request carrying neither `Origin` nor `Referer` on a non-safe method is
    **403**.
15. **Log level and the rmcp sweep.** With `WILLIKINS_LOG` raised to DEBUG and to TRACE, a full
    cycle (enrollment, login, `/authorize`, `/token`, plan, approve, apply, a provider error, an
    auth failure) emits no secret byte, no document text, no header value and no JSON-RPC request
    or response body on stderr. The sweep additionally covers **the signing key, the authorization
    code store, the ceremony endpoints and the recovery codes**: no private-key byte, no
    authorization code, no `code_verifier` and no recovery code reaches stderr at any level the
    binary can be configured to emit. The sweep records what rmcp itself emits at each level;
    anything that would print a body or a secret makes the level refused rather than the test
    relaxed.
16. **The concurrency permit.** The 64-call bound still refuses with the kind-tagged `Busy`, and
    the permit is held by the blocking closure — asserted by a test that would have passed before
    only if rmcp ran the handler on its own task. This is about where the permit is held and
    nothing else; it does not depend on any rmcp status mapping.
17. **Adversarial pass 3.** Every attack in decision 7's table, recorded under `docs/research/`
    as passes 1 and 2 were, with every bypass becoming a fixture plus a test. It attacks the
    **issuing** side at least as hard as the validating side, which is the point the exposure
    statement under Risks makes and the reason this pass runs after both halves exist.
18. **The conformance run.** Opt-in, `#[ignore]`d, behind a `live-tests` feature on
    `willikins-server` plus `WILLIKINS_LIVE_TESTS=1`, never in the workspace gate.
    `willikins-server` has **no `[features]` table today**, so task 18 adds one — `live-tests = []`
    — plus the `[[test]]` entry with `required-features = ["live-tests"]` that keeps the harness
    out of `--all-targets`. There is no external provider token to obtain, so what this test does
    is drive a **stock local MCP client** by hand against a locally bound `serve --http`, through
    its loopback redirect. Neither target app can reach a local server: Anthropic says "Claude
    reaches your server from Anthropic's infrastructure, so a server running on your machine needs
    a public URL", and both are hosted clients with HTTPS redirects. So this test covers the local
    half, and test 19 covers the hosted half against the public domain. The local run covers discovery of
    both metadata documents, then `/authorize` with PKCE, `/token`, and `list_tools` with the token
    that comes back. Two halves are asserted rather than observed. An **unregistered** client
    reaches `/authorize` and is refused by name — discovery works for a stranger, registration does
    not, which is the goal's own qualification. A **registered** client completes the flow and
    calls a tool. Then the refusals that need no minting: no token, a truncated token, a token with
    a tampered signature, and the same token against a server started with a **different
    `WILLIKINS_PUBLIC_URL`**, which is how the audience is varied now that it is derived. The run
    is recorded, and repeated against the public domain as a step of test 19.
19. **Go-live checks**, run by hand with the operator, in decision 9's order.
    - **Measured:** the `Host` and `X-Forwarded-Host` the service receives; what the edge does to a
      trickled header stream.
    - **Over the public domain, with no credential:** both metadata documents and the JWKS, and a
      401 whose `resource_metadata` URL resolves. The domain resolves to a globally routable IPv4
      address, and the registered URL answers without a 3xx: Anthropic says "Claude drops the
      `Authorization` header on the redirect".
    - **The variable cutover:** `railway config plan` showing no variable delete; whether a
      push-triggered deploy picks up staged variable edits; for a volume-backed service, how much
      downtime a redeploy costs and whether a failed deploy hands the volume back (verify item 30);
      the old variables gone before the deploy, and `WILLIKINS_FAKE_CATALOG` gone after it.
    - **The volume (decision 25):** `/data/workflows` holds exactly the manifest's names and shas,
      and the startup log's per-file lines match it. The image holds no document. A file added to
      the volume after start is refused on its first read.
    - **Backups:** the credential store, client table and journal backed up and a restore
      rehearsed; the signing key in Doppler's history.
    - **Each app, separately (go-live step 10):**
      - Claude connected from claude.ai through "Use your own OAuth client" with a blank secret,
        then used from the Claude iOS app.
      - ChatGPT connected on the web with its static client id, recording whether a blank secret is
        accepted (verify item 27), then used from the ChatGPT iOS app.
      - For each, recorded rather than assumed:
        - the `Origin` header its backend sends on `initialize`, if any, before `allowed_origins`
          freezes (verify item 26);
        - that it discovers through the 401's header;
        - which scopes it requests;
        - that `/token` receives a form-encoded body and answers well inside 10 seconds;
        - the protocol revision it negotiates (verify item 34);
        - that `state` round-trips;
        - which browser context `/authorize` opens in on iOS, and that the passkey ceremony, the
          parked-request resume and the recovery-code path all work there (verify item 13);
        - what the app does when the 3600 s token expires (verify item 32);
        - whether ChatGPT steps up on a 403 `insufficient_scope` (verify item 28).
    - **Then:** the stock-client run of test 18 repeated against the public domain, and one real
      plan-approve-apply cycle against the operator's documents.

    Discovery changes are allowed about five minutes to propagate, because Claude caches the
    protected-resource metadata with "a staleness window of about five minutes".
20. **The authorization-server metadata document and the JWKS route.** `GET
    /.well-known/oauth-authorization-server` with no credential answers **200**,
    `application/json`, and its `issuer` is **byte-identical** to `WILLIKINS_PUBLIC_URL`, which is
    the URL the well-known path was built from — the MCP MUST, and it holds by construction under
    decision 12. `code_challenge_methods_supported` is exactly `["S256"]`;
    `grant_types_supported` and `token_endpoint_auth_methods_supported` are **present** and are
    `["authorization_code"]` and `["none"]`, never omitted to their RFC defaults; RFC 8414's three
    REQUIRED members are present: `authorization_endpoint` and `token_endpoint` (each derived from
    `WILLIKINS_PUBLIC_URL`) and `response_types_supported == ["code"]`; `scopes_supported` equals
    the `/mcp` document's three MCP scopes exactly and never names `offline_access` or
    `willikins:approve`;
    `authorization_response_iss_parameter_supported` is `true`; `jwks_uri` is published and
    resolves; `client_id_metadata_document_supported` is **absent**, because CIMD is not
    implemented. **No array with zero elements is serialised as `[]`** — a case per array field,
    which is what a `skip_serializing_if` that is missing from one field fails. `GET /jwks.json`
    answers **200** with every live key, each carrying a `kid`, and **no private component**: the
    test asserts the response body contains no `d` member. All four routes answer with an unknown
    `Host` and require no token.
21. **The signing key: its `kid`, its persistence and the rotation overlap.** Trap 1 as an
    assertion, not a comment: every JWK willikins publishes carries a `kid` equal to the RFC 7638
    thumbprint of its own key, the same value the JOSE header carries, and a JWK built without one
    is refused at assembly rather than silently skipped by `JwkSet::find` — the failure mode being
    that willikins cannot validate its own token. Trap 2: an Ed25519 key in PKCS#8 **v2** is
    refused **by name** if Ed25519 is offered at all. A restart with the key persisted keeps every
    outstanding token valid; a restart **without** it refuses them with `invalid_token`, which is
    the cost decision 22 refuses to pay. Rotation: with two keys published, a token signed by the
    new one validates, a token signed by the retiring one validates, and the issuer signs with the
    new one from the moment it is published. The private key's `Debug` prints no key material, it
    has no `Display` and no `Serialize`, and it is zeroized on drop.
22. **`/authorize`.** `response_type` other than `code` is refused and the implicit grant is
    structurally unrepresentable. A request with **no `code_challenge`** is refused; a
    `code_challenge_method` of `plain` is refused; an unsupported method answers `invalid_request`.
    An unknown `client_id` and a `redirect_uri` outside the registered set each answer **400
    directly and are never redirected** — the phase-1 split, and a redirected phase-1 error is an
    open redirect. A registered loopback redirect on a **different port** is accepted, for
    `127.0.0.1`, `[::1]` and `localhost` alike, and never across the three; the same redirect on a
    different host or a different path is refused. A registered **non-loopback HTTPS** redirect
    (the shape of `https://claude.ai/api/mcp/auth_callback`) is matched exactly: a different port,
    a trailing `/` or a different path is refused. **`state`:** present → echoed byte-exact on the
    success redirect; absent → omitted; an error redirect carries both `state` and `iss`. **No
    session:** an `/authorize` without one parks the validated request, sends the browser to
    `/approvals/login`, and after a `SoftPasskey` login resumes the consent screen for exactly that
    request. The same resume id is refused from a browser without the `__Host-willikins-authorize`
    cookie, and a full park store answers **503**. **The consent page's CSP** is
    `form-action 'self' <the validated redirect origin>` and nothing wider; a browser check
    (Chrome and iOS WebKit) confirms the consent POST's redirect actually lands at a registered
    HTTPS callback (verify item 35). The consent screen **displays the
    redirect URI's hostname**, with an additional warning for a `localhost`-only redirect. `iss` is
    present on **every** response, the error responses included. A `resource` that is not the one
    canonical resource identifier is refused `invalid_target`, and the accepted one is bound into
    the code. `Cache-Control: no-store` is set before anything else on the response, and the
    endpoint's rate limit refuses beyond its bound. Every refusal journals its named
    `AuthFailedReason` and an unauthenticated flood is coalesced through decision 1's per-reason
    bucket.
23. **`/token` and the code bindings.** **One negative fixture per binding**, each carrying the
    header comment this repo's convention requires — naming its acceptance test and the exact error
    it must produce — and each asserting the binding is enforced from the **stored** record and not
    from a claim in the request: a code redeemed by the wrong `client_id`, with the wrong
    `code_verifier`, with a changed `redirect_uri`, and with a `resource` that is changed or newly
    introduced. A code redeemed **twice concurrently** yields exactly one token, which is what the
    atomic get-and-remove buys and what a find-then-delete would fail only under concurrency. A
    code past `WILLIKINS_AUTHORIZATION_CODE_TTL_SECONDS` is refused by its **own** expiry check
    even if the store would still return it. A code redeemed by the wrong client answers
    `invalid_grant` **indistinguishably** from an unknown code, asserted on body and status
    together. `grant_type` other than `authorization_code` is refused. The endpoint accepts
    `application/x-www-form-urlencoded`, the body every client sends. The response carries **no
    `refresh_token`**, and `offline_access` appears in no metadata document.
24. **Passkey enrollment and authentication.** Driven by `SoftPasskey`, with no browser.
    Registration enrols a credential, and a second registration of the **same `CredentialID`** — to
    the same account or to any other — is refused, which is the caller's obligation the crate
    cannot enforce. `exclude_credentials` is populated at registration. Authentication with
    `UserVerificationPolicy::Required` succeeds; a **UV-lying** authenticator
    (`SoftPasskey::new(true)`) is refused; a **counter regression** aborts with
    `CredentialPossibleCompromise`, while 0-on-both-sides is accepted, which is normal for a synced
    passkey. `Passkey::update_credential` is called on every successful authentication, asserted by
    the stored counter moving. A ceremony **finished against a different account** than the one it
    started for is refused — CVE-2026-69199's shape, and the reason the binding is stored with the
    ceremony rather than taken from a request field. `backup_eligible` and `backup_state` are
    **recorded** and never used to refuse.
25. **Recovery codes, the enrollment CLI and the break-glass.** A recovery code is at least 128
    bits from a CSPRNG, is stored only as an Argon2id hash, logs in exactly once, and is
    **replaced** by a freshly issued one after use; replaying it is refused with
    `AuthFailedReason::RecoveryCodeReplayed`. Concurrent recovery attempts are bounded by a
    semaphore, so an unauthenticated endpoint cannot turn 19 MiB per hash into a memory-exhaustion
    path, and per-account throttling refuses beyond its bound. Enrollment is **not complete until a
    second credential exists**, or the operator is nagged until one does. The CLI subcommand mints
    a one-time enrollment URL to stdout, journals the act as loudly as an approval, and prints the
    subject to paste into both allowlists; the URL is single-use.
26. **The session core.** Against a router the test builds, before `/approvals` depends on any of
    it: a login **rotates the session id unconditionally** and the previous id is destroyed, not
    merely superseded — a request carrying it afterwards has no session. The idle timer is bumped
    on a write and **not** on a read. The absolute timer is never bumped. A full store answers
    **503**. A **credential change flushes every live session** for that account, through a
    constant-time compare of the credential hash recorded at login. Session ids are 128 bits from a
    CSPRNG. `Set-Cookie` is asserted as a **response header**, never as store contents, because the
    `SignedCookieJar` drop is silent. `Cache-Control: no-store` is present on every response that
    carries a session id. A non-safe method with `Sec-Fetch-Site: cross-site` is refused, `Vary`
    names `Sec-Fetch-Site, Origin`, and the fallback origin check still runs when the header is
    absent.
27. **The image carries no workflow document** (decision 25, task 20). Read from the repository's
    own `Dockerfile` and `.dockerignore`, as `crates/willikins-server/tests/it/image_contents.rs`
    already does, because this host never builds an image. The runtime stage has exactly one
    `COPY`, the binary's. No `COPY` reads from `workflows/`. No `ENV` sets
    `WILLIKINS_WORKFLOWS_DIR`. `.dockerignore` excludes `workflows`, `private` and
    `crates/willikins-cli/tests/operator_*`. The seventeen-document assertion
    (`the_image_workflows_directory_holds_exactly_the_seventeen_positive_documents`) is replaced,
    not relaxed, and the module doc's stale "three" goes with it. `.railway/railway.ts` carries
    `WILLIKINS_WORKFLOWS_DIR: preserve()`, asserted by reading the file. The README's variable row
    no longer says the image sets it, held by `readme_variables.rs`.
28. **The trusted directory on a writable volume** (decision 25, task 21). A
    `WILLIKINS_WORKFLOWS_DIR` that is itself a symlink to a directory of valid documents →
    `StartupError::Symlink` at start, and the same refusal from `list_workflows` if the swap happens
    after start. A symlinked entry is still refused. A file swapped for a symlink between the
    `symlink_metadata` check and the open is refused by `O_NOFOLLOW`. The hash `ServerStarted`
    records for each document equals the sha256 of the exact bytes that were parsed, asserted by a
    test that changes the file between the two reads the old loop made. **A negative fixture placed
    in the directory after `Butler::start` cannot be planned**: `plan` by its name refuses with the
    same error startup would have produced, and `list_workflows` fails closed with
    `ButlerError::Startup`, never by silently skipping the file. Startup logs one line per document
    with its file name and short sha, a named warning for an empty directory, and a named warning
    when `RAILWAY_VOLUME_MOUNT_PATH` is set and the directory is not beneath it. No fixture from
    `workflows/fixtures/` is ever placed in a deployment's directory: the test builds its own
    temporary one.
29. **`willikins documents check`** (decision 25, task 22). Over a directory it prints one
    `<sha256>  <file name>` line per document in `shasum -a 256` format, and the set equals
    `ServerStarted.workflow_hashes` for a server started on the same directory. Over a directory
    holding a negative fixture or a broken `uses:` child, it exits non-zero naming the file, with
    the same `StartupError` a start would have produced. It writes nothing, and it refuses a
    symlinked directory like the server does.
30. **The approvals page shows what was planned, not what is on disk now** (decision 25, task 23).
    A pending plan's section shows the root document's short sha beside the `used` closure's.
    Render it, change the document's bytes on disk, and render again. The second render shows **no**
    "document says" text and says instead that the document changed since the plan and that apply
    will refuse it with `DocumentChanged`. Restoring the original bytes restores the description.
31. **Teardowns on the approvals page** (decision 4, task 24). A teardown that needs approval,
    planned through `Butler::plan_rollback`, is listed with its steps in order, every value masked
    by `mask_json`, and its own session-bound nonce. The approver's decision journals
    `ApprovalGranted` against the teardown's `plan_id`. A session whose `sub` is not in
    `WILLIKINS_APPROVER_SUBJECTS` → **403** `WrongRole`. A nonce minted for a forward plan cannot
    decide a teardown, and the reverse: both are **403** `InvalidNonce`. A teardown `plan_id` POSTed
    with no nonce at all → **403** `InvalidNonce`, as for a plan.

## Gates

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

All four before every commit, bare `cargo`, in the background with a 600,000 ms timeout, reading
the log body rather than a captured exit code. `-j 2` and `RUST_TEST_THREADS=2` are load-bearing
on this host; never run two cargo commands at once. The conformance run (test 18) and the go-live
checks (test 19) are `#[ignore]`d or by hand and are never part of the gate. Every Workflow
`CONTEXT` string names these four commands and no wrapper.

**Scoped runs, in 3k's form**, for the inner loop between full gates:
`RUST_TEST_THREADS=2 cargo test -p <crate> --test it -j 2 -- <module>::`. Keep the trailing `::`.
`--exact` needs the module prefix too (`<module>::<test_name>`); without it, it silently matches
nothing instead of failing. A unit test inside a crate's `src/` runs with `--lib` in place of
`--test it`. A scoped run never replaces the four gates before a commit.

## Tasks

**The precondition is met.** The milestone 2 plan carries `**Completed:** 2026-09-16` (its line 5).
Go-live steps 7 and 8 are milestone 2's own remaining operator work, and step 8 is still work:
`WILLIKINS_FAKE_CATALOG: preserve()` is still in `.railway/railway.ts`.

Twenty-six tasks, plus the research that is already done, in dependency order. Tasks 20 to 25 are
new on 2026-10-20. No earlier task is obsolete, but two lose a part: task 1 no longer builds both
`jsonwebtoken` backends, and task 13 loses the two origin "fixes" that turned out to be current
behaviour. **Everything runs sequentially on `main`**, one lane at a time. This host has 11 GB of
RAM shared with other sessions, and the milestone 2 experiment with parallel worktree lanes took
six hours and was OOM-killed. Agents stage only their own paths, and the coordinator commits by
path. The table is ordered as the tasks run: 20 to 22 come straight after 1b, because they move
the documents off the image, are independent of OAuth, and gate nothing else's design.

Each "Pinned by" cell names the scoped run for the inner loop, in 3k's form (see Gates). A new
test file in that cell means a new `tests/it/<stem>.rs` plus its `mod <stem>;` line in that crate's
`tests/it/main.rs`. The four gates still run before every commit.

| # | Task | Depends on | Delegate to |
| --- | --- | --- | --- |
| 0 | Research: `docs/research/2026-09-16-m2c-authorization.md`, then `…-m2c-own-authorization-server.md` and `…-m2c-scope-revision.md`. **Done 2026-09-16.** The 2026-10-20 code audit, client-and-spec pass and volume pass are folded into this plan's 2026-10-20 addendum | | six parallel passes, then two; then three on 2026-10-20 |
| 1 | **Dockerfile and dependency pins, first, because it is the one thing that can invalidate the shape.** **Record** the `jsonwebtoken` backend as the `rust_crypto` the workspace already uses (`Cargo.toml:75`), and do not build both: a switch would be workspace-wide and would touch App Store Connect (pinned dependencies). Add `libssl-dev` and `pkg-config` to the builder for `webauthn-rs-core`'s unconditional `openssl`/`openssl-sys`. **Verify that the distroless runtime image ships the `libssl.so.3` that `openssl-sys` 0.9.114 links against** (verify item 2, build-blocking). Rewrite the `Dockerfile:41` comment that claims no `openssl-sys` anywhere. Pins: `webauthn-rs` 0.5.5 (+ core/proto), `argon2` 0.6.0, `password-hash` **0.6.1**, `axum-extra` 0.12 `cookie-signed`, `rand_core = { version = "0.6", features = ["getrandom"] }` on `willikins-server` (the providers-github precedent, not `rand` 0.8), and `webauthn-authenticator-rs` 0.5.5 dev-only. Record the RUSTSEC-2023-0071 justification (pinned dependencies). Pinned by: the image builds on Railway's builder, the runtime container starts and serves `/healthz`, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- image_contents::`, and all four gates | 0 | sonnet, verified by opus |
| 1b | rmcp 3.4.0 (conditional, decision 18): `cargo update -p rmcp --precise 3.4.0`, the `ServerInfo` rename at three `mcp.rs` sites, `enforce_origin_validation()`, all four gates. Lands it or records why not; nothing depends on it | 1 | sonnet |
| 20 | **The image carries no workflow document** (decision 25). **Operator first, before this task's commit is pushed:** create `/data/workflows` and publish the public `workflows/*.yaml` into it by decision 25's procedure, then set `WILLIKINS_WORKFLOWS_DIR=/data/workflows` on the service. Then the task: drop `COPY workflows/*.yaml /app/workflows/`, `ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows` and their comment from the Dockerfile; add `workflows`, `private` and `crates/willikins-cli/tests/operator_*` to `.dockerignore`; rewrite `image_contents.rs` to test 27's assertions (its seventeen-document test is replaced and its "three" module doc fixed); add `WILLIKINS_WORKFLOWS_DIR: preserve()` to `.railway/railway.ts`, after a read-only `railway config plan` shows no delete; and change the README's variable row. Pinned by: test 27, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- image_contents:: readme_variables::` | 1, operator | sonnet; the operator does the volume step and the plan read |
| 21 | **Harden the trusted directory for a writable volume** (decision 25). Refuse a symlinked `WILLIKINS_WORKFLOWS_DIR` at start and on every scan (`StartupError::Symlink`). Open named documents with `O_NOFOLLOW` and check the opened handle. `scan_directory` hashes the bytes it parsed (`load_document_with_bytes`). Startup logs each name with its short sha, and warns on an empty directory and on a directory outside `RAILWAY_VOLUME_MOUNT_PATH`. Pinned by: test 28, a new module `trusted_directory_volume`, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- trusted_directory_volume:: acceptance_13_trusted_directory::` | 20 | sonnet test-first, opus attacks |
| 22 | **`willikins documents check <dir>`** (decision 25): runs `scan_directory`, prints the `shasum -a 256` manifest equal to `ServerStarted.workflow_hashes`, writes nothing. README section on the publish procedure, with decision 25's "agents never write the volume" rule. Pinned by: test 29, `RUST_TEST_THREADS=2 cargo test -p willikins-cli --test it -j 2 -- documents_check::` | 21 | sonnet |
| 2 | **First commit:** freeze `pre-2c-every-event.jsonl` from the current binary, before any 2c change (decision 2), through an `#[ignore]`d generator shaped like the two in `crates/willikins-journal/tests/it/pre_pass_2_replay.rs:311` and `tests/it/post_pass_2_shapes.rs:217`. It holds **every** `Event` variant, the four teardown variants included. Then the **foreign-issuer fixture** (decision 7): a fixed test key, a key set carrying a `kid`, `mint(flaws)` including the two new flaws, key rotation, and the shared environment-block helper. **No `/authorize`, no `/token`, no metadata documents, no HTTP JWKS route and no blocking `start()`**: each of those lost its consumer with decision 16, and the foreign-issuer assertion is made in process. Wire `SoftPasskey`. Shared test support, used by tasks 4, 5, 7, 9 and 16. Pinned by: all three fixtures replay through `Journal` and `replay`, `RUST_TEST_THREADS=2 cargo test -p willikins-journal --test it -j 2 -- pre_2c_replay:: pre_pass_2_replay:: post_pass_2_shapes::` | 1 | sonnet, verified by opus |
| 3 | **The signing key and the issuer's mint.** The `#[domain(secret)]` signing-key type in `willikins-types`, modelled on `AppleSigningKey`, with its own `reveal_for_signing`. That is the sixth token-less exception, added to `clippy.toml`'s reason and to `crates/willikins-core/tests/it/expose_secret_guard.rs`. ES256 generation (`p256` with `rand_core::OsRng`), PKCS#8 loading from the Doppler variable (open decision 2), the RFC 7638 thumbprint as `kid` set on **both** the header and the published JWK, `JwkSet` assembly, the two-key overlap mechanism, `mint(sub, scopes, aud) -> Jwt` emitting all seven RFC 9068 claims with `typ` overwritten, and the `JwkSource` boundary with its in-process implementation. Pinned by: test 21, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- issuer_key::`, `cargo test -p willikins-core --test it -j 2 -- expose_secret_guard::`, `cargo check -p willikins-types -j 2` | 2 | sonnet, verified by opus |
| 4 | **The resource-server middleware, against both issuers.** `OAuthConfig`, `validate`, `require_token`, the header strip, the subject-allowlist check, the new validating-half `AuthFailedReason` variants, and the refusals of decisions 6, 7, 12 (`NonCanonicalUrl`) and 15. Plus the assertion that the fixed-order path runs **identically** for a willikins-minted and a fixture-minted token. **Also removes `HttpConfig::build`'s agent-hash rules and retires `WILLIKINS_AGENT_TOKEN_HASHES`**, and migrates every `HttpConfig::build` call site (counted below). Pinned by: tests 3, 4, 5, 6, and test 12's validating-half refusals, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- oauth_validation:: adversarial_10b:: binary_startup::` and `cargo test -p willikins-server --lib -j 2 -- http::config::` | 3 | sonnet, verified by opus |
| 5 | **Both metadata documents, the JWKS route and the challenge surface.** RFC 9728 at the insertion-built path, with `scopes_supported` the three MCP scopes and the root deliberately not a route. RFC 8414 with a byte-identical `issuer`, `authorization_endpoint`, `token_endpoint`, `response_types_supported: ["code"]`, `code_challenge_methods_supported`, explicit `grant_types_supported` and `token_endpoint_auth_methods_supported`, `authorization_response_iss_parameter_supported`, `jwks_uri`, the same `scopes_supported`, and zero-element arrays omitted. `/jwks.json`. All four routes outside the bearer middleware and rmcp's host check, as `/healthz` is. The 401 naming the three MCP scopes, and the 403. rmcp's `allowed_origins` set to the one derived public origin at 3.3.0 (decision 18). Pinned by: tests 1, 2, 20, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- oauth_metadata:: deploy_host_headers::` | 4 | sonnet, verified by opus |
| 6 | Principal and journal: the `oauth-<12 hex>` derivation; the `Principal { id, claims }` signature change across **all eleven** `Butler` methods decision 2 lists (3m's `plan_rollback` and `apply_rollback` included), `record_auth_failure`, the eight `#[tool]` methods, `principal_for`, the approvals handlers, and `willikins-cli`'s commands including `rollback`; the optional claim fields on all eight principal-carrying events, bounded and escaped; `OAuthSubject` and `ClientId` as plain domain types. Pinned by: test 8, `RUST_TEST_THREADS=2 cargo test -p willikins-journal --test it -j 2 -- event_shapes:: pre_2c_replay::` and `cargo test -p willikins-server --test it -j 2 -- oauth_principal:: rollback_r8::` | 4 | sonnet, verified by opus |
| 7 | Scopes: the four and the hierarchy; the `scope`/`scp` readers; the tool table with the pre-assigned rollback rows, **tied to the snapshotted tool list**; the body parse; the 403 with `insufficient_scope`; `scopes_supported` (the three MCP scopes); the same table feeding the consent screen; `willikins:approve` never granted at `/authorize`. Pinned by: test 7, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- oauth_scopes:: mcp_server::` | 5, 6 | sonnet, verified by opus |
| 8 | **Session core, as library code.** The signed `__Host-` cookie and jar, both bounded stores with their 503, unconditional id rotation, the absolute **and** idle timeouts, `no-store`, 128-bit ids, the non-persistent cookie, credential-change revocation, `X-Frame-Options`, the CSP with `script-src`, `Sec-Fetch-Site` and `Vary`. Plus **pinning tests** for the origin check's exact match and its both-headers-absent refusal, which already ship (decision 4). **No `/approvals`-level assertion lands here**: `basic_auth` still gates that surface until task 9 removes it, and decision 6 forbids a commit in which both gates exist. Pinned by: test 26, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- session_core::` | 7 | sonnet, verified by opus |
| 9 | **Passkey enrollment and authentication, and the gate swap.** `webauthn-rs` wiring, the durable credential store at `/data/credentials/`, ceremony-to-account binding, UV Required, counter regression and `update_credential`, `exclude_credentials` and CredentialID uniqueness, the uniform username step, the inline script under a CSP nonce, logout, the `WrongRole` page (showing `sub` whole), and the **mint → `oauth::validate` → session** hop. **Removes `basic_auth`, `TokenHash`, `matches_any` and the constant-time compare, and retires `WILLIKINS_APPROVER_TOKEN_HASH` in this series**, so no commit on `main` has an unauthenticated `/approvals` and none has two gates on it. Flips the three `adversarial_10b.rs` pins named below. **Every `/approvals`-level assertion is this task's.** Pinned by: tests 9, 10, 11, 24, and test 12's approver-hash retirement, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- approvals_login:: passkey:: adversarial_10b::` | 8 | **sonnet test-first, opus attacks**: CVE-2026-69199's class lives here |
| 23 | **The approvals page shows what was planned** (decision 25): the root's short sha beside the `used` closure, and "document says" only while the file's sha equals `record.document_sha256`, otherwise the `DocumentChanged` notice. Pinned by: test 30, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- approvals_document_identity::` | 9 | sonnet test-first, opus attacks |
| 24 | **Teardowns on the approvals page** (decision 4): `pending_teardowns` in `willikins-journal`, the teardown section through `mask_json`, session-bound nonces for teardown `plan_id`s, the same role rules. Pinned by: test 31, `RUST_TEST_THREADS=2 cargo test -p willikins-journal --test it -j 2 -- views::` and `cargo test -p willikins-server --test it -j 2 -- approvals_teardowns:: rollback_r8::` | 9, 6 | sonnet test-first, opus attacks |
| 10 | **Recovery codes, the enrollment CLI and the break-glass.** Codes of at least 128 bits, Argon2id, single-use and reissued after use; an Argon2 semaphore and per-account throttling, so an unauthenticated endpoint cannot be a memory-exhaustion path; the two-credential enrollment rule; the CLI subcommand that mints a one-time enrollment URL to stdout and journals it as loudly as an approval; the printed subject for both allowlists. **Where this CLI runs against a distroless deployment is open question 2**, and the task does not guess. Pinned by: test 25, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- recovery_codes::` and `cargo test -p willikins-cli --test it -j 2 -- enroll::` | 9 | sonnet, verified by opus |
| 11 | **`/authorize` and `/token`.** The pre-registered client table at `/data/clients.yaml`. The two-phase error split. Exact redirect matching: loopback port relaxed for `127.0.0.1`, `[::1]` and `localhost`, never across them; hosted HTTPS redirects matched exactly. Mandatory `S256`. **`state` stored and echoed byte for byte** beside `iss` on every response, errors included. The consent screen, with the redirect hostname and its own `form-action` CSP. The **parked request** for an `/authorize` with no session, and its resume. `resource` → `aud`. The code record with all four bindings **stored**, the atomic get-and-remove claim, the independent expiry check and the 600-second ceiling. Rate limits and `Cache-Control: no-store`. Form-encoded `/token`. `invalid_grant` indistinguishability. No refresh tokens. The issuing-half `AuthFailedReason` variants. Pinned by: tests 22, 23, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- issuer_authorize:: issuer_token::` | 10 | **opus writes or co-writes.** The densest security surface in the milestone |
| 12 | Delete `hash-token` from both binaries, its README section and the `hash_token` test module with its `mod` line; update the variable reference. Pinned by: test 12's last case, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- readme_variables:: binary_startup::` | 11 | sonnet |
| 13 | Bracket-aware port stripping in the origin check, the one real defect (`approvals.rs:276-284`: `rsplit_once(':')` cuts `[::1]` to `[:`). Pinned by: test 14, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- origin_ipv6::` | 11 | sonnet |
| 14 | Two steps, in order. (1) `WILLIKINS_LOG` as a `tracing_subscriber` `EnvFilter` directive, which flips `crates/willikins-server/tests/it/adversarial_13.rs`'s `the_binary_ignores_rust_log_and_emits_nothing_below_info` (line 1249) into its successor. (2) The sweep of everything the process emits at DEBUG and TRACE, rmcp's own output included, now also covering `/authorize`, `/token`, the ceremony endpoints, the code store, the parked-request store, `state` and the signing key. Amend milestone 2's trust boundary 5 in that plan's own addendum style. Pinned by: test 15, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- adversarial_13:: log_sweep::` | 12 | sonnet, swept by opus |
| 15 | Move the concurrency permit into the blocking closure. Pinned by: test 16, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- blocking_pool_13::` | 12 | sonnet |
| 16 | **Adversarial pass 3**, against the foreign-issuer fixture and against willikins' own endpoints: decision 7's whole table, recorded under `docs/research/`, plus the 2026-10-20 surfaces. Those are `state` tampering and omission; a parked request resumed from another browser; the consent CSP widened beyond the one origin; a forward-plan nonce against a teardown and the reverse; a symlinked or swapped workflows directory; a document changed between plan and approval; and the row "a vendor-backend `Origin` is not 403". Freeze `post-2c-new-shapes.jsonl`. Test 17 | 12, 13, 14, 15, 21, 22, 23, 24 | **opus** |
| 17 | Bound the header read: replace `axum::serve` with a `hyper::server::conn::http1` accept loop (timer first, decision 8) carrying graceful shutdown and connection accounting; flip the pass-2 slowloris test. Runs **after** pass 3, with a short pass-3 addendum recording that the transport changed under it. Pinned by: test 13, `RUST_TEST_THREADS=2 cargo test -p willikins-server --test it -j 2 -- adversarial_13:: http_server:: http_smoke::` | 16 | sonnet, verified by opus |
| 18 | **The conformance run.** The `live-tests` feature and its own `[[test]]` target (outside `tests/it/`, as 3k allows), and the harness that drives a stock local MCP client through discovery, `/authorize`, `/token` and `list_tools` against a locally bound server, unregistered and registered. Test 18 | 17 | sonnet writes the harness, run with the operator |
| 25 | **The invariant's wording** (decision 25): the "trusted ref" invariant re-founded, edited together in `CLAUDE.md` and `AGENTS.md`; a design-doc addendum beside `docs/plans/2026-09-11-willikins-design.md`'s "a checkout of a trusted ref"; and the written rule that only the operator writes the volume, in both files and in memory. **The operator approves the wording first** (open question 3). No code | 20, operator | coordinator with the operator |
| 19 | Go-live (decision 9): the domain; the measured `Host` and edge behaviour; allowed hosts; enrollment and both allowlists; the private documents and the client table on the volume; `.railway/railway.ts` and `railway config plan`; the retired variables removed in the same change; Doppler's secrets; the backups; `WILLIKINS_FAKE_CATALOG` removed; both apps connected and used from iOS; the stock-client run against the public domain; one real cycle. Mark this plan **Completed**. Test 19 | 18, 25, operator | coordinator with the operator |

**Why task 1 moved to the front.** In the delegating draft the Dockerfile question was a dependency
footnote. It is now the item most likely to invalidate the milestone's shape. `webauthn-rs-core`
depends unconditionally on `openssl` and `openssl-sys`, with no pure-Rust backend and no feature to
turn it off, and the runtime image's `libssl` is the weakest citation in either note. Both facts
would otherwise be discovered in a deploy at 3 a.m. rather than in a plan.

**Why task 20 has an operator step before its push.** Railway auto-deploys every push to `main`
onto the live service. The moment the image stops carrying documents and the ENV, a service whose
`WILLIKINS_WORKFLOWS_DIR` is unset, or points at an empty or missing path, refuses to start, and
under `ON_FAILURE` that is a crash loop with no shell to fix it from. So the directory and the
variable exist live first, and the commit follows.

**Why `/authorize` and `/token` is the opus task.** Kanidm spends roughly 3,600 implementation
lines against 5,160 test lines on this surface, a 1.4:1 ratio that is the most transferable number
in the corpus. The one fetched post-mortem of a comparable Rust server is a rewrite that deleted an
equality check, kept the comment claiming it happened, and survived a professional audit running in
the same window. That is why task 11's fixtures assert that the binding lives in the **store**.

**Why 17 runs after pass 3 rather than before it.** Task 17 rewrites the transport: graceful
shutdown, connection accounting and the run-drain bound all move into hand-written code. Running it
before the adversarial pass would make every finding in that pass ambiguous between "the
authentication is wrong" and "the accept loop is wrong", which is exactly the confusion the Risks
section asks to avoid. Pass 3 therefore runs against the transport milestone 2 shipped, 17 lands
after it with test 13, and a short addendum records what changed underneath. The public listener
still gets the bound before it is public: task 19 depends on 18, which depends on 17.

**Task 4's migration budget, counted rather than estimated** (recounted 2026-10-20 against the
3k layout). `HttpConfig::build` takes the two hash parameters that stop existing, so **every call
site changes**. There are **13** in the tree. One is the production call
(`crates/willikins-server/src/cli.rs:314`). The other **12 are test sites across seven files**:

- four unit tests in `crates/willikins-server/src/http/config.rs` (lines 315, 328, 340, 352);
- three in `crates/willikins-server/tests/it/adversarial_10b.rs` (39, 357, 364);
- one each in `tests/it/blocking_pool_13.rs` (224), `tests/it/composition_s3.rs` (42, added by
  2b), `tests/it/deploy_host_headers.rs` (65), `tests/it/http_server.rs` (97) and
  `tests/it/http_smoke.rs` (36).

Several files reach `build` through a local `base_config()` helper rather than calling it
directly, which is why the count is that low. Three more files set the hash **environment
variables** and move with them: `tests/it/binary_startup.rs`, `tests/it/adversarial_13.rs` and
`tests/it/serve_http_deploy_pins.rs`. Task 2's environment-block helper is what keeps that
migration from being a dozen hand-written blocks.

**Task 4 also has an ordering constraint inside `cli.rs`**, and it outlives the JWKS fetch that
first exposed it. `cmd_serve_http` calls `build_http_config` (line 253) *before* `build_runtime`
(line 267), and `HttpConfigError` is `Copy` and is produced by the pure `HttpConfig::build`. So a
refusal that needs the filesystem or the runtime cannot live there. That means an unloadable
signing key and an unreadable credential store, and both go on `cli.rs`'s own `StartError`,
beside `Bind` and `NoBindOrPort` (decision 6).

**Task 9 flips three pins in `crates/willikins-server/tests/it/adversarial_10b.rs`**, and it names
them rather than discovering them. `the_approver_token_presented_as_a_bearer_on_approvals_is_401`
(line 290) asserts a **401** carrying `WWW-Authenticate: Basic realm="willikins"` for a bearer
header at `GET /approvals`; that becomes the 302 of decision 4, with no challenge header at all.
`an_empty_basic_username_is_403` and `an_approver_username_spelling_an_agent_principal_is_403`
(lines 272 and 253) assert Basic-username behaviour that stops existing along with
`MalformedUsername`.

**The new test modules named above are this plan's choice of spelling**: `pre_2c_replay`,
`oauth_validation`, `oauth_metadata`, `oauth_principal`, `oauth_scopes`, `session_core`,
`approvals_login`, `passkey`, `recovery_codes`, `enroll`, `issuer_key`, `issuer_authorize`,
`issuer_token`, `origin_ipv6`, `log_sweep`, `trusted_directory_volume`, `documents_check`,
`approvals_document_identity` and `approvals_teardowns`. An implementer may rename one, and the
plan's cell moves with it. A filter that matches nothing passes silently, so each task's first
scoped run must show a non-zero test count.

Every implementation task is test-first: the failing test lands in the same commit series before
the behaviour, and every adversarial pass is opus.

## Verify with a browser

Thirty-six items: twenty-three merged from both research notes' own lists, and items 24 to 36
added on 2026-10-20 for what the client-and-spec and volume passes could not fetch verbatim.
Items keep their numbers, because the resolutions cite them; one that is settled says so in place. These are the ones this plan
depends on, each named with the task that owns it. **Nothing below may be written into frozen
code before it is settled.** Items that existed only to choose or characterise an external
identity provider are gone, not renumbered around: what `typ`, `alg` and claims a provider emits;
whether its access token carries `client_id`; and the six-provider caveat list. willikins emits all
of those by construction now, and where the answer still matters for an adapter's token the plan
says so in the decision rather than waiting on a fetch.

1. **Settled 2026-10-20: `rust_crypto` builds in the repository's Dockerfile**, because the shipped
   image already compiles `jsonwebtoken` 11.1.0 with it (`Cargo.toml:75`, used by
   `willikins-providers-http`), and Railway's builder is the build proof. `aws_lc_rs` stays
   untested, and it would be a workspace-wide switch with the `g++` blocker the pinned-dependencies
   section records.
2. **Task 1: whether `gcr.io/distroless/cc-debian12` ships the `libssl.so.3` that `openssl-sys`
   0.9.114 links against.** The distroless README names no libssl version at all and was fetched
   from `main` rather than a tag. **Build-blocking**, and the weakest citation in either note.
   Inspect the image.
3. **Task 1b: whether `cargo update -p rmcp --precise 3.4.0` resolves and the four gates pass.**
   The `ServerInfo` rename at three sites is the only breakage identifiable statically; 3.4.0's
   cancellation and pre-init changes were not verified against willikins' stateless configuration.
4. **Task 3: whether the PKCS#8 a Rust key factory emits round-trips through `jsonwebtoken`.**
   **The first half is exercised in-tree** (2026-10-20). `crates/willikins-types/src/appstore.rs`
   re-encodes a P-256 key to PKCS#8 PEM with `p256`'s `EncodePrivateKey`, and
   `crates/willikins-providers-http/src/apple_credential.rs:175` feeds that PEM to
   `EncodingKey::from_ec_pem`. Task 3's test 21 still asserts the round trip for a key *generated*
   by `SecretKey::random`, but nothing is unknown about the format. The second half stays open,
   and it no longer matters, because the `rand_core` 0.6 bound is met by the providers-github
   precedent: whether `ring`'s EC PKCS#8 output round-trips into `EncodingKey::from_ec_der`,
   given ring deliberately omits the inner `parameters` field. It would have decided whether `ring`
   could be the key factory instead of `p256`.
5. **Tasks 4 and 11: every OAuth 2.1 section number.** draft-ietf-oauth-v2-1-16 is an
   Internet-Draft (rev 16, 2026-09-03, expires 2027-03-07) whose Status of This Memo forbids citing
   it other than as work in progress, and which already carries one unreconciled tension (refresh
   tokens: non-normative §10 "must" versus normative §4.3 "SHOULD" — decision 21 resolves it by
   issuing none). Which draft each MCP section number resolves to is verified stable only for §5.2.
6. **Tasks 4 and 11: documents cited but not fetched**, each to be read before the code it governs
   is written. RFC 9700 (the Security BCP behind the exact-match rule), RFC 7662 and RFC 7009 (only
   if introspection or revocation is ever offered), draft-16 §4.3.1 and §7.5.1, RFC 9449 (DPoP),
   RFC 8705 (mTLS), RFC 9126 (PAR), RFC 9207 (`iss`), RFC 8693 §4.2 and §4.3 (the `scope` and
   `client_id` claim definitions RFC 9068 §2.2 delegates to), OpenID Connect Discovery 1.0, and the
   IANA OAuth Dynamic Client Registration client-metadata registry.
7. **Task 8: `SameSite=Strict` versus `Lax`, measured rather than argued.** With an in-house
   authorization server, `/authorize` is reached by a top-level navigation an MCP client launched
   and the consent page needs the session cookie. **Whether `Strict` withholds a cookie on an
   externally-initiated top-level navigation is a browser behaviour nobody fetched.** Default to
   `Lax` until it is measured, and treat the answer as a seam constraint (decision 4). Related and
   also unfetched: whether any mainstream browser still applies Chrome's historical two-minute
   "Lax+POST" grace — it applies only to cookies with **no** explicit `SameSite`, which is not this
   case, so do not freeze a test asserting "Lax blocks a forged cross-site POST" without checking.
8. **Settled 2026-10-20: `localhost` joins**, because Claude Code registers both
   `http://localhost/callback` and `http://127.0.0.1/callback` and needs both matched with the port
   ignored (the 2026-10-20 client-and-spec pass, from Anthropic's connector docs). The port is
   relaxed for the three hosts and never across them (decision 20). The original question:
   **Task 11: whether `localhost` joins `127.0.0.1` and `[::1]` in the port-relaxed redirect set.**
   OAuth 2.1 writes the port exception for the IP literals and says `localhost` is NOT RECOMMENDED;
   the MCP CIMD example document registers `http://localhost:3000/callback` alongside the IP form;
   the TypeScript SDK relaxes the port for all three while forbidding cross-matching between them;
   `oxide-auth` has an `IgnorePortOnLocalhost` variant. Four sources, three behaviours. Decide
   deliberately and fixture it.
9. **Tasks 9 and 18: whether `__Host-`prefixed cookies are honoured on `http://localhost`.** The
   prefix requires `Secure` and browsers treat localhost as a secure context, but no primary source
   was fetched for the prefix specifically. It decides only whether decision 5's improved developer
   loop is real.
10. **Tasks 9 and 10: NIST SP 800-63B-4 §3.2.2 (throttling) was not read verbatim**, so the
    recovery-code throttle number is unpinned. So is the apparent tension between §4.2.1.1 (at
    least 64 bits, stored hashed) and §3.1.2.2 (look-up secrets shorter than 112 bits SHALL use a
    password hashing scheme); decision 24 **follows the reading that satisfies both** — codes of at
    least 128 bits hashed with Argon2id — and says so rather than claiming the documents settle it.
11. **Tasks 8 and 11: the three lifetimes no specification sets.** The maximum access-token
    lifetime, which is also the minimum key-rotation overlap window; the session's absolute and
    idle timeouts. OWASP's fetched 2-to-5-minute idle range for high-value applications is the only
    anchor any of them has, and the authorization code's default is this plan's own choice under
    draft-16's RECOMMENDED 600-second ceiling.
12. **Tasks 3 and 19: where the signing key lives, and the Railway volume's mount path.** Both a
    Railway volume file and a Doppler-injected variable fit the fetched platform facts; the project
    rule that every secret lives in Doppler pulls one way and first-boot self-provisioning pulls the
    other (open decision 2). **The mount-path half is settled (2026-10-20): `/data`**, from
    `.railway/railway.ts:58` (`volumeMounts: { "/data": willikinsVolume }`), which this item missed
    by looking only for `railway.json` and `railway.toml`. Decision 25 fixes the layout under it.
    The signing-key half is settled by open decision 2 (Doppler).
13. **Tasks 9, 11 and 19: whether any browser in the operator's environment refuses
    `navigator.credentials` on the deployment's domain**, and since 2026-10-20 **which browser
    context the Claude and ChatGPT iOS apps open `/authorize` in, and whether the passkey ceremony,
    the parked-request resume and the recovery-code path all work there** (decision 20). Untested. The only mitigation the plan's shape offers is the
    recovery-code path, which is why decision 24 refuses to defer it to a later milestone.
14. **Tasks 5 and 19: whether the authorization-server routes must sit outside `allowed_hosts`.**
    The same question the resource-server note raised for the RFC 9728 route, now with
    `/authorize`, `/token`, `/jwks.json` and the second well-known path beside it.
15. **Task 8: whether any `GET` handler on `/approvals` mutates state.** The handlers were not read
    by either research task, and the `Sec-Fetch-Site` rule of decision 4 exempts safe methods.
16. **Tasks 3 and 9: whether the `/approvals` session depends on any artefact derived from the
    signing key.** If it does, a lost key logs every human out as well as invalidating every token,
    and the Risks entry about key loss understates itself.
17. **Task 19: what `Host` header the service receives on public Railway traffic.** Not documented
    anywhere on `docs.railway.com`; it decides `WILLIKINS_ALLOWED_HOSTS` and rmcp's `allowed_hosts`.
    Measure against a live deployment.
18. **Task 19: whether Railway's edge overwrites a client-supplied `X-Real-IP`,
    `X-Forwarded-Host` or `X-Forwarded-Proto`.** Stated for none of the three, so none is
    spoof-proof; `X-Forwarded-For` appears on none of the seventeen fetched pages, which is not
    proof it is absent on a real request. Nothing in this plan reads a forwarded header, and
    nothing may start to before this is measured.
19. **Task 19: the service's public hostname.** It has none at present — the operator deleted the
    generated domain on 2026-09-15 — and a generated one cannot be declared in
    `.railway/railway.ts`. Read it from the dashboard or the CLI when it exists.
20. **Task 19: whether an omitted `domains` key deletes an already-attached custom domain**, what
    `preserve()` does for a variable that does not yet exist live, and whether a push-triggered
    deploy picks up variable edits staged on Railway's variables page. The documented "omit means
    delete" exemption covers only generated domains. Settle all three with a read-only
    `railway config plan` and one observed deploy; never by applying. If staged edits are not
    picked up by a push, the outcome this item used to accept was one failed deployment behind a
    healthcheck that keeps the previous one live. For a volume-backed service that is no longer
    assumed (item 30).
21. **Settled for rmcp 3.3.0; open only for 3.4.0: whether a tool handler can set the HTTP
    response status.** At 3.3.0 it cannot. `jsonrpc_http_status`
    (`rmcp-3.3.0/src/transport/streamable_http_server/tower.rs:626`) maps a handler's error to
    **400**, **404** or **200** and nothing else — so 401 and 403 are unreachable from a handler
    and the scope check stays in the middleware (decision 3). Task 7 needs nothing further.
22. **Task 4: whether a non-`Bearer` `Authorization` header is malformed (400) or absent (401).**
    RFC 6750 §3.1 was not fetched. Today's behaviour is 401 `MissingCredential` and
    `adversarial_10b.rs` pins it; it is kept until the RFC is read.
23. **Before the seam is frozen (decisions 16 and 19, and the pluggable-seam section): two sources
    read at the wrong revision.** The MCP TypeScript SDK was read at **v1.29.0**, the last 1.x tag;
    `releases/latest` now returns a 2.x monorepo whose auth module was not read, and the
    `OAuthServerProvider`/`OAuthTokenVerifier` split the seam leans on may have changed shape
    there. And `draft-ietf-oauth-client-id-metadata-document-00`, which MCP pins in every citation,
    **expired on 11 April 2026**; the working-group document is at revision 02 (2026-07-06) and the
    -00 to -02 diff was not read, which matters the day CIMD is added.

**Added 2026-10-20.** Every Railway item below is settled **by the operator, by hand, in a
disposable environment**, never by an agent and never against the live volume. Every app item is
settled by test 19 with the operator.

24. **Tasks 20 and 19: what `railway volume files` does.** Five questions, none answered by the
    fetched pages:
    - Does `files upload --overwrite` of a directory delete remote files that are absent locally?
    - Is a single-file upload atomic, or can a reader see a partial file?
    - Does `files rename` work on a directory, and atomically?
    - Can `--concurrency` (default 32) expose a half-populated directory to `list_workflows`?
    - Are `files` paths relative to the volume root? The examples (`/backup.tar`, `files list /`)
      suggest so, which would make the container's `/data/workflows` `/workflows` to the CLI.

    Decision 25's procedure stages, checks and swaps precisely because these are unknown.
25. **Task 20: whether `files upload` needs the service running, and works against a distroless
    image; and whether `upload` or `browse` can create a symlink.** Decision 25's directory-symlink
    refusal and `O_NOFOLLOW` hold either way. The second question decides only whether the attack
    is reachable through Railway's own tooling.
26. **Tasks 5 and 19: which `Origin` header, if any, Anthropic's and OpenAI's backends send on
    `initialize`.** No primary source states it. It decides whether decision 18's one-entry
    `allowed_origins` list passes both apps or answers 403. Record it before the list freezes.
27. **Task 19: whether ChatGPT's static-credential form accepts a client id with a blank secret.**
    OpenAI's auth guide implies that method `none` works for predefined clients ("This applies to
    CIMD, DCR, and predefined OAuth clients"), but the form itself was not observed. If it does
    not, the delta is a confidential client and `client_secret_post`, which reopens trust boundary 5
    (open question 5). Related, also unfetched (the help-centre page answered 403): which ChatGPT
    plan tiers allow a custom connector, and whether such a connector is usable from the iOS app.
28. **Task 19: whether ChatGPT steps up on willikins' transport-level 403 `insufficient_scope`.**
    OpenAI documents re-linking only through per-tool `securitySchemes` plus a tool result carrying
    `_meta["mcp/www_authenticate"]`. The 401 naming the three MCP scopes (decision 1) should make a
    step-up rare. If test 19 shows ChatGPT needs the tool-level path, the delta is additive: a
    `securitySchemes` field in each tool's metadata and the `_meta` challenge on a scope refusal.
29. **Tasks 10 and 19: whether `railway ssh`, `scp` or `sftp` reach a shell-less distroless
    container.** It decides decision 25's fallback transport, and where decision 24's host-side
    enrollment CLI and break-glass can run at all (open question 2). The SSH username is the
    service's domain, or its service instance id while it has none.
30. **Task 19: what happens when a new deployment of a volume-backed service fails to start.**
    Railway prevents two deployments from mounting the volume, so redeploys cost downtime. Whether
    a failed new deployment hands the volume back to the old one is not stated. Until it is
    measured, decision 9 step 6 plans for an outage.
31. **Decision 25: whether a read-only mount option exists.** None appears on the fetched pages.
    If one does, `/data/workflows` could be read-only to the process, which is stronger than "the
    server never writes it". That would need a second mount, and a service has one volume, so it is
    probably not available.
32. **Task 19: what each app does when its 3600 s access token expires.** It might re-run OAuth
    inline, or flip the connector to "Reconnect" so the user has to go to settings. That decides
    how heavy open question 1's hourly re-consent really is.
33. **Decision 25: whether a Railway rollback leaves the volume as it is.** The fetched page says a
    rollback "restores a previous deployment's Docker image and its custom variables" and does not
    mention volumes. That it leaves documents and the journal untouched is an inference.
34. **Tasks 7 and 19: which MCP protocol revision each app negotiates against rmcp 3.3.0's
    sessionless server.** If it is 2026-07-28, decision 3's middleware must check `Mcp-Name`
    against `params.name`.
35. **Task 11: whether Chrome and iOS WebKit follow the consent POST's redirect to a registered
    HTTPS callback under `form-action 'self' <that origin>`.** MDN says browsers disagree about
    `form-action` and redirects. Test 22's browser check measures it.
36. **Go-live step 9: Railway volume backups in practice.** The fetched page gives the schedules
    (daily backups "kept for 6 days") and the rules ("Backups can only be restored into the same
    project + environment", "Wiping a volume deletes all backups"). Not established: whether a
    restore is all-or-nothing for the volume (it would then rewind the journal, the credential
    store, the client table and the documents together, which is why decision 25 never restores
    for documents), and whether a restore rehearsal is possible in a disposable environment.

## Risks

**The one the operator already knows: this is the security-critical part of the system and
willikins is writing it rather than buying it.** Stated in terms of what is exposed and to whom.

*What becomes anonymously reachable on a public domain with no edge rate limit:* `/authorize`,
`/token`, `/jwks.json`, both well-known documents, and the passkey ceremony endpoints. Before this
revision the only anonymous surfaces were the protected-resource-metadata document and a 401.

*What a bug in issuance buys an attacker:* a token minted with the operator's `sub` and
`willikins:apply` — which is the eight MCP tools against the **one** GitHub organization and
**one** Doppler workplace the deployment's two credentials cover, until milestone 3's credential
routing lands. **Bounded by three things that do not depend on the issuing code being correct:** an
unlisted `sub` is refused in the middleware before any tool runs (decision 3); anything `apply`
touches that is not auto-approved still needs a human at the approvals page; and no caller token
has ever reached an upstream API, because the `Credential` methods in `willikins-providers-http`
are the only sites that put a credential on an outgoing wire. That third boundary is the
resource-server half, which this revision leaves untouched.

*What makes it acceptable:* the profile willikins is building is kanidm's required profile — one
grant, mandatory PKCE `S256`, one signing algorithm — **minus its client-authentication
requirement**, because MCP clients here are public clients with loopback redirects and draft-16
accepts exactly that given mandatory PKCE. At the size the research note measures: roughly 4,000 to
6,000 implementation lines plus about 1.4 times that in tests, against a corpus where rauthy's
protocol machinery is about 7,300 lines and Cloudflare's *complete* MCP authorization server is
5,967 with the login declared out of scope. The alternatives were a 36,230-line six-week-old
unaudited crate by one author, or a vendor identity product every self-hoster of willikins would
inherit. And the one independent audit of a comparable Rust identity provider found **1 Elevated,
3 Low, none in the OAuth grant logic** — two of the four being Rust-shaped rather than
protocol-shaped, a timing oracle and a reachable `unwrap()` on a cancelled request, which is a
useful calibration for where to point pass 3.

*What would make it unacceptable:* shipping without adversarial pass 3, or without a negative
fixture per code binding held **in the store**. The whole argument is the one fetched post-mortem:
the binding that was lost was lost by a rewrite that kept the comment claiming it happened, and the
professional audit running in the same window missed it.

**The exposure is not symmetric with what 2c originally proposed.** Before this revision, a bug in
validation refuses a good token — annoying, safe. After it, a bug in issuance mints a bad one. Pass
3 must attack the issuing side at least as hard as the validating side, and the task order puts it
after everything except the transport rewrite for exactly that reason.

Then, in descending order of how likely each is to surprise someone:

- **OpenSSL enters the build.** `webauthn-rs-core` depends unconditionally on `openssl` and
  `openssl-sys` with no pure-Rust backend and no feature to turn it off; the Dockerfile's own "no
  `aws-lc-sys`, `openssl-sys` or `cmake` anywhere in the tree" comment becomes **false** and must
  be rewritten; the builder needs `libssl-dev` and `pkg-config`; and whether the distroless runtime
  image ships the matching `libssl.so.3` is **build-blocking** (verify item 2).
- **`rsa` is still compiled in** under `jsonwebtoken`'s `rust_crypto` feature bundle, and
  RUSTSEC-2023-0071 has `patched = []` deliberately. Its dismissal now survives **only** on
  decision 22's normative constraint that willikins never signs with the RSA family. A future
  `cargo-audit` or `cargo-deny` gate needs a documented ignore with a **rewritten** justification;
  the old one, which rested on "a resource server only verifies", is void.
- **The RP ID can never change.** Harder than the audience freeze and with no migration path at
  all: `webauthn-rs` 0.5.5 has no Related Origin Requests support, and that would not solve a
  domain *change* anyway. A domain move orphans every credential and every human re-enrols.
- **First durable identity state.** The credential store is a backup-and-restore concern this
  project has never had; the journal is append-only and is not a credential store. Railway further
  constrains it: one volume per service, no replicas with volumes, and downtime on redeploy.
- **Key loss is a total outage of every outstanding token**, and if the key is not persisted that
  happens on every restart. Verify item 16 asks whether it logs every human out as well.
- **Milestone size.** The task count roughly doubles and the human half is where the cost is:
  rauthy is 84,583 lines, of which about 7,300 is the protocol and about 77,000 is users,
  passwords, passkeys, MFA and the rest. This is a milestone and it is not a week.
- **`webauthn-rs`' MSRV is exactly 1.88**, the workspace floor, with zero headroom — as is
  `jsonwebtoken` 11's. Any bump in either raises the workspace's declared floor.
- **No external audit and effectively one author.** Two Rust authorization servers converging
  independently on constant-time secret comparison, one of them post-audit, is the one thing in the
  corpus that is settled rather than rediscovered. Everything else is judgement.
- **Browser dependency.** If any browser in the operator's environment refuses
  `navigator.credentials` on the deployment's domain, the recovery-code path is the only mitigation
  the plan's shape offers — which is why decision 24 refuses to defer it.
- **A stolen session cookie is valid until its TTL expires, the idle timer fires, the browser logs
  out, or the credential changes.** The last of those is new and is the real revocation: a
  credential change flushes every live session for that account (decision 4). Beyond it there is
  still no operator-side "kill this session" surface, so the fallbacks are waiting out the shorter
  of the two timeouts, redeploying, or removing the subject from `WILLIKINS_APPROVER_SUBJECTS`.
- **A stolen access token is valid until it expires**, and there is no revocation endpoint. With no
  refresh tokens, that window is exactly `WILLIKINS_ACCESS_TOKEN_TTL_SECONDS`, which is the whole
  reason the lifetime is a decision rather than a default.
- **Sessions, ceremonies, authorization codes and the cookie signing key are process memory, and
  the deployment is one replica.** `.railway/railway.ts` pins
  `replicas: { "europe-west4-drams3a": 1 }`, which is what makes an in-memory session store correct
  at all — and note the new failure it also prevents: with a startup-generated cookie `Key`, a
  session minted on replica A is a **forgery** on replica B. Scaling out needs a shared store and a
  configured signing key first, and that is a milestone 3 decision, not a dial to turn.
- **Sessions, codes and nonces die on a redeploy.** Approving after a redeploy means logging in
  again, and an authorization code in flight is lost. The journal is durable and a pending plan is
  unaffected; the README gains the line.
- **The audience is stable while it is published, and moving it costs a migration.** RFC 9728 §3.3
  makes the published `resource` byte-identical to the URL a client used. Mitigation, in two
  halves: the host is one the operator owns, so a move off Railway does not move the identifier at
  all; and a move of the identifier itself is a documented window through
  `WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`. Decision 12 records why that variable does most of its work
  on the adapter path.
- **Replacing `axum::serve` (task 17) is a transport rewrite, not a line.** Graceful shutdown,
  connection accounting and the run-drain bound all move into hand-written code, and hyper 1.11
  panics outright if `header_read_timeout` is set without a timer. It is sequenced after
  adversarial pass 3 deliberately, so a regression there cannot be confused with a finding about
  authentication.
- **A plain-HTTP POST to the public domain is silently converted to a GET** at Railway's edge. An
  MCP client, an approvals form or a `/token` request that reaches `http://` does not fail loudly;
  it gets a method it did not send. Mitigation: `WILLIKINS_PUBLIC_URL` is `https://` and the README
  says so; a GET at `/mcp` is already a 405 from rmcp, which is the visible symptom.
- **There is no per-IP rate limit at Railway's edge**, and the anonymous surface just grew.
  `WILLIKINS_AGENT_SUBJECTS` bounds the set of principals that can reach a per-principal bucket at
  all, but what is attacker-reachable *before* authentication is now signature verification, an
  Argon2 hash on the recovery path, and a WebAuthn ceremony — which is why decisions 20 and 24 each
  carry their own bound (an endpoint rate limit, a semaphore, a store cap) rather than leaning on
  one. A per-IP limiter cannot be built until verify item 18 settles whether any forwarded header
  is trustworthy, and pass 3 measures what a flood actually costs.
- **Duplicate dependency majors and build time**, unchanged in kind and larger in degree.
  `Cargo.lock` today holds one `base64` (0.23.1), one `sha2` (0.11.0), two `rand` majors and three
  of `getrandom`. `jsonwebtoken` 11 and `axum-extra`'s signed-cookie feature want `base64` ^0.22
  and `sha2` ^0.10; `p256` forces `rand` 0.8 as a third major; and `argon2`, `password-hash`,
  `webauthn-rs` and its two siblings join what the crypto backend already pulls. Cargo compiles all
  of them, which is minutes of cold build on an 11 GB host and a wider surface for the day an audit
  gate exists.

## Notes for milestone 3

- **Client ID Metadata Documents**, when a named client needs one. It is graded SHOULD, it stores
  nothing, and its client ids are the **only** ones that survive a later change of authorization
  server — which is the one argument for it that is not about convenience. The evidence that a real
  client probes for it is a single code comment in rauthy naming `claude.ai`. If it is taken, its
  outbound fetch needs a narrowly typed fetcher with private-address and loopback refusal, HTTPS
  only and a 5 KB cap, not a general HTTP client, and decision 20's narrowing of the "no raw URLs"
  invariant is what makes that legible.
- **Dynamic client registration: do not build it, ever.** MCP grades it MAY, explicitly deprecated
  and retained for backwards compatibility. It is an anonymous write endpoint, a persisted client
  registry, a garbage collector and a rate limit, and deferring it costs nothing.
- **Refresh tokens and a revocation endpoint**, together, because they are the same trade. Issuing
  no refresh token deletes the only stateful authorization-server requirement there is — rotation
  with reuse detection and family revocation — and every one of rauthy's worst fetched problems
  lived on that path. The cost is that the access-token lifetime is exactly how often a human
  re-consents, and that an operator who believes a token leaked waits the lifetime out. Revisit
  both at once or neither. **Banked at `todos/2026-09-16-unattended-agents-token-longevity.md`**,
  which carries the pairing rule, the cost to an agent meant to run without a human, and the three
  ways out: a longer per-client lifetime (the client table already has the column), refresh plus
  revocation together, or capability tokens with offline attenuation (`biscuit-auth`, macaroons),
  which suit willikins' shaped authority — a plan is a fixed set of operations against named
  resources — and which are unverified beyond the crate's existence.
- **Automated key-rotation scheduling.** The *mechanism* is in this milestone (decision 22); the
  cron is not. Rotation is an operator action until someone asks for it, and no RFC prescribes a
  schedule.
- **Pluggable authentication**, banked at `todos/2026-09-16-pluggable-auth-adapters.md`. The seam
  section below the decisions says what must stay true for it to be an addition rather than a
  rewrite. Its default shape is the standards-track one: keep willikins as the token issuer and add
  a grant that trusts an external assertion, which is how MCP's own Enterprise-Managed
  Authorization extension splits. Read
  `draft-ietf-oauth-identity-assertion-authz-grant` before freezing anything.
- **Credential routing is unchanged by this milestone.** One GitHub organization, one Doppler
  workplace, per go-live step 11. The proposed shape stays what milestone 2's notes recorded: route
  by `GitHubOrg` to `WILLIKINS_GITHUB_TOKEN_<ORG>`, add a `DopplerWorkplace` input for the Doppler
  half, and prefer a GitHub App minting short-lived installation tokens. The OAuth principal this
  milestone introduces is what a per-organization authorization decision would key off, which is
  the reason to keep `subject` and `issuer` in the journal now.
- **A multi-tenant resource identifier.** If milestone 3 ever exposes per-organization resources,
  RFC 8707 §3 says the tenant belongs in the resource URI. That is a second audience, not a second
  server, and it is easier because the identifier chosen here already has a path.
- **Multi-issuer configuration**, if a deployment ever needs to trust two issuers at once —
  issuer-qualified allowlist entries and a configuration block to match (decision 3).
- **Multi-replica sessions.** A shared session store and a configured cookie `Key`, in that order,
  before `replicas` moves off 1.
- **Sender-constrained tokens** (DPoP, mTLS) if the threat model ever includes a stolen bearer
  token. Neither RFC was fetched.
- **Token introspection**, only if an adapter's provider issues opaque tokens. RFC 7662 was never
  fetched; the cost is a network round trip per MCP request inside a 30 s budget.
- The remaining pass-2 hand-overs: validating tool outputs against declared port types and the
  distinguishable redaction marker (item 5), a `journal_version` field (7), `willikins journal
  repair` (8), and revisiting the concurrency bound with a real measurement (4). Item 6, measuring
  the journal fold, is **not** on this list: pass 3 measures it (decision 1), so what milestone 3
  inherits is the decision about caching, with a number already in hand.
- **Edge rules** could fence `/approvals` by IP before a request reaches the service, but Client IP
  matching is IPv4-only and they need a plan allowance. Under Attack Mode cannot be used while
  `/mcp`, `/approvals` and the authorization-server routes share a domain, because it turns away
  every non-browser request — and three of those routes now *must* answer a non-browser client.

## Decisions the operator settled, and what each delta would have been

Four questions stood open when this plan was rewritten. The operator settled all four on
2026-09-16, and the plan is written at those settings throughout — the task table, the pin table
and the variable table are not conditional. This section keeps the reasoning and the delta of the
road not taken, because a decision whose alternative is forgotten cannot be revisited honestly.

1. **The human login method: passkeys, through `webauthn-rs` 0.5.5.** Operator: "Passkeys are good
   enough." *The reason:* this deployment has no email and no SMS, so a password path has **no
   reset**, and losing the reset also removes the account-lockout mechanism's only escape hatch.
   *What it carries:* OpenSSL enters the build, which falsifies the Dockerfile's own comment about
   holding no `openssl-sys`, and task 1 is where that comment is corrected rather than quietly
   left wrong. *The delta if it is ever reversed:* decision 24's ceremonies become a form login,
   `webauthn-rs` and `webauthn-authenticator-rs` leave the pin table, `argon2` hashes passwords
   rather than only recovery codes, the ceremony store may become a stateless signed pre-auth
   cookie, and NIST's SHALL-level compromised-password blocklist arrives, needing either
   third-party egress or a corpus download.

2. **Where the signing key lives: Doppler-injected, as two variables.** *The reason:* the design
   doc's own rule is that every secret lives in Doppler, and a Doppler-injected key makes rotation
   a variable change rather than an edit to a file on a volume nothing backs up. Two variables and
   not one because decision 22's overlap must be expressible: a single variable makes a rotation a
   flag day in which every outstanding token is refused at once.
   **On Doppler's secret version history**, which the operator raised against this shape: it is an
   audit and recovery feature — past values are viewed from the secret's row in the dashboard, and
   historical values can be redacted irreversibly (<https://docs.doppler.com/docs/secrets>, fetched
   2026-09-16). It does not deliver two values to one running process: the integration injects the
   current value of a variable, so a process that must accept tokens signed by the outgoing key
   still needs that key present. What version history *does* settle is the rollback: a rotation
   that installs a bad key is recoverable from the dashboard without willikins keeping a copy, so
   the retiring-key variable exists only for the overlap and may be cleared as soon as the window
   closes. The interaction that makes the overlap worth its variable is decision 20's: **there are
   no refresh tokens**, so an outstanding token that is refused does not refresh — it sends its
   holder back to `/authorize`, which needs a human. Without the overlap, rotating the key demands
   one human consent per client, immediately, which is the kind of cost that makes an operator
   rotate less often.

3. **Client registration: pre-registration, one configuration entry per client.** *The reason:* for
   a handful of agent clients it costs zero new endpoints and zero attack surface, where a client
   ID metadata document is an outbound fetch of an attacker-chosen URL and dynamic registration is
   an anonymous write endpoint with a garbage collector. *The delta toward metadata documents:*
   `client_id_metadata_document_supported` must be advertised (decision 23), the narrowly typed
   fetcher of the milestone-3 note is required, and verify item 23's expired draft matters
   immediately.

4. **The access-token lifetime: 3600 seconds.** *The reason:* with no refresh tokens the lifetime
   is exactly how often a human must re-consent at `/authorize`, and an hour is the shortest span
   that does not interrupt a working session. It is also decision 22's key-rotation overlap window,
   so the two numbers are one decision and move together.

**Not operator decisions, recorded here so they do not become a fifth.** `SameSite=Strict` versus
`Lax` is a **browser measurement** (verify item 7), not a preference: measure whether `Strict`
withholds the session cookie on an externally-initiated top-level navigation to `/authorize` before
flipping anything, and default to `Lax` until it is measured. Whether `localhost` joins `127.0.0.1`
and `[::1]` in the port-relaxed redirect set is a fixture decision for task 11 (verify item 8).
Whether `__Host-` cookies are honoured on `http://localhost` decides only whether decision 5's
improved developer loop is real (verify item 9).

## Review resolutions

How each finding of the 2026-09-16 document review was resolved, and then how the 2026-09-16 scope
revision moved them. Reviewers: coherence, feasibility, security-lens, scope-guardian, adversarial.
Findings from two or more reviewers on the same point are merged; the reviewers in brackets are who
raised it. Seventy-five findings merged into entries 1 to 57; entry 58 is the scope revision, and
says which of the fifty-seven it supersedes.

**Entries 1 to 57 are kept verbatim as the record, so they cite the task and verify-item numbers
the delegating draft had.** The key, once, rather than fifty-seven edits that would falsify the
record: tasks **3 → 4**, **5 → 6**, **6 → 7**, **7 → 9**, **8 → 12**, **9a → 17**, **9b → 13**,
**9c → 14**, **9d → 15**, **10 → 16**, **11 → 18**, **12 → 19**; tasks 1, 1b and 2 keep their
numbers. Verify items were renumbered wholesale against the merged list above, and three of the
delegating draft's — its items 3, 6 and 11, all about choosing or characterising an external
provider — are **removed** rather than renumbered, so a resolution citing one of those is citing a
question this plan no longer asks.

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
    right steps. *Corrected 2026-10-20:* the body did not match this entry. Decision 9 said "Nine
    steps" but numbered them 1, 6, 7, 8, 9: steps 2 to 5 had been lost, and step 5's cutover
    sub-list was left nested under step 1. They are restored from commit 656613a. The 2026-10-20
    addendum then inserts the volume-and-clients step (new 5) and the connect-the-apps step (new
    10), so steps 5 to 9 of the 2026-09-16 text are now 6, 7, 8, 9 and 11.
57. **The environment-variable count had to be restated** (scope, residual). After removing
    `WILLIKINS_OAUTH_ACCEPTED_TYP`, `WILLIKINS_OAUTH_AUDIENCE` and `WILLIKINS_OAUTH_REDIRECT_URI`
    and adding `WILLIKINS_AGENT_SUBJECTS` and `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS`, the count
    goes from eighteen to **seventeen added, ten required in http mode and seven optional**, and
    every tunable's row names the component that reads it.

58. **The 2026-09-16 scope revision** (the operator, then
    `docs/research/2026-09-16-m2c-scope-revision.md`). **The fifty-seven resolutions above still
    stand wherever they concern the resource-server half**, which this revision leaves untouched —
    RFC 9068 validation in its fixed order, the check-to-`AuthFailedReason` table, the per-reason
    journal bucket and its `suppressed` field, the RFC 9728 document, the 401 and 403 challenge
    shapes, the four scopes and their hierarchy, both subject allowlists, the derived principal and
    its additively journalled claims, the four exposure tasks, and the shape of the go-live
    sequence. Thirty-four of them are untouched — resolutions 1, 2, 3, 7, 12, 13, 14, 15, 16, 17, 20,
    21, 22, 23, 25, 26, 27, 28, 32, 33, 34, 36, 37, 38, 39, 40, 44, 46, 49, 50, 51, 52, 55 and 56 —
    and none of them is reversed. The remaining twenty-three are below. **What the new shape
    supersedes, named one by one so no reader follows a dead resolution:**

    - **4** keeps its rule and changes its subject. There are no provider URLs left to be
      `https://`-or-loopback, so the rule now governs `WILLIKINS_PUBLIC_URL` — which is
      simultaneously the issuer identifier, the audience's stem and the RP ID's source, and which a
      test or a hand-run loopback server must be able to set to `http://127.0.0.1:<port>`
      (decision 7). It is still permanent and still not test-gated.
    - **5** (`client_secret_basic` through a new `Credential::authorize_basic`) — **gone with its
      subject.** The approvals page is no longer an OAuth client of anyone, willikins included, so
      there is no client secret, no token exchange, no timeout on it, no new `Credential` method
      and no `clippy.toml` or `expose_secret_guard` exemption. Trust boundary 2 reverts to its
      milestone 2 form.
    - **6, in its OAuth half only.** The rule it established survives verbatim — `/approvals` never
      inspects `Authorization`, and a request without a valid session gets the identical response
      whatever its headers — and so do both bounded stores, the 503, logout, the `SameSite`
      reasoning, the `WrongRole` page and the CSP. What goes is the `state` value, the callback and
      its `iss` check. The `__Host-willikins-login` cookie is **re-founded, not removed**: it binds
      a passkey **ceremony** rather than an OAuth state, and it closes the same two-approver attack
      in its WebAuthn form, which is CVE-2026-69199's shape.
    - **10, 11, 18 and 24** are overtaken on dependency facts. The `aws_lc_rs` blocker is **`g++`**
      and not cmake; openssl enters the build regardless, through `webauthn-rs-core`; the
      `tower-sessions` rejection reason was **false** (`memory-store` is a default feature) and
      four real ones replace it; and `ureq` no longer moves into `[dependencies]`, because decision
      16 removes the fetch that would have justified it. Resolutions 10 and 11 keep their substance
      for tasks 14 and 17, which this revision does not touch.
    - **9 and 53** hold exactly, on the same `Copy`-and-pure reasoning — with
      `StartError::JwksUnavailable` replaced by decision 6's two runtime refusals, since the
      startup JWKS fetch it named no longer exists.
    - **8** stands as a mechanism and changes its contents. `.railway/railway.ts` still drops the
      two retired `preserve()` rows, still gains one row per new `serve --http` variable, and still
      requires a read-only `railway config plan` showing no variable delete before any apply — but
      the rows it gains are the new variable set, and the same change now also carries the signing
      key and the credential-store path (go-live step 6 since the 2026-10-20 renumbering).
    - **19** survives with a different subject: the `live-tests` feature is now test 18's
      conformance harness rather than a live provider token.
    - **29 and 31** lose their subjects. There is no provider to recommend and therefore no
      contradiction between decision 10 and Open decisions (29), and no provider metadata document
      for the fixture to fake (31) — though 31's *reasoning*, that fixture surface with no consumer
      is not worth building, is what decides decision 7's revised fixture too.
    - **30 and 35** change in kind. The goal's discovery-and-registration sentence is now split by
      decision 23: discovery works for a stranger client, registration does not, and test 18 is
      written about exactly that rather than about CIMD or dynamic registration.
    - **41** loses its subject: there is no authorization request to willikins' own endpoint from
      willikins, so "send `resource` on the token request too" applies to the *client* half of the
      flow, which is now every MCP client's job and decision 20's to enforce.
    - **42** survives and is demoted to the second path: the `WrongRole` page still shows the
      session's own `sub` and `iss`, but the first path is now the enrollment CLI printing the
      subject (go-live step 4).
    - **43 and 45** keep their conclusions on replaced ground. `Lax` is still the answer, but
      because the consent page is reached by an externally-initiated top-level navigation and an
      adapter's callback would need it — not because of a callback willikins still has — and it is
      now a measurement to take before flipping rather than an argument to settle (decision 4,
      verify item 7). Login CSRF keeps its pass-3 coverage with the ceremony in the callback's
      place: a ceremony started in one browser and finished in another. One half of 43 is
      **reversed** rather than re-founded: the per-plan nonce is promoted from defence in depth to
      **load-bearing and session-bound**, because `SameSite` is scoped to the registrable domain
      and every sibling host under `example.com` is same-site.
    - **47** holds except for one clause. The migration it counted is still 11 `HttpConfig::build`
      test sites across six files, the retirement is still split across two tasks, and task 9 still
      flips the `adversarial_10b.rs` pins — but the blocking `start()` it added to the fixture
      loses its consumer with the HTTP JWKS route (decision 7), so spawned-binary tests exercise
      willikins' own issuer only.
    - **48** loses its carve-out. Trust boundary 5's whole paragraph about
      `WILLIKINS_OAUTH_CLIENT_SECRET` goes, because that secret does not exist. What must **not**
      be written in its place is a claim that willikins now holds no secret: it holds a *signing*
      secret, which authenticates nothing to anyone and whose compromise is strictly worse than a
      shared secret's, and that statement lives in trust boundary 1b.
    - **54** keeps its substance and gains a failure it did not name: with a startup-generated
      cookie `Key`, a session minted on one replica is a **forgery** on another.
    - **57**'s count is superseded twice over and is recomputed from the rows of the variable
      table above: fifteen added, six required in http mode and nine optional.
