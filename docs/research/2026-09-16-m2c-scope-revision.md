# Milestone 2c scope revision: willikins issues its own tokens

**Created:** 2026-09-16
**Revises:** `docs/plans/2026-09-16-milestone-2c-authorization.md`. This document is the input the
coordinator folds into that plan. It does not rewrite the plan and it does not restate the parts
that survive.
**Research:** `docs/research/2026-09-16-m2c-own-authorization-server.md` (cited below as
**AS §x**) and `docs/research/2026-09-16-m2c-authorization.md` (cited as **RS §x**). Every fact in
this document is carried from one of those two notes, each of which quotes a primary source
fetched on 2026-09-16. **No source was fetched by this task and no cargo command was run.**
**Feeds:** `docs/plans/2026-09-11-willikins-design.md` — the trust model, and the rule that policy
lives in the workflow and never in the tool.

## The decision this revision is written under

The operator rejected delegating to an external identity provider on 2026-09-16, in any edition,
hosted or self-hosted. willikins is software other people self-host; a dependency on a vendor
identity product is one those people would inherit. So willikins issues its own tokens and
authenticates its own humans, built from Rust crates. Reading a vendor product for its source or
documentation as prior art is encouraged; depending on one is not, and nothing below proposes it.

The same day the operator set the direction this revision designs toward: auth may *later* be a
pluggable system with a default in-house implementation and adapters for other systems, **banked as
a later improvement in a todo**. So the in-house implementation is the only thing built now, and
the work must leave a seam clean enough that an adapter is a later addition rather than a rewrite.
Section 5 states that seam and says what must be true now for it to hold. It deliberately adds no
abstraction the in-house path does not need.

**What survives untouched from the current 2c plan**, so no section below re-derives it: the
resource-server half. RFC 9068 validation in its fixed order, the RFC 9728 protected-resource
metadata document, the 401 and 403 challenge shapes, the four scopes and their hierarchy, the two
subject allowlists, the derived `oauth-`prefixed principal, the claims journalled additively behind
a frozen fixture, the four exposure tasks inherited from adversarial pass 2, and the shape of the
go-live sequence. **What changes is who issues the tokens it validates** — and that turns out to
touch eleven of the plan's eighteen numbered decisions, one of its five trust boundaries twice, and
roughly half its environment table.

Two corrections AS §"Two corrections" records against the earlier research note are load-bearing
here and are repeated so they are not lost in the fold: `tower-sessions`' `memory-store` **is** a
default feature (the crate is still declined, for four other fetched reasons, AS §5.1); and the
RUSTSEC-2023-0071 dismissal in RS §4.3 is **void**, because an issuer signs and the advisory is a
private-key timing leak observable over the network (AS §3.4). Both files are coordinator-owned and
were not edited.

---

## 1. What willikins must implement

Each item names the requirement it satisfies and is marked **[spec]** — a specification MUST, or a
practical requirement a conformant client imposes — or **[deployment]** — something no
specification asks for that this deployment cannot ship without. Where an item is a MUST only
because of a choice made elsewhere in this document, it says so.

### 1.1 Discovery and metadata

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 1 | RFC 8414 authorization-server metadata served at `/.well-known/oauth-authorization-server`, GET, 200, `application/json` | MCP: an authorization server **MUST** provide at least one of RFC 8414 or OIDC Discovery. RFC 8414 alone is conformant and is the cheaper branch — no OIDC provider metadata, no ID tokens (AS §1.2) | [spec] |
| 2 | The document's `issuer` byte-identical to the issuer identifier the URL was built from | MCP: "the `issuer` value in the document **MUST** be identical to the issuer identifier used to construct the well-known URL. If they differ, the client **MUST NOT** use the metadata" (AS §1.2). Holds by construction once decision 12's sibling derives the issuer from `WILLIKINS_PUBLIC_URL` | [spec] |
| 3 | `code_challenge_methods_supported: ["S256"]` | MCP: "If `code_challenge_methods_supported` is absent, the authorization server does not support PKCE and MCP clients **MUST** refuse to proceed" (AS §1.4). There is no runtime PKCE discovery; metadata is the only signal | [spec] |
| 4 | `grant_types_supported` and `token_endpoint_auth_methods_supported` emitted **explicitly** | Their RFC 8414 defaults describe a server willikins is not: omitting the first defaults to `["authorization_code", "implicit"]`, a grant OAuth 2.1 deletes; omitting the second defaults to `client_secret_basic`, which makes every public client appear unsupported (AS §1.3) | [spec] |
| 5 | `authorization_response_iss_parameter_supported: true` | RFC 9207 §2.3: a server supporting the `iss` parameter **MUST** set it (AS §1.6). Paired with item 12 | [spec] |
| 6 | Array claims with zero elements **omitted**, never serialised as `[]` | RFC 8414: "Claims with zero elements MUST be omitted from the response" (AS §1.3). Bites a `#[derive(Serialize)]` struct directly — `skip_serializing_if = "Vec::is_empty"` on every array field | [spec] |
| 7 | `jwks_uri` published and a JWKS route served | RFC 8414 makes `jwks_uri` OPTIONAL and MCP requires no JWKS at all. This is the **seam**: "a resource server that only ever knew how to read a key out of its own process **is** the rewrite" (AS §7 rule 1) | [deployment] |
| 8 | RFC 9728 protected-resource metadata, `authorization_servers` naming willikins' own issuer | MCP: the PRM document **MUST** include `authorization_servers` with at least one entry (AS §1.2). Already in the plan; only the value changes | [spec] |
| 9 | Both well-known URLs built by **insertion** between host and path, never by appending | RFC 8414 §3.1. rauthy's own code comment records a real MCP client probing exactly the insertion form and silently falling back to DCR when it 404s (AS §6.7). With the issuer as a bare origin the two forms coincide for the AS document, but the RFC 9728 document at `/.well-known/oauth-protected-resource/mcp` needs it regardless | [spec] |

### 1.2 The authorization endpoint

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 10 | `GET /authorize` accepting `response_type=code` and nothing else; the implicit grant structurally absent | OAuth 2.1 deletes the implicit grant. Prior art makes the weak mode unrepresentable rather than branched on — `z.literal('code')` in the TS SDK (AS §6.11) — which is the same discipline willikins already applies with domain newtypes | [spec] |
| 11 | PKCE mandatory, `S256` only. A request with no `code_challenge` is refused; an unsupported method answers `invalid_request`; `plain` does not exist | draft-16 §4.1.1: `code_challenge_method` is **REQUIRED**, value `S256` or a future extension; a server **MUST** reject a request with no challenge from a public client. The §7.5.1 carve-out needs a confidential client using the OIDC nonce, which cannot apply here (AS §1.5) | [spec] |
| 12 | `iss` on **every** authorization response, including error responses | RFC 9207 §2 **MUST** for any server supporting it; draft-16 §4.1.2 already lists it REQUIRED where MCP still says SHOULD. One query parameter and one metadata boolean; expensive to retrofit (AS §1.6) | [spec] |
| 13 | Exact redirect-URI matching against the registered set, **with the loopback port exception** | MCP: "**MUST** validate exact redirect URIs against pre-registered values". draft-16 §8.4.2: "The authorization server **MUST** allow any port to be specified at the time of the request for loopback IP redirect URIs" — a desktop MCP client binds an ephemeral port at request time. A literal string-equality matcher is non-conformant; a matcher that ignores the port everywhere is a vulnerability (AS §1.7) | [spec] |
| 14 | Two-phase errors: a bad `client_id` or `redirect_uri` answers **400 directly** and is never redirected | Redirecting a phase-1 error is an open redirect. The TS SDK's own comment states the split (AS §6.11) | [spec] |
| 15 | A consent screen that **clearly displays the redirect URI hostname**, with an additional warning for `localhost`-only redirects | MCP security-considerations: "**MUST** clearly display the redirect URI hostname during authorization"; "**SHOULD** display additional warnings for `localhost`-only redirect URIs" (AS §1.10) | [spec] |
| 16 | `resource` accepted at `/authorize`, bound into the authorization code, and anything that is not the one canonical resource identifier refused with `invalid_target` | RFC 8707: the AS "SHOULD audience-restrict issued access tokens to the resource(s) indicated"; it **MAY** fail a request that omits the parameter with `invalid_target`. This is the hinge between the two halves in one process: the `aud` the AS writes here is the `aud` the RS already validates (AS §1.8) | [spec] for binding, [deployment] for the strictness |
| 17 | Default rate limits on `/authorize` and `/token`, and `Cache-Control: no-store` set before anything else | Both endpoints are anonymously reachable on a public domain that has no edge rate limit (plan Risks). Cloudflare's shipped MCP authorization server defaults to 100 and 50 per 15 minutes (AS §6.11) | [deployment] |
| 17a | **Every authorization-server refusal is journalled**, through the same machinery decision 1 already specifies: new **additive** `AuthFailedReason` variants (unknown client, redirect-URI mismatch, missing or `plain` code challenge, PKCE verifier mismatch, code replayed, code expired, resource widened, ceremony/account mismatch, recovery code replayed), with the refusals of an **unauthenticated** request passing through decision 1's per-reason token bucket and its `suppressed` field | `/authorize`, `/token` and the ceremony endpoints are anonymously reachable, so every refusal on them is a journal append an attacker can trigger — the exact shape decision 1 already closed for `/mcp`. Additive only: no variant is removed, per the journal's replay discipline | [deployment] |
| 17b | The design doc's invariant "**no tool may take a raw URL, shell command, or arbitrary API path as input**" governs **tool ports**, not the authorization server's own protocol machinery — stated in the plan rather than left to a reviewer | `/authorize` takes a client-supplied `redirect_uri`, which reads as a violation unless the plan says otherwise (AS §1.10). The narrowing that makes it true: a `redirect_uri` is **matched against the pre-registered set and never dereferenced, fetched or followed** by the server. Should CIMD ever land, its outbound fetch needs a narrowly typed fetcher with private/loopback refusal, HTTPS-only and a 5 KB cap — not a general HTTP client | [deployment] |

### 1.3 The token endpoint

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 18 | `POST /token`, `grant_type=authorization_code` only | No other grant is needed; each one not implemented is one that cannot be attacked | [deployment] |
| 19 | The authorization code is **single-use, claimed by an atomic get-and-remove**, lifetime ≤ 10 minutes, and its **own expiry checked** independently of any store TTL | draft-16 §4.1.2: "A maximum authorization code lifetime of 10 minutes is RECOMMENDED." A find-then-delete leaves a window in which two concurrent token requests redeem one code; rauthy claims with `get_remove` and still re-checks `code.exp` against the clock (AS §6.4) | [spec] for lifetime and single use, [deployment] for the atomicity |
| 20 | The code record **stores** its bindings — `client_id`, `code_challenge`, `redirect_uri`, `resource` — and each is re-checked at redemption | draft-16 §4.1.2: "The authorization code is bound to the client identifier, code challenge and redirect URI." RFC 6749 §4.1.3: the token request's `redirect_uri` and the authorization request's "values **MUST** be identical" — a separate obligation from re-checking the registered set (AS §1.7, §6.5). **In the store, not only in a claim**: GHSA-7qh2-3hc5-2vqp was a rewrite deleting the last copy of a binding while keeping the comment that claimed it happened, and a professional audit running in the same window missed it (AS §6.6) | [spec] |
| 21 | `resource` at the token request may only **match** what the code granted — never widen, never introduce | RFC 8707's reading, and the exact rule a shipped server encodes (AS §6.4) | [spec] |
| 22 | Public clients only; `token_endpoint_auth_method: "none"`; **no client secret anywhere in the deployment** | MCP desktop clients with loopback redirects are public clients (AS §1.5). The one confidential client the old plan had — the approvals login — stops being an OAuth client at all (decision 4 below), so the whole client-secret surface disappears | [deployment] |
| 23 | A code redeemed by the wrong client answers `invalid_grant` **indistinguishably** from an unknown code | Prior art; it turns a wrong-client redemption into a non-oracle (AS §6.11) | [deployment] |
| 24 | **No refresh tokens are issued**, and `offline_access` is never advertised | MCP: clients "**MUST NOT** assume refresh tokens will be issued; the AS retains discretion", and protected resources "**SHOULD NOT** include `offline_access`". Not issuing them is conformant and it deletes the only stateful AS requirement there is — the rotation-with-reuse-detection family table (AS §1.11). Every one of rauthy's worst problems lived on the refresh path (AS §10.9) | [spec permits], [deployment] chooses |

### 1.4 Minting the token

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 25 | An RFC 9068 JWT access token carrying **all seven** REQUIRED claims: `iss`, `exp`, `aud`, `sub`, `client_id`, `iat`, `jti` | RFC 9068 §2.2 — `jti` REQUIRED here even though it is OPTIONAL in plain RFC 7519. There is **no single-party carve-out** for a co-hosted AS (AS §3.9) | [spec] |
| 26 | `typ: "at+jwt"` set explicitly on the JOSE header | RFC 9068 §4, and the validator's frozen `typ` set (plan decision 14). `Header::new(alg)` defaults `typ` to `"JWT"`, so it **must be overwritten** (AS §3.1) | [spec] |
| 27 | `iss` and `aud` are two different strings even though one process plays both roles | RFC 9068 §5: "authorization servers **MUST** use a distinct identifier as an `aud` claim value to uniquely identify access tokens issued by the same issuer for distinct resources." `iss` is the RFC 8414 issuer identifier; `aud` is always a resource identity (AS §3.9) | [spec] |
| 28 | Signed asymmetrically, never `none`, and **never with the RSA family** | RFC 9068 §2.1: access tokens **MUST** be signed and **MUST NOT** use `none`. Never-RSA is this deployment's own normative constraint, because `jsonwebtoken`'s `rust_crypto` feature drags `dep:rsa` in unconditionally and RUSTSEC-2023-0071 (`patched = []`) is a private-key signing-timing leak observable over the network — which now applies, since willikins signs (AS §3.4) | [spec] for the first two, [deployment] for the third |
| 29 | ES256 as the issuing algorithm; RS256 retained in the **validator's** allowlist only, through the per-key intersection decision 15 already specifies | RFC 9068 §2.1: conforming servers "**MUST** include RS256 among their supported signature algorithms." Exit (c) of AS §3.5 satisfies that on the verifying side, where the public-key-only reasoning genuinely holds, while the issuer signs ES256 only. Implementable **only** as per-`kid` selection, because `jsonwebtoken` refuses a `Validation` whose algorithm list spans key families | [spec] + [deployment] |
| 30 | `kid` set on **both** the JOSE header and the published JWK, to the same value — an RFC 7638 thumbprint | `Jwk::from_encoding_key` leaves `kid` as `None`, and `JwkSet::find` skips keys without one, so a JWKS built the obvious way contains a key the resource-server half can never select and willikins fails to validate its own tokens. `jsonwebtoken` 11 computes the thumbprint natively, in RFC 8037's lexicographic member order, so no registry is needed for the two halves to agree a name (AS §3.2, §3.3 trap 1) | [deployment] — a self-inflicted MUST |
| 31 | If Ed25519 is ever offered, PKCS#8 **v1** only | `Jwk::from_encoding_key` matches the Ed key length against exactly 48 bytes, and every default generator examined emits v2 (AS §3.3 trap 2). **ES256 has neither trap**, which is the quiet argument for choosing it and never reaching this row | [deployment] |
| 32 | The private key is **persisted**, has a redacted `Debug`, no `Display`, no `Serialize`, and is zeroized | Without persistence every outstanding access token dies silently on every restart: a new key has a different thumbprint, so `JwkSet::find` returns `None` for every older token and RFC 9068 §4 requires `invalid_token` (AS §3.8). Railway redeploys a volume-attached service with downtime anyway, so restarts are not rare. Redaction by construction is the codebase's own invariant — `EncodingKey` already derives `Zeroize, ZeroizeOnDrop` and prints `content: "[redacted]"`, and rauthy encrypts its keys at rest with a redacted `Debug` for the same reason (AS §3.1, §6.4) | [deployment] |
| 33 | A **two-key overlap mechanism**: publish both with distinct `kid`s, sign with the new one from the moment it is published, keep the old until every token it signed has expired, then drop it | RFC 7517 §4.5 is the only normative statement and prescribes no overlap length; RFC 9068 §4 warns that publishing more keys widens the compromise surface, so two live keys is a ceiling and not a steady state. **The overlap window is exactly the maximum access-token lifetime**, which no specification sets (AS §3.7) | [deployment] |

### 1.5 Client registration

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 34 | A **pre-registered** client table: client id, display name, registered redirect URIs, one entry per client | No registration mechanism is a server MUST. MCP grades CIMD **SHOULD**, DCR **MAY and deprecated**, pre-registration no MUST at all — and client preference order puts pre-registration *first* (AS §1.9). For a handful of agent clients this is not a compromise; it is what makes the in-house AS genuinely small (AS §2.7) | [deployment] |
| 35 | `client_id_metadata_document_supported` advertised **only if** CIMD is implemented, which it is not in 2c | The draft makes advertising it a MUST for any AS that implements it and publishes RFC 8414 metadata; the converse is that advertising it without implementing it sends clients to a dead end (AS §1.9) | [spec], conditional |

### 1.6 Authenticating the human

MCP says **nothing whatsoever** about this — no MUST, no SHOULD, no shape (AS §1.12). All three MCP
reference implementations implement the OAuth protocol surface and declare the login itself out of
scope (AS §6.10). Everything in this table is willikins' own, and it is where the real cost of the
milestone is.

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 36 | A **durable credential store**, on the Railway volume beside the journal | A `Passkey` record cannot be in-memory the way sessions and nonces are. It is the first durable identity state willikins would own, and the journal is append-only and is not a credential store (AS §4.12) | [deployment] |
| 37 | Passkey registration and authentication ceremonies through `webauthn-rs` 0.5.5, with the in-progress state **server-side** | The crate refuses to derive serde on the state by default, precisely so it cannot be put in a cookie and replayed. That is the same shape the plan's pending-login store already has — 300 s TTL, pruned on insert, capped, 503 when full — and the crate's own `DEFAULT_AUTHENTICATOR_TIMEOUT` is 300 s (AS §4.3) | [deployment] |
| 38 | Each ceremony **bound to the account it finishes against** | CVE-2026-69199: WebAuthn state stored under a code but not bound to a user id, with the target user taken from a separate argument, so an attacker could sign a challenge for their own account and finish against a victim's (AS §6.7). Same class as the two-approver login-CSRF the plan's decision 4 already closes | [deployment] |
| 39 | `UserVerificationPolicy::Required`, `require_valid_counter_value`, `Passkey::update_credential` on every successful authentication, `exclude_credentials` at registration, and an assertion that the registered `CredentialID` has not previously been registered to any other account | UV Required is the crate's default and is what makes the crate's claim true that a passkey is self-contained MFA, so no password is needed alongside it. The counter check aborts with `CredentialPossibleCompromise` and tolerates 0-on-both-sides, which is normal for synced passkeys. The CredentialID uniqueness assertion is the caller's obligation and the crate cannot enforce it (AS §4.3, §4.5, §4.6) | [deployment] |
| 40 | `backup_eligible` and `backup_state` **recorded, not refused** | Refusing cloud-synced passkeys is not reachable through the safe API anyway, and it would refuse the most likely recovery path for a single operator who loses a device. NIST treats the flag as policy input, not a blanket rejection (AS §4.5) | [deployment] |
| 41 | **Recovery codes**: ≥128 bits from a CSPRNG, Argon2id-hashed, single-use, a new one issued after each use | NIST SP 800-63B-4 §4.2.1.1: a recovery code **SHALL** include at least 64 bits, **SHALL** be stored hashed, and after use the CSP **SHALL** invalidate it and issue a new one. AS §4.10 follows the ≥128-bit-with-Argon2id reading because it satisfies both §4.2.1.1 and §3.1.2.2, and the plan should say it is following that reading. **This puts `argon2` in the tree on the passkey branch too** — it is not a password-only dependency | [deployment] |
| 42 | **Two credentials enrolled before the first enrollment is complete**, or the operator is nagged until a second exists | Prevention before recovery. Both NIST and the crate's own docs treat multi-credential enrollment as the relying party's job (AS §4.10) | [deployment] |
| 43 | A **host-side break-glass**: a CLI subcommand on the host that mints a one-time enrollment URL to stdout, journalled as loudly as an approval | Consistent with the design doc's own trust model — the boundary is the network, and whoever holds host, environment and Doppler access already controls the deployment. What a deployment must **not** do is add a fourth layer that reintroduces a static shared secret, which trust boundary 5 forbids (AS §4.11) | [deployment] |
| 44 | The first `<script>` on the approvals page, inlined, under a `script-src` with a per-response nonce or hash | The ceremony runs through `navigator.credentials.create()`/`.get()` and the page must base64url-decode the challenge, `user.id` and every credential id before the call and re-encode five fields for the POST back — about 100 lines. willikins' approvals page contains no `<script>` today, so 2c's CSP (`frame-ancestors 'none'`, `form-action 'self'`) **must grow a `script-src`** (AS §4.8) | [deployment] |
| 45 | A username step that answers **uniformly** | `start_passkey_authentication` takes the account's own credentials, so the server must know which account before it can build a challenge; the usernameless alternative needs the preview `conditional-ui` feature the authors advise against. For a single operator that is one text field, but it makes the endpoint an account-existence oracle unless it answers uniformly (AS §4.8) | [deployment] |
| 46 | The WebAuthn **RP ID frozen** at `WILLIKINS_PUBLIC_URL`'s host | `rp_id` cannot be changed without breaking every associated credential, and `webauthn-rs` 0.5.5 has **no** Related Origin Requests support — which would not solve a domain *change* anyway. There is no analogue of decision 12's audience-migration path (AS §4.4) | [spec], and it is the hardest freeze in the milestone |

### 1.7 Session, now that willikins runs the login

Everything the plan's decision 4 specifies about the session cookie survives — `__Host-` prefix,
`Secure`, `HttpOnly`, `Path=/`, no `Domain`, signed through `axum-extra`'s `SignedCookieJar`, an
opaque id over a bounded server-side store, both stores TTL'd, pruned on insert, capped at 1,024 and
answering 503 when full, logout, `X-Frame-Options: DENY`, the CSP, the per-plan nonce, the
Origin/Referer check, and the `WrongRole` page showing the session's own subject. Five things are
**newly required** because willikins now runs the login, and they are all OWASP-cited:

| # | What lands | Why | Mark |
| --- | --- | --- | --- |
| 47 | **Unconditional session-id rotation at login**, destroying the previous id | Session fixation is a new surface: under delegated OAuth the session was born fresh at the callback; a login that sets any pre-authentication cookie creates the classic target. OWASP: the id "must be renewed or regenerated ... after any privilege level change", and `axum-login`'s guarded `cycle_id()` is the shape **not** to copy (AS §5.2, §5.4a) | [deployment] |
| 48 | An **idle timeout** beside the absolute one, bumped on **reads** as well as writes, split into its own variable | `WILLIKINS_SESSION_TTL_SECONDS` is absolute only, so an unattended browser can approve for a full hour. OWASP requires both, and names 2–5 minutes as the idle range for high-value applications — which a page whose one action is approving a plan that provisions secrets is by its own framing. Bumping on reads is the mistake `tower-sessions`' `OnInactivity` makes (AS §5.4b, §5.1c) | [deployment] |
| 49 | `Cache-Control: no-store` on every response that carries a session id | OWASP: "Unlike `no-cache`, which allows caching but requires revalidation, `no-store` ensures that the response (including headers like `Set-Cookie`) is never stored in any cache." This is also the concrete answer to the caching gap the earlier note flagged from the Railway/Cloudflare side (AS §5.4c) | [deployment] |
| 50 | 128-bit session ids from a CSPRNG, and a **non-persistent** cookie (no `Max-Age`, no `Expires`) | OWASP names both; the plan says "an opaque session id" and neither (AS §5.4d) | [deployment] |
| 51 | **Credential-change revocation**: bind the session to a hash of the credential as of login and constant-time compare it on every request, flushing on mismatch | This is the mechanism that kills every live session when a credential changes — the revocation the plan's Risks section says it does not have. Copy the idea from `axum-login` and decline the crate (AS §5.2) | [deployment] |
| 52 | `Sec-Fetch-Site` rejection of non-safe methods on `cross-site`, with `Vary: Sec-Fetch-Site, Origin`; the Origin match made to run **through the trailing `/`**; and a block when neither `Origin` nor `Referer` is present | OWASP now lists Fetch Metadata as a first-class CSRF defence with a **mandatory** fallback to origin verification, which willikins already has — so it is purely additive. The trailing-slash rule is what stops `willikins.bandeabonnot.com.attacker.com` passing; "if neither header is present ... we recommend **blocking**" (AS §5.6) | [deployment] |
| 53 | The per-plan nonce **bound to the session**, and never written to the journal or a log line | The plan calls it "defence in depth here, not the only defence". That understates it: SameSite is scoped to the **registrable domain**, so every sibling host under `bandeabonnot.com` is same-site to `willikins.bandeabonnot.com`. OWASP: "Always bind the CSRF token explicitly to session-specific data" and it "must not be leaked in the server logs or in the URL" (AS §5.5) | [deployment] |

---

## 2. What can honestly be deferred, and what deferring costs

### 2.1 Genuinely deferrable for a single-operator deployment

| Deferred | Conformance position | What deferring costs |
| --- | --- | --- |
| **Refresh tokens** | Nothing in MCP's refresh section is an AS MUST; "the AS retains discretion" (AS §1.11) | The access-token lifetime becomes exactly how often an MCP client's human must re-consent at `/authorize`. That is open decision 4. It also deletes the only stateful AS requirement there is |
| **Client ID Metadata Documents** | SHOULD, and only for servers that implement it (AS §1.9) | A client with no prior relationship cannot self-register: every new client is one configuration entry the operator adds by hand. It is also the **only** registration form whose client ids survive a later AS swap, so deferring it makes the future adapter marginally more expensive (AS §7). Add it when a named client needs it; rauthy's code comment naming `claude.ai` is the only fetched evidence a real client probes for it (AS §6.7) |
| **Dynamic client registration** | MAY, and explicitly deprecated and "retained for backwards compatibility" (AS §1.1) | Nothing. It is an anonymous write endpoint, a persisted client registry, a garbage collector and a rate limit (AS §2.7, §6.10). **Do not build it, ever** |
| **A revocation endpoint (RFC 7009)** | A case-insensitive grep for `revoc`/`revoke` over all five fetched MCP authorization pages returns **zero hits**; RFC 8414 makes `revocation_endpoint` OPTIONAL (AS §1.12) | An operator who believes a token leaked waits out the token lifetime or redeploys. With a short lifetime and no refresh tokens, that window is the lifetime itself |
| **Consent persistence** ("remember this client") | No requirement | One consent click per authorization. For a handful of clients that is a feature, not a cost |
| **Token introspection and opaque tokens** | RFC 9068 JWTs make introspection unnecessary; the plan already scopes it out | Unchanged from the plan |
| **OIDC discovery, ID tokens, PAR, DPoP, mTLS** | RFC 8414 alone satisfies the discovery MUST (AS §1.2); the rest were never MUSTs | Unchanged from the plan |
| **Multi-replica sessions and a shared session store** | Not a protocol question | `.railway/railway.ts` pins one replica, which is what makes an in-memory store correct. Scaling out needs a shared store and a configured cookie `Key` first — and note the new one: with a startup-generated `Key`, a session minted on replica A is a **forgery** on replica B (AS §5.7) |
| **Automated key-rotation scheduling** | RFC 7517 §4.5 prescribes no schedule (AS §3.7) | The *mechanism* is not deferrable (§2.2 below); the cron is. Rotation is an operator action until someone asks for it |
| **The `oauth-as` re-evaluation** | n/a | A dated note, gated on 1.0, a second maintainer, or an audit (AS §2.5) |
| **Multi-issuer configuration** | n/a | Already the plan's milestone-3 answer (decision 3) |

### 2.2 Looks deferrable and is not

This is the list the coordinator most needs, because every item on it is cheap now and expensive or
impossible later.

1. **Recovery codes and the break-glass CLI, in this milestone.** There is no password reset and no
   email or SMS side channel, so there is no other path back in. If any browser in the operator's
   environment refuses `navigator.credentials` on the deployment's domain, the recovery path is the
   *only* mitigation in the plan's shape — which is exactly why it must land in the same milestone
   rather than after it (AS §4.9, §4.11, §9.4).
2. **Key persistence.** Deferring it is not "we will add it later"; it is "every outstanding access
   token dies silently on every restart", a zero-overlap rotation forced on every deploy (AS §3.8).
3. **The two-key JWKS overlap *mechanism*.** The schedule is deferrable; the ability to publish two
   keys at once is not. Retrofitting it means a flag day where every outstanding token is refused.
4. **Publishing `jwks_uri` and serving a JWKS.** OPTIONAL by the RFC, mandatory by the seam. AS §7
   names skipping it as the thing that *is* the rewrite.
5. **`iss` in the authorization response, plus its metadata boolean.** One query parameter and one
   boolean. Clients that check it start refusing responses that lack it the day MCP raises SHOULD to
   MUST, which its own text says is expected (AS §1.6).
6. **`kid` on both the header and the JWK.** Skipping it does not degrade gracefully — willikins
   cannot validate its own tokens at all (AS §3.3 trap 1).
7. **Session-id rotation at login, the idle timeout, `no-store`, 128-bit ids.** Each is a few lines
   now and a security review finding later, and rotation in particular cannot be bolted on after a
   session shape has been frozen in tests (AS §5.4).
8. **The two-credential enrollment rule.** A single-credential deployment is one lost device away
   from the break-glass, every time.
9. **A negative fixture per code binding — client, challenge, redirect_uri, resource — asserting the
   binding lives in the *store*.** This is the repo's own convention applied to the one place a
   fetched post-mortem says it matters: a rewrite deleted an equality check, kept the comment
   claiming it happened, and an independent professional audit running in the same window did not
   find it (AS §6.6). Deferring these fixtures is deferring the only thing that would catch that
   class of regression.
10. **The consent screen's redirect-hostname display.** It is a flat MCP MUST and it is the only
    thing standing between a user and an attacker-registered redirect (AS §1.10).
11. **`code_challenge_methods_supported`, `grant_types_supported` and
    `token_endpoint_auth_methods_supported` in the metadata.** Absence of the first makes every
    conformant client refuse to proceed; absence of the other two makes the document actively lie
    about the server (AS §1.3, §1.4).
12. **The atomic get-and-remove claim on the authorization code.** A find-then-delete is a
    double-redemption window that appears only under concurrency, which is where it will not be found
    by a test written later (AS §6.4).

---

## 3. Which numbered decisions change, and to what

The plan's numbering is what the coordinator edits against, so every one of the eighteen is named,
in order, including those that do not move.

**1. Resource server only — survives, minus one clause.** The validation set and its fixed order,
the check-to-`AuthFailedReason` table, the 401/403 split, the per-reason journal token bucket with
its `suppressed` field, the PRM document, the `WWW-Authenticate` shape and the passthrough
prohibition all stand **unchanged**. What leaves is the heading's second sentence, "It issues
nothing" — that claim belongs to trust boundary 1, which is rewritten (appendix B), not to the
validation list. Add one sentence: **the validator never skips a check because willikins minted the
token** — the fixed-order RFC 9068 checks run identically for an in-house and a foreign token, and
decision 7's fixture is what asserts it rather than prose.

**2. Principal identity — unchanged.** `oauth-<12 hex of sha256(iss ‖ 0x00 ‖ sub)>`,
`Principal { id, claims }`, the additive claim fields, the fixture discipline. The `iss` fold is
still right and now earns a second reason: it is what stops an adapter's subjects colliding with
willikins' own enrollment identifiers. One note for the fold: the in-house `sub` is a UUID willikins
generates at enrollment (AS §4.12) and would fit `PrincipalId`'s grammar directly — the derivation
stays anyway, because it must keep working for a foreign `sub` that does not.

**3. Scopes — changes one paragraph, keeps every conclusion.** The four scopes, the hierarchy, the
`scope`/`scp` readers (both still accepted, because an adapter's tokens land in the same validator),
the fail-closed empty set, the middleware placement and its five ordered steps, and both subject
allowlists all survive. What goes is the *stated premise* of "a scope is not authority": "decision 10
*requires* a provider with an open registration path ... so the set of clients that may ask for
`willikins:apply` is open by design." That is false now — willikins pre-registers its own clients.
**The conclusion survives on a different ground and the plan must say which**: a scope is what a
client asked for and a consent screen granted, and the human who clicks consent is not necessarily
the human a deployment authorises to apply. Second change: allowlist entries are now
willikins-generated enrollment identifiers, printed by the enrollment CLI (AS §4.12); the "log in
once and read your subject off the `WrongRole` page" bootstrap survives as the second path, not the
only one. The "one issuer, so bare `sub` values are unambiguous" argument is **stronger** now, not
weaker.

**4. The approvals page — changes shape. It stops being an OAuth client of anyone.** Not of an
external provider, and — this is the part that is easy to get wrong — **not of willikins' own
`/authorize` either**. Running a browser through a redirect loop back to the same process to obtain
a token the same process could mint is machinery with no security gain. The flow becomes:

> passkey ceremony → the issuer mints an access token for `<PUBLIC_URL>/mcp` with the enrolled
> subject and the granted scope → **`oauth::validate` accepts it on exactly the same path `/mcp`
> uses** → the session records the subject, issuer and scopes that validation returned.

That last hop is not ceremony; it is the seam (AS §7 layer 2). The plan's own words — "the same
function and the same configuration the `/mcp` middleware uses: one validation path, not two" —
survive verbatim and are now the seam's load-bearing rule.

*What this deletes from decision 4:* the 302 to a provider's authorization endpoint, `/approvals/callback`,
the `state` value and its store semantics, the `code_verifier`, the `iss`-parameter check on the
callback, the token exchange and its `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS` and byte cap, the
dropped-unread refresh and id tokens, `client_secret_basic`, `Credential::authorize_basic` and its
`clippy.toml`/`expose_secret_guard` exemption, the "willikins is a confidential client" paragraph
and its *Rejected: a public client with PKCE* counter-argument, and every mention of
`WILLIKINS_OAUTH_CLIENT_ID`/`_SECRET`/`_AUTHORIZE_URL`/`_TOKEN_URL`.

*What survives, re-founded:* the pending-login store **stays** — a passkey ceremony *requires*
server-side state and the crate refuses to serialise it, so AS §5.3's "delete the store" reading is
conditional on a form login and does not apply (AS §4.3). The `__Host-willikins-login` cookie stays,
re-founded as the **ceremony** binding rather than the OAuth-state binding, and it closes the same
two-approver attack in its WebAuthn form — which is precisely CVE-2026-69199's shape (AS §6.7). The
session cookie, both caps and their 503, logout, CSP (plus `script-src`), `X-Frame-Options`, the
`WrongRole` page, the nonce and the Origin check are all unchanged, and gain §1.7's five additions.

*And one thing decision 4 gains:* **the human session acquires a second consumer.** `/authorize`'s
consent page needs the same session — one login, two surfaces. That is what makes the `SameSite`
question live rather than a free flip to `Strict`; see the verify item in §6.

**5. Which listeners require OAuth — unchanged, and its stated cost improves.** "OAuth on every
bind, loopback included; there is no dual mode" stands, and so does the refusal of a `--dev-token`
flag. The one paragraph that changes is "The cost, stated plainly": after this revision a
hand-run `serve --http` on loopback can **enroll a passkey and log in against itself**, with no
external provider and no pasted token — so the developer loop is no longer "write a test, or point
at a real provider". State this as an improvement *conditional* on the unresolved question of
whether `__Host-`prefixed cookies are honoured on `http://localhost` (AS §9.2), not as a fact.

**6. `hash-token` and the two hash variables — unchanged.** Retire both loudly, refuse at startup on
every bind, and keep the split across two tasks so no commit on `main` holds a set-but-ignored
variable. The task that removes `basic_auth` is now the task that lands the passkey login, which
preserves the rule exactly. `HttpConfigError` keeps `RetiredVariable`, `SymmetricAlgorithm`,
`SessionOutlivesApprovalWindow`, `InsecureUrl` and `EmptyAllowlist`. `StartError` gains the new
**runtime** refusals for the same reason `JwksUnavailable` lived there — they happen after the
runtime exists and `HttpConfigError` is `Copy` and pure: no signing key and none can be generated or
written; an unreadable or unwritable credential store.

**7. The fake authorization server and pass 3 — changes: it shrinks and is repurposed.** It
**loses** `/authorize` and `/token` (faking willikins' own endpoints tests nothing — the real ones
are driven directly) and the wrong-`iss` authorization response with them. It **keeps** the fixed
test key, the JWKS with a `kid`, `mint(flaws)` with its whole flaw list, key rotation, the hanging
JWKS, the blocking `start()`, and the environment-block helper. What it **becomes** is a
**foreign-issuer fixture**: the thing that proves the resource-server half still validates, on the
identical path, a token it did not mint. That is seam rule 2 asserted by a test instead of by prose,
and it is the single cheapest insurance the milestone buys. Two flaws are added to `mint`: a token
signed with willikins' own key carrying a foreign `iss`, and a token carrying the configured `iss`
signed with a foreign key. The loopback-`http://` configuration rule of decision 7 survives for it.

The harness additionally gains **`SoftPasskey`** from `webauthn-authenticator-rs` 0.5.5 as a
dev-dependency — same repo, same MSRV 1.88, `new(falsify_uv)` and its own counter — which is what
makes the ceremony, the UV lie and the counter regression testable with no browser (AS §4.7).

Pass 3's attack table survives nearly whole (the callback rows go with the callback) and gains
rows: each code binding attacked one at a time (wrong client, wrong verifier, changed
`redirect_uri`, changed or introduced `resource`); one code redeemed twice concurrently; a phase-1
`/authorize` error attempted as a redirect; a loopback redirect on a different port (accepted) and
on a different host or path (refused); `/authorize` with no `code_challenge`;
`code_challenge_method=plain`; a JWKS served after a restart with no persisted key; a ceremony
finished against a different account; a recovery code replayed; a UV-lying authenticator; a counter
regression.

**8. The pass-2 exposure work — unchanged.** Tasks 9a to 9d are orthogonal to who issues the token.
One addition to 9c's sweep: DEBUG and TRACE must now also be swept over `/authorize`, `/token`, the
ceremony endpoints, the code store and anything that touches the signing key.

**9. The go-live sequence — changes in three steps and gains one.** Step 1 (the host) survives and
gets **heavier**: `willikins.bandeabonnot.com` now also freezes the **issuer identifier** and the
**WebAuthn RP ID**, and the RP ID has *no* migration path where the audience has one (AS §4.4) —
say it in the same breath. Step 4 changes from "everyone logs in once and reads their `sub` off the
`WrongRole` page" to "**run the enrollment CLI, enroll two credentials, save the recovery codes, and
paste the printed subject into both allowlists**", keeping the `WrongRole` page as the second path.
Step 5 drops `WILLIKINS_OAUTH_ISSUER` (derived), `_CLIENT_ID`, `_CLIENT_SECRET`, `_AUTHORIZE_URL`,
`_TOKEN_URL`, `_TOKEN_TIMEOUT_SECONDS` and — on decision 16's recommended branch — the four
`JWKS`-related variables, and gains the signing-key, credential-store, token-lifetime, idle-timeout
and client-table variables (appendix A). Step 6 drops the third Doppler secret and, on open decision
2's recommended default, **gains the signing key and its retiring sibling** in its place. The
registered `redirect_uri` is no longer willikins' own: it is each
pre-registered MCP client's. **New step: back up the credential store and the signing key** — the
first backup-and-restore concern the project has had (AS §4.12).

**10. The identity provider is the operator's decision — replaced entirely.** There is no provider.
What replaces it: willikins is its own authorization server; **no hosted or vendor identity product
is a dependency in any edition**; and **no authorization-server framework is adopted** —
`oxide-auth` is rejected on measurement (18,732 lines across three crates and three duplicate
majors to obtain ~1,500 wanted lines, with no RFC 8414, JWKS, RFC 7591, RFC 8707 or `at+jwt`
anywhere in it, and a `Grant` with no audience field at all; AS §2.2, §2.4), and `oauth-as` 0.9.4 is
the closest fit in the ecosystem and is **read as prior art, not depended on** — six weeks old, one
author, one star, no audit, 36,230 lines of security-critical source, with a dated re-evaluation
gated on 1.0, a second maintainer, or an audit (AS §2.5). The three must-haves die with the
decision; willikins satisfies all three by construction. The six-provider survey and the licence
table are **superseded as a recommendation** and retained only as prior-art reading. The one thing
that survives out of decision 10 is its best structural idea: the **single provider-agnostic
configuration block**, which becomes the seam's data layer (§5) rather than a provider selector.

**11. The revision anchor — unchanged, and one of its conditions is discharged.** 2026-07-28 stays
the anchor. Task 4's "first step: fetch the three sub-pages and the `ext-auth` extensions" is
**done**: AS §1 quotes `/authorization-server-discovery`, `/client-registration` and
`/security-considerations` verbatim, and §1.13 covers `ext-auth` (both extensions are OPTIONAL and
additive, so neither adds an obligation). The obligations those pages carry are folded into §1
above. The "mix-up attacks" worry the plan flagged is answered: emitting `iss` on the authorization
response (item 12) is the mix-up countermeasure.

**12. The resource identifier — survives, and gains two siblings.** `<PUBLIC_URL>/mcp`, derived and
not separately configurable, the insertion-built well-known URL, and
`WILLIKINS_OAUTH_PREVIOUS_AUDIENCES` as the documented migration all stand. Sibling one: the
**issuer identifier is now derived too** — `WILLIKINS_PUBLIC_URL` exactly, with no path, which also
makes the RFC 8414 well-known URL's insertion and append forms coincide and removes rauthy's
three-route trap by construction (AS §6.7). Sibling two: the **RP ID is derived from
`WILLIKINS_PUBLIC_URL`'s host and has no migration path at all**. Decision 12's own sentence "nothing
is forever" holds for the audience and **does not hold** for the RP ID; that asymmetry belongs in the
decision, not in a footnote.

**13. AS-metadata discovery — reverses, in exactly one direction.** willikins now **publishes** RFC
8414 authorization-server metadata: that is MCP's discovery MUST and it is unavoidable. It still
**fetches** none, because there is no external authorization server to fetch from — so the
decision's actual sentence survives and the decision gains its other half. The consequence the plan
draws from it changes: "a metadata document that lies about the issuer" now has no consumer at all
(the callback it used to attack is gone), and the attack that replaces it is against *willikins'
own* published document — that its `issuer` is byte-identical to the URL it was fetched from, which
is test-1-shaped and holds by construction under decision 12's sibling.

**14. The frozen `typ` set — unchanged; one clause dies.** `at+jwt` and `application/at+jwt`, no
knob, per RFC 9068 §4. What dies is the clause making "the provider emits `at+jwt`" a provider
precondition checked by test 18: **willikins emits it by construction** (item 26). Keep the set
two-valued rather than collapsing it to one, because an adapter's issuer may emit the long form.

**15. The algorithm allowlist — changes: it gains a default, and never-RSA becomes normative.**
`WILLIKINS_OAUTH_ALGORITHMS` becomes **optional with default `ES256`**, still asymmetric-only, still
refusing `HS*` with `SymmetricAlgorithm`, still **intersected per key** — and the per-key
intersection is now load-bearing rather than a migration nicety, because it is the only way exit (c)
is implementable. Two normative additions: **willikins never signs with the RSA family**, restated
here because RUSTSEC-2023-0071's old dismissal is void now that willikins holds a private key
(AS §3.4); and **RS256 stays permissible in the validator's allowlist**, where the public-key-only
reasoning genuinely does hold, which is what satisfies RFC 9068 §2.1's "MUST include RS256 among
their supported signature algorithms" without exposing the leak (AS §3.5, exit (c)). Test 3's
existing two-family case is what proves it.

**16. The JWKS fetch and cache — changes, and the recommended branch removes five variables.** The
fetch-and-cache described here has **no production consumer in 2c**: issuer and validator share a
process, and AS §7 rule 3 says the RS must *not* `ureq`-fetch its own `jwks_uri` over the loopback.
Two branches, and the plan must pick one rather than leave it implicit:

- **(a) Recommended — introduce a one-method `JwkSource` boundary now and ship only the in-process
  implementation.** Removes `WILLIKINS_OAUTH_JWKS_URI`, `WILLIKINS_JWKS_TIMEOUT_SECONDS`,
  `WILLIKINS_JWKS_REFRESH_SECONDS`, `WILLIKINS_JWKS_MIN_REFETCH_SECONDS`,
  `StartError::JwksUnavailable`, `ureq`'s move into `willikins-server`'s `[dependencies]`, and test
  5's hang and HTTP-rollover cases. **What it costs, stated rather than discovered:** the seam is
  then asserted by a second **in-process** issuer — decision 7's foreign-issuer fixture — rather than
  by an HTTP one, so the day an adapter lands, the fetch-and-cache is *new code written against a
  boundary that already exists*: an addition, not a rewrite. Test 5 shrinks to in-process key
  rollover and the two-key overlap window. The refresh-failure policy, the staleness bound and the
  unknown-`kid` refetch floor stay **written down in the decision**, marked as the adapter path's,
  so nobody re-derives them later.
- **(b) Keep the fetcher and point the resource server at its own `jwks_uri`.** Keeps every variable
  and test 5 whole. Costs a loopback HTTP round trip on the validation path, a cache of the server's
  own keys, and a rule AS §7 explicitly forbids.

**17. Clock skew and required claims — unchanged, with one asymmetry to state so nobody "fixes" it.**
Leeway 60 s explicit, `validate_nbf` on, `required_spec_claims = {exp, aud, iss, sub}`. The
**issuer** now emits all seven RFC 9068 REQUIRED claims (item 25) while the **validator's** required
set stays at four. That is deliberate: the validator must keep accepting an adapter's token, and
decision 2 already tolerates a missing `client_id`. Say so in the decision.

**18. The rmcp 3.4.0 bump — unchanged.** Still conditional, still nothing depends on it, and
recording why it was skipped is still the whole obligation.

---

## 4. Task shape

Milestone 2 style: one row per task, what it lands, what pins it, who writes it. Everything runs
sequentially on `main`, one cargo lane at a time. Every implementation task is test-first. The
biggest three rows are 9, 11 and 16; none is a month.

| # | Task | What pins it | Depends on | Delegate to |
| --- | --- | --- | --- | --- |
| 1 | **Dockerfile and dependency pins — first, because it is the one thing that can invalidate the shape.** Choose the `jsonwebtoken` 11 backend by building **both** in the image; add `libssl-dev` and `pkg-config` to the builder for `webauthn-rs-core`'s unconditional `openssl`/`openssl-sys` (AS §4.2); **verify `gcr.io/distroless/cc-debian12` actually ships the `libssl.so.3` `openssl-sys` 0.9.114 links against** (AS §9.3 — build-blocking, and the weakest citation in the note); rewrite the Dockerfile comment that claims no `openssl-sys` anywhere, which becomes **false**. Pins: `webauthn-rs` 0.5.5 (+ core/proto), `argon2` 0.6.0, `password-hash` **0.6.1** (0.6.0 is yanked), `rand` 0.8 as a direct dependency for the `rand_core` 0.6 bound (AS §3.6), `axum-extra` 0.12 `cookie-signed`, `webauthn-authenticator-rs` 0.5.5 dev-only | The image builds; the runtime container starts and serves `/healthz`; all four gates | 0 | sonnet, verified by opus |
| 1b | rmcp 3.4.0, conditional and skippable exactly as decision 18 says | Gates, or a recorded reason | 1 | sonnet |
| 2 | **First commit: freeze `pre-2c-every-event.jsonl`** from the current binary before anything changes, through an `#[ignore]`d generator in the shape of the two existing ones. Then the **foreign-issuer fixture** (decision 7 revised): fixed test key, JWKS with `kid`, `mint(flaws)` incl. the two new flaws, rotation, hang, blocking `start()`, environment-block helper, **no `/authorize` or `/token`**. Wire `SoftPasskey` | Both existing fixtures plus the new one replay through `Journal` and `replay` | 1 | sonnet, verified by opus |
| 3 | **The signing key and the issuer.** ES256 generation (`p256` + `rand` 0.8), PKCS#8 persistence at its chosen home, redacted `Debug`/no `Display`/no `Serialize`/zeroize, `kid` as the RFC 7638 thumbprint set on **both** header and JWK, `JwkSet` assembly, the two-key overlap mechanism, `mint(sub, scopes, aud) -> Jwt` emitting all seven RFC 9068 claims, and the `JwkSource` boundary with its in-process implementation | Trap 1 and trap 2 as acceptance tests (a JWK with no `kid` is a refusal; an Ed25519 v2 key is refused by name or Ed25519 is not offered); willikins validates its own token end to end; a restart with a persisted key keeps outstanding tokens alive, and without one refuses them | 2 | sonnet, verified by opus |
| 4 | **The resource-server middleware, against both issuers.** The plan's task 3 as written — `OAuthConfig`, `validate`, `require_token`, the header strip, the subject allowlist, the new `AuthFailedReason` variants, decisions 6/7/15 refusals — plus the assertion that the fixed-order path runs **identically** for a willikins-minted and a fixture-minted token. Removes `HttpConfig::build`'s agent-hash rules and retires `WILLIKINS_AGENT_TOKEN_HASHES` in the same series. Migrates the 11 `HttpConfig::build` test sites the plan counts | Tests 3, 4, 5 (reshaped per decision 16), 6 | 3 | sonnet, verified by opus |
| 5 | **Both metadata documents, the JWKS route and the challenge surface.** RFC 9728 at the insertion path; RFC 8414 with byte-identical `issuer`, `code_challenge_methods_supported`, explicit `grant_types_supported` and `token_endpoint_auth_methods_supported`, `authorization_response_iss_parameter_supported: true`, `jwks_uri`, zero-element arrays omitted; `/jwks.json`; all four routes outside the bearer middleware and rmcp's host check as `/healthz` is; rmcp's `allowed_origins` set unconditionally at 3.3.0 | Tests 1 and 2, plus a new metadata test per RFC 8414 field and a byte-identical-`issuer` test | 4 | sonnet, verified by opus |
| 6 | **Principal and journal** — the plan's task 5, unchanged | Test 8 | 4 | sonnet, verified by opus |
| 7 | **Scopes** — the plan's task 6, plus the consent screen presenting the same four scopes and the hierarchy | Test 7 | 5, 6 | sonnet, verified by opus |
| 8 | **Session core, as library code.** The signed `__Host-` cookie and jar, both bounded stores with 503, unconditional id rotation, absolute **and** idle timeouts bumped on reads, `no-store`, 128-bit ids, non-persistent cookie, credential-change revocation, `X-Frame-Options`, CSP with `script-src`, `Sec-Fetch-Site` + `Vary`, the trailing-`/` origin fix. **No `/approvals`-level assertion lands here**: `basic_auth` still gates that surface until task 9 removes it, and decision 6 forbids a commit in which both gates exist | Unit tests against a router the test builds: rotation destroys the previous id, the idle timer is bumped on a read, a full store answers 503, a credential change flushes the session, and `Set-Cookie` is asserted as a **header** and never as store contents — the `SignedCookieJar` drop is silent (AS §5.7) | 7 | sonnet, verified by opus |
| 9 | **Passkey enrollment and authentication, and the gate swap.** `webauthn-rs` wiring, the durable credential store, ceremony-to-account binding, UV Required, counter regression + `update_credential`, `exclude_credentials` + CredentialID uniqueness, the uniform username step, the inline script under a CSP nonce, logout, the `WrongRole` page, and the **mint → `oauth::validate` → session** hop. **Removes `basic_auth`, `TokenHash`, `matches_any` and the constant-time compare, and retires `WILLIKINS_APPROVER_TOKEN_HASH` in this series**, so no commit on `main` has an unauthenticated `/approvals` and none has two gates on it. Flips the three `adversarial_10b.rs` pins the plan names. **Every `/approvals`-level assertion is this task's**, because this is where the gate swaps | `SoftPasskey` drives registration and authentication; a UV-lying authenticator is refused; a counter regression is refused; a ceremony finished against another account is refused; **tests 9, 10 and 11** | 8 | **sonnet test-first, opus attacks** — CVE-2026-69199's class lives here |
| 10 | **Recovery codes, the enrollment CLI and the break-glass.** ≥128-bit codes, Argon2id, single-use, reissued; an Argon2 semaphore and per-account throttling so an unauthenticated endpoint cannot be a memory-exhaustion path (AS §4.9.1); the two-credential enrollment rule; the CLI subcommand minting a one-time enrollment URL to stdout and journalling it as loudly as an approval; the printed subject for both allowlists | A code is single-use and its replay refused; a used code is replaced; the semaphore bounds concurrent hashes; the break-glass journals an event | 9 | sonnet, verified by opus |
| 11 | **`/authorize` and `/token`.** The pre-registered client table; two-phase errors; exact redirect matching with the loopback port exception; mandatory `S256`; the consent screen with the redirect hostname; `iss` on every response including errors; `resource` → `aud`; the code record with all four bindings **stored**; the atomic get-and-remove claim; the independent expiry check and the ≤10-minute ceiling; rate limits and `Cache-Control: no-store`; `invalid_grant` indistinguishability; no refresh tokens | A **negative fixture per binding**, each naming its acceptance test and the exact error it must produce, per this repo's convention. Plus: a phase-1 error is never redirected; `plain` is refused; a missing challenge is refused; a loopback port change is accepted and a host or path change is not | 10 | **opus writes or co-writes.** The densest security surface in the milestone, and AS §6.6 is the reason |
| 12 | Delete `hash-token` from both binaries and its README section; update the variable reference | Test 12's last case | 11 | sonnet |
| 13 | IPv6-aware origin check (the plan's 9b) | Test 14 | 11 | sonnet |
| 14 | `WILLIKINS_LOG` as an `EnvFilter` directive, then the DEBUG/TRACE sweep — now also over `/authorize`, `/token`, the ceremony endpoints, the code store and the signing key (the plan's 9c) | Test 15 | 12 | sonnet, swept by opus |
| 15 | Move the concurrency permit into the blocking closure (the plan's 9d) | Test 16 | 12 | sonnet |
| 16 | **Adversarial pass 3** against the foreign-issuer fixture and against willikins' own endpoints: decision 7's revised table in full, recorded under `docs/research/`; freeze `post-2c-new-shapes.jsonl` | Every bypass becomes a fixture plus a test | 12, 13, 14, 15 | **opus** |
| 17 | Bound the header read (the plan's 9a), after pass 3, with its short pass-3 addendum | Test 13 | 16 | sonnet, verified by opus |
| 18 | **The conformance run that replaces test 18.** There is no provider token to obtain, so the live-token test loses its subject. What replaces it: a **stock MCP client** driven by hand against a locally-bound `serve --http` — discovery of both metadata documents, pre-registration, `/authorize` with PKCE, `/token`, `list_tools` — plus the refusals that need no minting (no token, truncated, tampered signature, a server started with a different `WILLIKINS_PUBLIC_URL`). The harness stays `#[ignore]`d behind the `live-tests` feature the plan already adds | The run, recorded; then repeated against the public domain at go-live | 17 | sonnet writes the harness, run with the operator |
| 19 | Go-live (decision 9 as revised): domain, measured `Host` and edge behaviour, allowed hosts, enrollment and both allowlists, `.railway/railway.ts` and `railway config plan`, the retired variables removed in the same change, Doppler's secrets, the credential-store and signing-key backup, `WILLIKINS_FAKE_CATALOG` removed, the stock-client run against the public domain, one real cycle; mark the plan **Completed** | Test 19 as revised | 18, operator | coordinator with the operator |

**Why task 1 moves to the front.** In the current plan the Dockerfile question is a dependency
footnote. It is now the item most likely to invalidate the milestone's shape: `webauthn-rs-core`
depends unconditionally on `openssl` and `openssl-sys` with no pure-Rust backend and no feature to
turn it off, the runtime image's `libssl` is the note's weakest citation, and both facts are
discovered in a deploy at 3am rather than in a plan (AS §4.2, §9.3).

**Why `/authorize` and `/token` is the opus task.** Kanidm spends roughly 3,600 implementation lines
against 5,160 test lines on this surface — a 1.4:1 ratio that is the most transferable number in the
corpus (AS §6.9). The one fetched post-mortem of a comparable Rust server is a rewrite that deleted
an equality check, kept the comment claiming it happened, and survived a professional audit running
in the same window (AS §6.6).

---

## 5. The pluggable-auth seam

Keep it to a seam and a todo. Do not design adapters now, and do not let the seam add abstraction
the in-house path does not need — which concretely means **no `dyn Trait` for authenticate-a-human
today, no adapter module, and no configuration switch**.

### 5.1 What the boundary is, in code

Three layers, nesting rather than conflicting (AS §7). Only one of them is new code, and it is the
smallest.

1. **A data seam that already exists and costs nothing.** The issuer identifier, the key-set
   location, and the RFC 9728 document's `authorization_servers[]` array — the single
   provider-agnostic configuration block the plan's decision 10 already isolates. Today the issuer is
   `WILLIKINS_PUBLIC_URL`; an adapter fills the same three fields with someone else's. RFC 9068 §4
   confirms this is the mechanism the specification intends: authorization servers "SHOULD use OAuth
   2.0 Authorization Server Metadata to advertise to resource servers their signing keys via
   `jwks_uri` and what `iss` claim value to expect via the `issuer` metadata value."
2. **A code seam that is one function wide.** `oauth::validate(token) -> Result<Claims, TokenRejection>`,
   returning `(sub, iss, client_id, scopes, expiry)`. `/mcp` and the approvals session both go
   through it and nothing downstream knows who issued. The MCP TypeScript SDK splits at precisely
   this line (`OAuthTokenVerifier`, "Slim implementation useful for token verification"), so this is
   a shipped design, not a hopeful one. Beside it, **`JwkSource`** — one method, "where does the key
   set come from" — with exactly two implementations it will ever have: in-process (shipped) and
   fetch-and-cache (the adapter's).
3. **The issuer's own input, as a function boundary and not a trait.** `mint(sub, scopes, aud) -> Jwt`.
   The in-house path needs that signature anyway; a `dyn` behind it is what an adapter adds, not
   what 2c ships.

### 5.2 What the in-house implementation provides

Passkey enrollment and authentication; the durable credential store; recovery codes and the
break-glass; the pre-registered client table with its redirect URIs; `/authorize`, the consent
screen and `/token`; the signing key and its whole lifecycle; `/jwks.json` and the RFC 8414
document; and the mint-then-validate hop that turns a finished ceremony into a session.

### 5.3 What an adapter for an external identity provider would have to provide

An issuer identifier and a key-set location willikins can validate against; an authorization
endpoint and a token endpoint its clients can reach; audience binding from RFC 8707's `resource`, or
a proprietary equivalent the operator configures; JWT access tokens carrying `typ: at+jwt` and a
`sub`; and a route by which a human ends up holding a willikins session. **Two adapter shapes exist
and prior art ships both** (AS §7, §1.13):

- **Keep willikins as the token issuer and add a grant that trusts an external assertion.** This is
  MCP's own standards-track enterprise story — `ext-auth`'s Enterprise-Managed Authorization splits
  exactly this way, with a "Resource Authorization Server" that issues the tokens and an "IdP
  Authorization Server" used for single sign-on. **Make this the todo's default.** It leaves the
  resource-server half untouched, and `draft-ietf-oauth-identity-assertion-authz-grant` is the
  document to read before freezing anything (AS §9.3 names it as unfetched).
- **Proxy the endpoints upstream**, the TS SDK's `ProxyOAuthServerProvider` shape. Recorded as the
  alternative; not the default.

Design neither now.

### 5.4 What must be true now, or the adapter is a rewrite

1. **Publish `jwks_uri` and serve a JWKS**, even though RFC 8414 makes it OPTIONAL for a co-hosted
   AS. A resource server that only ever knew how to read a key out of its own process **is** the
   rewrite (AS §7 rule 1). This is the single most important row in this document that a reader
   would otherwise cut as unnecessary.
2. **Never let the resource-server half skip validation because it minted the token.** The
   fixed-order RFC 9068 checks run identically for in-house and foreign tokens — and decision 7's
   foreign-issuer fixture is what *asserts* it, which is why that fixture is kept rather than deleted
   along with the external provider.
3. **Abstract the key-set *source*, not the fetch** (decision 16 branch (a)). The in-house issuer
   supplies the set in-process; an adapter supplies it through fetch-and-cache. The overlap window
   differs accordingly and the decision should say so: in-process it is exactly the maximum token
   lifetime; with an external IdP the JWKS cache TTL adds to it.
4. **Keep `iss` in the principal derivation** (decision 2 already does), so an adapter's subjects
   cannot collide with willikins' own enrollment identifiers.
5. **Keep both subject allowlists.** They are issuer-independent, and an external IdP's `sub` needs
   them exactly as much as an in-house one does — arguably more.
6. **Keep the validator's required claim set at four, not seven.** The issuer emits seven; a
   validator demanding seven would refuse an adapter's token (decision 17).
7. **Keep the accepted `typ` set two-valued.** An adapter's issuer may emit `application/at+jwt`.
8. **Decide `SameSite` once, in writing, as a seam constraint rather than a cookie detail.** An
   adapter's cross-site callback landing needs `Lax`; an in-house-only deployment might take
   `Strict`. Do not take `Strict` now and rediscover this later. (And see the verify item in §6:
   whether `Strict` withholds the session cookie on an externally-initiated top-level navigation to
   `/authorize` — which the consent page needs — is a browser behaviour nobody fetched, AS §9.2.)
9. **Budget one `skipLocalPkceValidation`-shaped flag** for the proxy shape. In a production SDK
   that flag is the *entire* delta, so it is cheap to design in and annoying to retrofit.

### 5.5 What does not port, written down rather than discovered

- **Passkeys.** Bound to the RP ID, which cannot change (AS §4.4). Switching to an external IdP
  orphans every credential and switching back re-enrols. Nothing mitigates this; it is the price of
  the in-house default.
- **Pre-registered client ids.** MCP: clients "**MUST** maintain separate registration state per
  authorization server and **MUST NOT** assume that credentials valid for one authorization server
  will be accepted by another" (AS §1.2). Every client re-registers the day the advertised AS
  changes. CIMD ids would port, because they are self-hosted URLs resolved on demand — which is the
  one real argument for CIMD that is not about convenience.
- **The credential store, the signing key and their backups.** The seam makes the *protocol*
  swappable. It does not make the *history* portable.

### 5.6 The todo

`todos/2026-09-16-pluggable-auth-adapter.md`, with the repo's YAML frontmatter
(`title`, `created`, `status`, `priority`, `area`, `related`), `status: banked`, pointing at §5 of
this document for the seam and at AS §1.13 and §7 for the two adapter shapes. Gated on someone
naming an identity provider they actually want to use — not on a date.

---

## 6. Open decisions for the operator

Four. Each has one recommended default and exactly one reason.

1. **The human login method. Recommended: passkeys, through `webauthn-rs` 0.5.5.**
   *The reason:* this deployment has no email and no SMS, so a password path has **no reset**, and
   losing the reset also removes the account-lockout mechanism's only escape hatch (AS §4.9).
   *What it carries:* OpenSSL enters the build, and the RP ID freezes the domain permanently.

2. **Where the signing key lives. Recommended: a Doppler-injected PKCS#8 PEM variable.**
   *The reason:* the design doc's own rule is that every secret lives in Doppler, and a
   Doppler-injected key makes rotation a variable change rather than an edit to a file on a volume
   that nothing backs up. (The counter-pull — first-boot self-provisioning onto the volume beside the
   journal — is equally consistent with the platform facts, AS §3.8, and it is what the credential
   store must do anyway.)
   *One shape constraint either way:* item 33's two-key overlap must be expressible, so the
   Doppler branch is **two variables — the current signing key and an optional retiring one** —
   not one. A single-key variable cannot express an overlap, and an overlap that cannot be expressed
   is a flag day in which every outstanding token is refused.

3. **Client registration. Recommended: pre-registration, one configuration entry per client.**
   *The reason:* for a handful of agent clients it costs zero new endpoints and zero attack surface,
   where CIMD is an outbound fetch of an attacker-chosen URL and DCR is an anonymous write endpoint
   with a garbage collector (AS §2.7).

4. **The access-token lifetime. Recommended: 3600 seconds.**
   *The reason:* with no refresh tokens the lifetime is *exactly* how often a human must re-consent
   at `/authorize`, and an hour is the shortest span that does not interrupt a working session. (It
   is also the key-rotation overlap window, AS §3.7, so the two numbers are one decision.)

**Not operator decisions, recorded so they do not become a fifth:** `SameSite=Strict` versus `Lax`
is a **browser-measurement verify item**, not a preference (AS §9.2) — measure whether `Strict`
withholds the session cookie on an externally-initiated top-level navigation to `/authorize` before
flipping anything, and default to `Lax` until it is measured. Whether `localhost` joins `127.0.0.1`
and `[::1]` in the port-relaxed redirect set is a fixture decision for task 11, with four fetched
sources showing three behaviours (AS §9.2). Whether `__Host-` cookies are honoured on
`http://localhost` decides only whether decision 5's improved developer loop is real (AS §9.2).

---

## 7. Risks

**The one the operator already knows: this is the security-critical part of the system and willikins
is writing it rather than buying it.** Stated in terms of what is actually exposed and to whom.

*What becomes anonymously reachable on a public domain with no edge rate limit:* `/authorize`,
`/token`, `/jwks.json`, both well-known documents, and the passkey ceremony endpoints. Before this
revision the only anonymous surfaces were the PRM document and a 401.

*What a bug in issuance buys an attacker:* a token minted with the operator's `sub` and
`willikins:apply` — which is the eight MCP tools against the **one** GitHub organization and **one**
Doppler workplace the deployment's two credentials cover, until milestone 3's credential routing
lands. **Bounded by three things that do not depend on the issuing code being correct:** an unlisted
`sub` is refused in the middleware before any tool runs (decision 3); anything `apply` touches that
is not auto-approved still needs a human at the approvals page; and no caller token has ever reached
an upstream API, because the `Credential` methods in `willikins-providers-http` are the only sites
that put a credential on an outgoing wire. That third boundary is the resource-server half, which
this revision leaves untouched.

*What makes it acceptable:* the profile willikins is building is kanidm's required profile — one
grant, mandatory PKCE `S256`, one signing algorithm (AS §6.9) — **minus its client-authentication
requirement**, because MCP clients here are public clients with loopback redirects and draft-16
accepts exactly that given mandatory PKCE (AS §1.5). At the size AS §10.1 measures: roughly 4,000–6,000 implementation lines plus about
1.4× that in tests, against a corpus where rauthy's protocol machinery is ~7,300 lines and
Cloudflare's *complete* MCP authorization server is 5,967 with the login declared out of scope. The
alternatives were a 36,230-line six-week-old unaudited crate by one author, or a vendor identity
product every self-hoster of willikins would inherit. And the one independent audit of a comparable
Rust identity provider found **1 Elevated, 3 Low, none in the OAuth grant logic** — two of the four
being Rust-shaped rather than protocol-shaped (a timing oracle, a reachable `unwrap()` on a
cancelled request), which is a useful calibration for where to point pass 3 (AS §6.8).

*What would make it unacceptable:* shipping without adversarial pass 3, or without a negative
fixture per code binding held **in the store**. AS §6.6 is the whole argument — the binding that was
lost was lost by a rewrite that kept the comment claiming it happened, and the audit running in the
same window missed it.

**The exposure is not symmetric with what 2c originally proposed.** Before this revision, a bug in
validation refuses a good token — annoying, safe. After it, a bug in issuance mints a bad one. Pass
3 must attack the issuing side at least as hard as the validating side, and the task order puts it
after everything except the transport rewrite for that reason.

Then, in descending order of how likely each is to surprise someone:

- **OpenSSL enters the build.** `webauthn-rs-core` depends unconditionally on `openssl` and
  `openssl-sys`; the Dockerfile's own "no `aws-lc-sys`, `openssl-sys` or `cmake` anywhere in the
  tree" comment becomes **false** and must be rewritten; the builder needs `libssl-dev` and
  `pkg-config`; and whether the distroless runtime image ships the matching `libssl.so.3` is the
  note's weakest citation and is **build-blocking** (AS §4.2, §9.3).
- **`rsa` is still compiled in** under `jsonwebtoken`'s `rust_crypto` feature bundle, and
  RUSTSEC-2023-0071 has `patched = []` deliberately. Its dismissal now survives **only** on the
  normative constraint that willikins never signs with the RSA family. A future `cargo-audit` or
  `cargo-deny` gate needs a documented ignore with a **rewritten** justification — the old one is
  void (AS §3.4).
- **The RP ID can never change.** Harder than the audience freeze and with no migration path. A
  domain move orphans every credential and every human re-enrols (AS §4.4).
- **First durable identity state.** The credential store is a backup-and-restore concern the project
  has never had; the journal is append-only and is not a credential store (AS §4.12). Railway
  further constrains it: one volume per service, no replicas with volumes, downtime on redeploy.
- **Key loss is a total outage of every outstanding token**, and if the key is not persisted that
  happens on every restart (AS §3.8).
- **Milestone size.** The task count roughly doubles and the human half is where the cost is: rauthy
  is 84,583 lines, of which ~7,300 is the protocol and ~77,000 is users, passwords, passkeys, MFA
  and the rest (AS §6.2, §10.2). This is a milestone and it is not a week.
- **`webauthn-rs`' MSRV is exactly 1.88**, the workspace floor, with zero headroom — as is
  `jsonwebtoken` 11's. Any bump in either raises the declared floor.
- **No external audit and effectively one author.** Two Rust authorization servers converging
  independently on constant-time secret comparison (one of them post-audit) is the one thing in the
  corpus that is settled rather than rediscovered (AS §6.4) — everything else is judgement.
- **Browser dependency.** If any browser in the operator's environment refuses
  `navigator.credentials` on the deployment's domain, the recovery-code path is the only mitigation
  the plan's shape offers — which is why §2.2 refuses to defer it (AS §9.4).
- **Duplicate dependency majors and build time**, unchanged in kind from the plan's own entry and
  larger in degree: `argon2`, `password-hash`, `rand` 0.8, `webauthn-rs` and its two siblings join
  what `jsonwebtoken`'s backend already pulls.

---

## Appendix A: environment-variable delta

The plan's table says "seventeen added, ten required in http mode and seven optional". The
coordinator restates the count; this is the delta it is computed from.

**Removed** (the external provider goes, and with it):
`WILLIKINS_OAUTH_ISSUER` (now **derived** from `WILLIKINS_PUBLIC_URL`, decision 12's sibling),
`WILLIKINS_OAUTH_CLIENT_ID`, `WILLIKINS_OAUTH_CLIENT_SECRET`, `WILLIKINS_OAUTH_AUTHORIZE_URL`,
`WILLIKINS_OAUTH_TOKEN_URL`, `WILLIKINS_OAUTH_TOKEN_TIMEOUT_SECONDS`.
**On decision 16 branch (a), additionally:** `WILLIKINS_OAUTH_JWKS_URI`,
`WILLIKINS_JWKS_TIMEOUT_SECONDS`, `WILLIKINS_JWKS_REFRESH_SECONDS`,
`WILLIKINS_JWKS_MIN_REFETCH_SECONDS`.

**Changed:** `WILLIKINS_OAUTH_ALGORITHMS` becomes **optional**, default `ES256` (decision 15).

**Added:** the signing key's home (one variable, shape per open decision 2); the credential-store
path; `WILLIKINS_ACCESS_TOKEN_TTL_SECONDS` (default per open decision 4, and also the key-rotation
overlap window); `WILLIKINS_SESSION_IDLE_TIMEOUT_SECONDS` (new, beside the existing absolute TTL);
the pre-registered client table (one variable holding the entries, or a path to a file holding
them); `WILLIKINS_AUTHORIZATION_CODE_TTL_SECONDS` (default short, ceiling 600 per draft-16's
RECOMMENDED maximum).

**Unchanged:** `WILLIKINS_PUBLIC_URL`, `WILLIKINS_AGENT_SUBJECTS`, `WILLIKINS_APPROVER_SUBJECTS`,
`WILLIKINS_OAUTH_LEEWAY_SECONDS`, `WILLIKINS_SESSION_TTL_SECONDS`,
`WILLIKINS_OAUTH_PREVIOUS_AUDIENCES`, `WILLIKINS_LOG`, and every variable the plan lists as
untouched.

**Still retired and still refused at startup:** `WILLIKINS_AGENT_TOKEN_HASHES`,
`WILLIKINS_APPROVER_TOKEN_HASH`.

## Appendix B: trust boundaries 1 and 5 are rewrites, not amendments

**Trust boundary 1 is overturned clause by clause** (AS §10.4). Today it reads: "willikins validates
tokens and issues none. ... It holds no signing key, mints no access token, runs no `/authorize`,
`/token` or `/register` endpoint, and stores no refresh token. Its only cryptographic input is a
JWKS of public keys fetched from the configured authorization server." Four of those six clauses
become false. **Split it in two rather than amending it:**

- **1a, the resource-server boundary — survives, and is the reason the seam falls where it does.**
  At `/mcp` willikins is an OAuth 2.1 resource server. It validates every token on the same
  fixed-order path whoever minted it, **including its own**, and never skips a check because it
  recognises the issuer. The inbound token is a credential for this server and nothing else: never
  forwarded, never journalled, never in a log line or an error body, stripped from the headers before
  any handler sees it.
- **1b, the authorization-server boundary — new.** willikins holds exactly one live signing key pair,
  two only during a rotation overlap. It mints RFC 9068 access tokens for **one** audience. It issues
  **no** refresh token and runs **no** `/register`. It publishes only the public half, at `jwks_uri`.
  The private key never leaves the process, never reaches the journal, a log line or an error body,
  and has a redacted `Debug` with no `Display` and no `Serialize` — the `Credential`/`Value`
  discipline the codebase already enforces, applied to a signing key, which is what rauthy does for
  the same reason (AS §6.4).

**Trust boundary 5 survives and gets stronger.** "No static shared secret authenticates a caller to
willikins" stands. The **entire paragraph** carving out `WILLIKINS_OAUTH_CLIENT_SECRET` as "one
static secret remains in the process and is not a counter-example" **loses its subject and goes** —
there is no client secret any more, because the approvals login is no longer an OAuth client
(decision 4). What must *not* be written in its place is a claim that willikins now holds no secret:
it holds a **signing** secret, which authenticates nothing to anyone and is not shared, and whose
compromise is strictly worse than a shared secret's because it mints tokens rather than presenting
one. That statement belongs in boundary **1b**, not in boundary 5.

**Trust boundary 2 loses one sentence.** "the approvals login's client secret joins them as a
`Credential` (decision 4), so the set grows by one named method and not by one new site" goes with
`Credential::authorize_basic`. The invariant reverts to its milestone 2 form: the `Credential`
methods in `willikins-providers-http` are the only sites that put a credential on an outgoing wire,
and the set does not grow at all in 2c.

**Trust boundary 3 is unchanged in substance** — two surfaces, two credential kinds, two allowlists,
`/approvals` never inspecting `Authorization` — with one wording fix: "a session approves only if its
`sub` is listed **and** its token carried `willikins:approve`" still holds, and the token in question
is now one willikins minted for itself at the end of a passkey ceremony.
