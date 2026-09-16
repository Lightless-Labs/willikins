# Milestone 2c: willikins as its own authorization server

**Created:** 2026-09-16
**Plan:** `docs/plans/2026-09-16-milestone-2c-authorization.md` — its resource-server half
survives unchanged; what changes is who issues the tokens it validates.
**Supersedes, in part:** `docs/research/2026-09-16-m2c-authorization.md` section 6 ("Identity
providers, one row per provider") and decision 10's provider survey. Sections 1-5 and 7 of that
note stand, with two corrections recorded below.
**Feeds:** `docs/plans/2026-09-11-willikins-design.md` (the trust model, and the rule that policy
lives in the workflow and never in the tool).

## The decision this note is written under

On 2026-09-16 the operator rejected the drafted recommendation — willikins as an OAuth 2.1
resource server delegating to a self-hosted Logto — outright:

> "So logto: no, no, and no. I won't have the entire project's authn / authz depend on the 'open
> source' edition of a SaaS. I've tried Auth0, Firebase, Clerk, and AWS Cognito in the past. All
> of them made me regret it. There is *no way* I'll shove such a tool down the throat of anyone
> who wants to use Willikins. And I'm reasonably certain there are more than a handful state of
> the art Rust crates regarding Authn, Authz, OAuth and sign in / sign up."

willikins is software other people self-host. A dependency on a vendor identity product is one
those people would inherit. So willikins issues its own tokens and authenticates its own humans,
built from Rust crates. **No hosted or vendor identity product, in any edition, is proposed as a
dependency anywhere in this note.** Reading one for its source or documentation as prior art is
encouraged and section 6 does exactly that; depending on one is not.

The same day the operator set the direction this research designs toward:

> "we *can* think about later having auth be a pluggable system, with a default in-house
> implementation and the ability to swap it (using adapters?) with other systems such as
> self-hosted or cloud Logto, Clerk, and co. But that's something I'd bank as a later improvement
> in a todo."

So the in-house implementation is the default and the only thing built now, and the work must
leave a seam clean enough that an adapter is a later addition rather than a rewrite. Section 7
states that seam once.

This is not a note that says the work is small. It is not. Section 10 gives the numbers.

## Fetch discipline

Every fact below comes from a primary source fetched verbatim on **2026-09-16**, with its URL and
the quote: a specification markdown twin, an RFC's plain text, a crate's own source at a pinned
version or from the published `.crate` tarball (which is what cargo actually builds), crates.io
version metadata, or a repository file at a resolved commit sha. The date is repeated in each
source parenthetical so a quote stays dated when it is lifted out of this file.

Anything that could not be fetched verbatim, every unpinned source, and every point where two
readers disagreed is in section 9, "Verify with a browser before relying on them". Nothing in
section 9 may enter frozen code.

Constraints every recommendation is measured against, restated so no section has to re-derive
them:

- Declared workspace MSRV **1.88** (`Cargo.toml` line 21); installed toolchain 1.97. A crate with
  an MSRV between 1.89 and 1.97 builds here and still raises the declared floor.
- The only HTTP client in the tree is **`ureq` 3.4.2**, synchronous, over rustls with
  webpki-roots. There is no async HTTP client. The server is **axum 0.8.9 on tokio**.
- The Dockerfile's builder stage installs `gcc` and `libc6-dev` with `--no-install-recommends`
  and nothing else; the tree has no `aws-lc-sys`, `openssl-sys` or `cmake` dependency. A crate
  that needs one is a **finding**, recorded as such, not an automatic disqualification.
- **No cargo command was run** — another lane may be building and this host cannot take two. So
  no weight, build-time or compile-success claim below is measured. Every "can the Dockerfile
  build it" judgement is dependency-graph inference from fetched manifests.

## Two corrections to `2026-09-16-m2c-authorization.md`

Both are load-bearing, both are in a file this task may not edit, and both are repeated in the
structured result for the coordinator.

- **Section 6 / plan line 1117, "`tower-sessions` needs a session store this server does not
  have", is false.** `memory-store` is one of the crate's *default* features. The conclusion
  (decline the crate) is right; the stated reason is not, and section 5.1 replaces it with four
  fetched ones.
- **Section 4.3's dismissal of RUSTSEC-2023-0071 does not survive this decision.** It reads "a
  resource server only verifies with public keys from a JWKS, so it does not apply to this use."
  An issuer signs, and the advisory is a private-key timing leak observable over the network.
  Section 3.4 restates the dismissal on a new and narrower ground.

Sections:

1. What the MCP profile requires of an authorization server
2. Rust crates for the authorization-server half
3. Issuing the token: signing, JWKS, and key lifecycle
4. Authenticating the human
5. Sessions, cookies and CSRF, now that willikins runs the login
6. Prior art, and what bit the people who shipped it
7. The seam, stated once
8. Pin table
9. Verify with a browser before relying on them
10. What this settles


## 1. What the MCP profile requires of an authorization server

The existing note covered the resource-server half. This section covers only the half it
deliberately skipped. Quotes are matched as exact substrings against `modelcontextprotocol.io`'s
raw `.md` twins and against RFC plain text, not against a summarizing fetcher's rendering.

### 1.1 The whole authorization-server burden, in one sentence

MCP defines **no authorization-server endpoints of its own**. It delegates the entire AS side to
OAuth 2.1 by section number and grades client registration.
(source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization.md,
2026-09-16)

- "1. Authorization servers **MUST** implement OAuth 2.1 with appropriate security\n   measures for both confidential and public clients.\n\n2. Authorization servers and MCP clients **SHOULD** support [OAuth Client ID Metadata Documents]\n...\n3. Authorization servers and MCP clients **MAY** support the OAuth 2.0 Dynamic Client Registration\n   Protocol ([RFC7591]). Note that\n   [Dynamic Client Registration] \n   is deprecated and retained for backwards compatibility"

Co-hosting the authorization server with the resource server is explicitly allowed, and gets no
carve-out of any kind: a co-hosted AS still publishes its own metadata at its own well-known
path, and the RS still validates the token as if a stranger had minted it. The sentence is
byte-identical at 2025-11-25 and 2026-07-28.
(source: same page, 2026-09-16)

- "The implementation details of the authorization server are beyond the scope of this specification. It may be hosted with the\nresource server or a separate entity."

### 1.2 Discovery: one document, one well-known path, one byte-exact string

At least one of RFC 8414 or OpenID Connect Discovery is required. Publishing only RFC 8414
metadata is conformant and is the cheaper branch — no OIDC provider metadata, no ID tokens.
(source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization.md,
2026-09-16)

- "5. MCP authorization servers **MUST** provide at least one of the following discovery mechanisms:\n\n   * OAuth 2.0 Authorization Server Metadata ([RFC8414])\n   * [OpenID Connect Discovery 1.0]"

MCP uses the default suffix and defines none of its own; the `issuer` inside the served document
**MUST** be byte-identical to the issuer identifier the client used to build the URL, or the
client MUST NOT use the document. That makes the issuer string a frozen configuration value,
exactly as the RS-side `resource` identifier already is.
(source:
https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery.md,
2026-09-16)

- "MCP uses the default `oauth-authorization-server` well-known URI\nsuffix defined in\n[RFC 8414 Section 3.1]\nfor authorization server metadata discovery. MCP does not define\nan application-specific well-known URI suffix."
- "the `issuer` value in the document **MUST** be identical to the issuer identifier used to construct the well-known URL. If they differ, the client **MUST NOT** use the metadata."

The protected-resource document MUST name at least one authorization server, and client
identifiers are **per authorization server** — which is the fact that decides how much an AS swap
costs later (section 7).
(source: same page, 2026-09-16)

- "The Protected Resource Metadata document returned by the MCP server **MUST** include\nthe `authorization_servers` field containing at least one authorization server."
- "Clients **MUST** maintain\nseparate registration state (client credentials, tokens) per authorization server and\n**MUST NOT** assume that credentials valid for one authorization server will be accepted by\nanother."

### 1.3 The metadata document itself, and two stale defaults that would misdescribe the server

Four fields are REQUIRED in practice for the code flow; `jwks_uri`, `registration_endpoint` and
`revocation_endpoint` are all OPTIONAL.
(source: https://www.rfc-editor.org/rfc/rfc8414.txt, 2026-09-16)

- "   issuer\n      REQUIRED.  The authorization server's issuer identifier, which is\n      a URL that uses the \"https\" scheme and has no query or fragment\n      components.\n...\n   token_endpoint\n      URL of the authorization server's token endpoint [RFC6749].  This\n      is REQUIRED unless only the implicit grant type is supported.\n\n   jwks_uri\n      OPTIONAL.  URL of the authorization server's JWK Set [JWK]\n      document.\n...\n   response_types_supported\n      REQUIRED.  JSON array containing a list of the OAuth 2.0\n      \"response_type\" values that this authorization server supports."
- "   token_endpoint_auth_methods_supported\n      OPTIONAL.  JSON array containing a list of client authentication\n      methods supported by this token endpoint. ... If omitted, the\n      default is \"client_secret_basic\""

Two omissions therefore lie about an OAuth 2.1 server and both must be emitted explicitly:
omitting `grant_types_supported` defaults to `["authorization_code", "implicit"]`, a grant OAuth
2.1 deletes; omitting `token_endpoint_auth_methods_supported` defaults to `client_secret_basic`,
which would make every public client (`token_endpoint_auth_method: "none"`) appear unsupported.

Serving mechanics, which bite a `#[derive(Serialize)]` struct directly: the document must answer a
GET, respond 200 with `application/json`, and **omit** any array claim with zero elements rather
than serialising `[]` — i.e. `skip_serializing_if = "Vec::is_empty"` on every array field.
(source: https://www.rfc-editor.org/rfc/rfc8414.txt, 2026-09-16)

- "3.1.  Authorization Server Metadata Request\n\n   An authorization server metadata document MUST be queried using an\n   HTTP \"GET\" request at the previously specified path."
- "A successful response MUST use the 200 OK HTTP\n   status code and return a JSON object using the \"application/json\"\n   content type ...\n   Claims that return multiple values are represented as JSON arrays.\n   Claims with zero elements MUST be omitted from the response."

### 1.4 The one field whose absence silently breaks every conformant client

`code_challenge_methods_supported` must be present. MCP turns its absence into a client-side
abort: there is no runtime PKCE discovery, so metadata is the only signal.
(source:
https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations.md,
2026-09-16)

- "**OAuth 2.0 Authorization Server Metadata**: If `code_challenge_methods_supported` is absent, the authorization server does not support PKCE and MCP clients **MUST** refuse to proceed."

### 1.5 PKCE: what OAuth 2.1 draft-16 actually says, fetched in this session

Two readers quoted different drafts and reached different conclusions about `plain`. Fetched
directly from the current revision (draft-16, September 2026, expires 7 March 2027), the question
is settled and `plain` does not exist: the method parameter is REQUIRED and its value is `S256`
or a future extension. There is no default to fall back to.
(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, §4.1.1, 2026-09-16)

- "   \"code_challenge\":  REQUIRED unless the specific requirements of\n      Section 7.5.1 are met.  Code challenge derived from the code\n      verifier.\n\n   \"code_challenge_method\":  REQUIRED, the value S256 or a value defined\n      by a future extension"
- "   An authorization server MUST reject requests without a code_challenge\n   from public clients, and MUST reject such requests from other clients\n   unless there is reasonable assurance that the client mitigates\n   authorization code injection in other ways."
- "   If the server does not support the requested code_challenge_method\n   transformation, the authorization endpoint MUST return the\n   authorization error response with error value set to invalid_request."

The carve-out §7.5.1 names — the only case where an AS may legally accept a request with no
challenge — **cannot apply to willikins**: it requires a confidential client using the OpenID
Connect nonce mechanism, and MCP desktop clients here are public clients with loopback redirects.
(source: same draft, §7.5.1.1, 2026-09-16)

- "   To prevent injection of authorization codes into the client, using\n   code_challenge and code_verifier is REQUIRED for clients, and\n   authorization servers MUST enforce their use, unless both of the\n   following criteria are met:\n\n   *  The client is a confidential client.\n\n   *  In the specific deployment and the specific request, there is\n      reasonable assurance by the authorization server that the client\n      implements the OpenID Connect nonce mechanism properly."

### 1.6 The authorization response: `iss` is REQUIRED, not SHOULD, and the code has three bindings

MCP grades `iss` emission as SHOULD and says a future revision is expected to raise it to MUST.
OAuth 2.1 draft-16 **already has**, and RFC 9207 makes it a MUST for any server supporting it.
Cheap now, expensive to retrofit.
(source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization.md,
2026-09-16)

- "MCP authorization servers **SHOULD** include the `iss` parameter in authorization responses, including error responses ... Authorization servers that include the `iss` parameter **MUST** advertise this by setting `authorization_response_iss_parameter_supported` to `true` in their metadata"

(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, §4.1.2, 2026-09-16)

- "   \"iss\":  REQUIRED.  The issuer identifier of the authorization server\n      which the client can use to prevent mix-up attacks, if the client\n      interacts with more than one authorization server."
- "      authorization code MUST expire shortly after it is issued to mitigate the risk\n      of leaks.  A maximum authorization code lifetime of 10 minutes is\n      RECOMMENDED.  The authorization code is bound to the client\n      identifier, code challenge and redirect URI."

That last sentence is the normative source for all three code bindings section 6 shows a shipped
implementation missing one of.

RFC 9207's own wire shape and metadata rule, fetched in this session because no reader had it:
(source: https://www.rfc-editor.org/rfc/rfc9207.txt, §2 and §2.3, 2026-09-16)

- "   In authorization responses to the client, including error responses,\n   an authorization server supporting this specification MUST indicate\n   its identity by including the iss parameter in the response.\n\n   The iss parameter value is the issuer identifier of the authorization\n   server that created the authorization response, as defined in\n   [RFC8414].  Its value MUST be a URL that uses the \"https\" scheme\n   without any query or fragment components."
- "   *  The issuer identifier included in the server's metadata value\n      issuer MUST be identical to the iss parameter's value.\n\n   *  The server MUST indicate its support for the iss parameter by\n      setting the metadata parameter\n      authorization_response_iss_parameter_supported ... to true."

It is one query parameter and one metadata boolean.

### 1.7 Redirect URIs: exact match, with one exception a naive matcher gets wrong

MCP states a flat MUST for exact matching. OAuth 2.1 gives the precise rule and the exception,
and turns the exception itself into an AS MUST, because a desktop MCP client binds an ephemeral
port at request time.
(source:
https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations.md,
2026-09-16)

- "Authorization servers **MUST** validate exact redirect URIs against pre-registered values to prevent redirection attacks."
- "1. All authorization server endpoints **MUST** be served over HTTPS.\n2. All redirect URIs **MUST** be either `localhost` or use HTTPS."

(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-13.txt, §2.3.1 and §8.4.2,
2026-09-16)

- "   Authorization servers MUST require clients to register their complete\n   redirect URI (including the path component).  Authorization servers\n   MUST reject authorization requests that specify a redirect URI that\n   doesn't exactly match one that was registered, with an exception for\n   loopback redirects, where an exact match is required except for the\n   port URI component"
- "   While redirect URIs using the name localhost (i.e.,\n   http://localhost:{port}/{path}) function similarly to loopback IP\n   redirects, the use of localhost is NOT RECOMMENDED."
- "   The authorization server MUST allow any port to be specified at the\n   time of the request for loopback IP redirect URIs, to accommodate\n   clients that obtain an available ephemeral port from the operating\n   system at the time of the request."

A literal string-equality matcher is non-conformant and will break real clients; a matcher that
ignores the port everywhere is a vulnerability. Whether `localhost` joins `127.0.0.1` and `[::1]`
in the port-relaxed set is genuinely unsettled across the fetched sources — section 9.2.

The token request's `redirect_uri` must additionally be **identical to the one used at the
authorization request**, which is a separate obligation from re-checking the registered set:
(source: https://www.rfc-editor.org/rfc/rfc6749.txt, §4.1.3, 2026-09-16)

- "   redirect_uri\n         REQUIRED, if the \"redirect_uri\" parameter was included in the\n         authorization request as described in Section 4.1.1, and their\n         values MUST be identical."

### 1.8 `resource`: every MUST is on the client; the AS obligation is RFC 8707's

MCP puts no AS-side MUST on `resource` at all.
(source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization.md,
2026-09-16)

- "MCP clients **MUST** implement Resource Indicators for OAuth 2.0 as defined in [RFC 8707]\n...\n1. **MUST** be included in both authorization requests and token requests.\n2. **MUST** identify the MCP server that the client intends to use the token with.\n...\nMCP clients **MUST** send this parameter regardless of whether authorization servers support it."

(source: https://www.rfc-editor.org/rfc/rfc8707.txt, 2026-09-16)

- "   invalid_target\n      The requested resource is invalid, missing, unknown, or malformed.\n\n   The authorization server SHOULD audience-restrict issued access\n   tokens to the resource(s) indicated by the \"resource\" parameter.\n   Audience restrictions can be communicated in JSON Web Tokens\n   [RFC7519] with the \"aud\" claim"
- "   If the client omits the \"resource\" parameter when requesting\n   authorization, the authorization server MAY process the request with\n   no specific resource or by using a predefined default resource value.\n   Alternatively, the authorization server MAY require clients to\n   specify the resource(s) they intend to access and MAY fail requests\n   that omit the parameter with an \"invalid_target\" error."

This is the hinge between the two halves in one process: the `aud` the AS writes here is the
`aud` the resource server already validates under RFC 9068. The practical reading for a
single-resource deployment is to accept `resource` on both endpoints, reject anything that is not
the one canonical URI with `invalid_target`, and mint `aud` from it.

### 1.9 Client registration: no mechanism is a server MUST

Three mechanisms exist, a client must obtain a client id through one of them, and the server-side
grading is CIMD **SHOULD**, DCR **MAY and deprecated**, pre-registration no MUST at all. Client
preference order puts pre-registration first.
(source:
https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration.md,
2026-09-16)

- "Clients supporting all options **SHOULD** use the following priority order:\n\n1. Use pre-registered client information for the server if the client has it available\n2. Use Client ID Metadata Documents if the Authorization Server indicates that it supports them (via `client_id_metadata_document_supported` in OAuth Authorization Server Metadata)\n3. Use Dynamic Client Registration as a fallback if the Authorization Server supports it (via `registration_endpoint` ...)\n4. Prompt the user to enter the client information if no other option is available"

CIMD is the mechanism that needs **no registration storage at all** — the AS keeps nothing and
resolves the client on demand — which is why it is attractive. Its obligations:
(source: same page, 2026-09-16)

- "**For Authorization Servers:**\n\n* **SHOULD** fetch metadata documents when encountering URL-formatted client\\_ids\n* **MUST** validate that the fetched document's `client_id` matches the URL exactly\n* **SHOULD** cache metadata respecting HTTP cache headers\n* **MUST** validate redirect URIs presented in an authorization request against those in the metadata document\n* **MUST** validate the document structure is valid JSON and contains required fields"

The client-identifier URL grammar is a checklist enforceable before any network call:
(source: https://www.ietf.org/archive/id/draft-ietf-oauth-client-id-metadata-document-00.txt,
2026-09-16)

- "   Client identifier URLs MUST have an \"https\"\n   scheme, MUST contain a path component, MUST NOT contain single-dot or\n   double-dot path segments, MUST NOT contain a fragment component and\n   MUST NOT contain a username or password Client identifier URLs SHOULD\n   NOT include a query string component, and MAY contain a port."
- "   As there is no way to establish a shared secret to be used with\n   client metadata documents, the following restrictions apply ...\n\n   *  the token_endpoint_auth_method property MUST NOT include\n      client_secret_post, client_secret_basic, client_secret_jwt, or any\n      other method based around a shared symmetric secret.\n\n   *  the client_secret and client_secret_expires_at properties MUST NOT\n      be used"

Advertising support is itself a MUST for any AS that implements it and publishes RFC 8414
metadata — forget the boolean and conformant clients skip CIMD entirely:
(source: same draft, 2026-09-16)

- "   Authorization servers that publish Authorization Server Metadata\n   [RFC8414] MUST include the following property to signal support for\n   client metadata documents as described in this specification.\n\n   client_id_metadata_document_supported: ...\n\n   This enables clients to avoid sending the user to a dead end"

### 1.10 The cost CIMD carries into a codebase whose invariant is "no raw URLs"

A CIMD-supporting AS becomes an HTTP client fetching attacker-chosen URLs. The draft's §6 is an
SSRF and DoS checklist.
(source: same draft, §6.5 and §6.6, 2026-09-16)

- "   Authorization servers SHOULD avoid fetching\n   any URLs using private or loopback addresses and consider network\n   policies or other measures to prevent making requests to these\n   addresses.  Authorization servers SHOULD also be aware of the\n   possibility that URLs might be non-http-based URI schemes"
- "   Authorization servers SHOULD limit the response size when fetching\n   the client metadata document ... The recommended maximum response size for\n   client metadata documents is 5 kilobytes."

Two consequences the plan must state rather than discover. First, the design doc's invariant "no
tool may take a raw URL, shell command, or arbitrary API path as input" governs **tool ports**;
the authorization server's own protocol machinery is not a tool port, and the plan must say so
explicitly or the invariant reads as violated. The fetch belongs behind a narrowly typed CIMD
fetcher with its own hard limits — private/loopback refused, non-HTTPS refused, body capped at
5 KB — not behind a general HTTP client. Second, the only HTTP client in the tree is blocking
`ureq` while the server is axum on tokio, so a CIMD fetch inside the authorize handler needs
`spawn_blocking` (the bridge `crates/willikins-server/src/mcp.rs` already documents) or it becomes
the workspace's first async HTTP dependency.

MCP adds a consent-screen MUST on top:
(source:
https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations.md,
2026-09-16)

- "Authorization servers:\n\n* **SHOULD** display additional warnings for `localhost`-only redirect URIs\n* **MAY** require additional attestation mechanisms for enhanced security\n* **MUST** clearly display the redirect URI hostname during authorization"

### 1.11 Refresh tokens: not issuing them is conformant, and it deletes the only stateful MUST

2026-07-28 adds a Refresh Tokens section that 2025-11-25 did not have. Nothing in it is an AS MUST.
(source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization.md,
2026-09-16)

- "* **MUST NOT** assume refresh tokens will be issued; the AS retains discretion"
- "**MCP Servers** (Protected Resources) **SHOULD NOT** include `offline_access` in\n`WWW-Authenticate` scope or Protected Resource Metadata `scopes_supported`, as refresh\ntokens are not a resource requirement."

If they *are* issued, the replay countermeasure is the one AS requirement that costs real storage.
Every MCP client using loopback redirects is a public client, so rotation-with-reuse-detection is
the only practical branch — a persisted refresh-token family table.
(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-13.txt, §4.3.1, 2026-09-16)

- "   Authorization servers MUST utilize one of these methods to detect\n   refresh token replay by malicious actors for public clients:\n\n   *  _Sender-constrained refresh tokens_ ... e.g., by utilizing DPoP [RFC9449] or mTLS [RFC8705].\n\n   *  _Refresh token rotation:_ the authorization server issues a new\n      refresh token with every access token refresh response.  The\n      previous refresh token is invalidated but information about the\n      relationship is retained by the authorization server ... it will\n      revoke the active refresh token as well as the access\n      authorization grant associated with it."

### 1.12 What MCP does not require at all, which is as important

- **Revocation: nothing.** A case-insensitive grep for `revoc`/`revoke` over all five fetched MCP
  authorization pages (2026-07-28 authorization plus its three sub-pages, and the whole 2025-11-25
  page) returns zero hits. RFC 8414 makes `revocation_endpoint` OPTIONAL. An operator-facing
  "revoke this token" control is a local feature, not a conformance item.
- **No dynamic client registration** (MAY, deprecated). **No OpenID Connect anything** (RFC 8414
  alone satisfies the discovery MUST). **No introspection endpoint** (RFC 9068 JWTs make it
  unnecessary; the RS half already validates them). **No JWKS is strictly required** by MCP —
  section 7 argues for publishing one anyway, for a seam reason rather than a conformance one.
- **Nothing whatsoever about how the authorization server authenticates the human.** That is
  entirely willikins' own design, constrained only by the two subject allowlists the plan already
  defines and by the consent screen's redirect-hostname MUST. Section 4 is therefore the section
  with the least specification behind it and the most judgement in it.

### 1.13 Co-hosting is free, and the ext-auth extensions are not obligations

`/.well-known/oauth-protected-resource` (RS) and `/.well-known/oauth-authorization-server` (AS)
are distinct paths on the same host, so one axum app serves both plus `/authorize` and `/token`
with no routing conflict and no second domain. All four join the unresolved `allowed_hosts`
exemption question the existing note already carries, since all four must be reachable before a
client has a token.

The `modelcontextprotocol/ext-auth` repository holds two extensions, both OPTIONAL and additive,
so neither adds an obligation. One of them matters as prior art for the seam:
(source:
https://raw.githubusercontent.com/modelcontextprotocol/ext-auth/main/specification/stable/enterprise-managed-authorization.mdx,
default branch head `fb374c7db2b34f18ca9183882e0beecdf661892b`, 2026-09-16)

- "- The **Resource Authorization Server** is the authorization server that issues access tokens for the MCP Server, as advertised in the MCP Server's Protected Resource Metadata ([RFC9728]).\n- The **IdP Authorization Server (IdP)** is the enterprise Identity Provider used for single sign-on."

The standards-track way to accept an external identity provider is therefore **not** to replace
your authorization server with theirs. It is to keep yours as the token issuer and add a grant
that trusts their assertion. That is the strongest single piece of evidence for where the seam
belongs.


## 2. Rust crates for the authorization-server half

The operator's expectation was "more than a handful state of the art Rust crates regarding Authn,
Authz, OAuth and sign in / sign up". That is true for the **client** and **resource-server**
halves and false for the **issuing** half. This section is the honest version of that answer.

**Verdict: write the authorization-code + PKCE flow directly over axum. Adopt no
authorization-server framework.** This is a conclusion from the measurements below, and it would
flip if the numbers were different.

### 2.1 There is one established general-purpose Rust authorization-server framework

Five crates.io searches — `q=oauth2+server`, `q=authorization+server`, `q=oidc+provider`,
`q=oauth+provider` (all sorted by recent downloads) and `keyword=oauth2` sorted by recent
downloads — return, above 10^4 recent downloads, only OAuth **clients** (`yup-oauth2` 4.5M,
`openidconnect` 3.96M, `openid`, `async-oauth2`, `google-oauth`) and **resource-server
validators** (`aliri` 290k, `aliri_oauth2`, `jwt-authorizer`, `tower-oauth2-resource-server`).
(source: https://crates.io/api/v1/crates?keyword=oauth2&sort=recent-downloads&per_page=20,
2026-09-16)

- " yup-oauth2                 12.1.2    recent=4521048  upd=2026-01-07\n openidconnect              4.0.1     recent=3957586  upd=2025-07-06  OpenID Connect library\n oxide-auth                 0.6.1     recent=622014   upd=2024-06-02  A OAuth2 library for common web servers\n oxide-auth-async           0.2.1     recent=536133   upd=2024-06-02\n oauth2-test-server         0.2.3     recent=20668    upd=2026-06-24\n oxide-auth-actix           0.3.0     recent=9414     upd=2024-05-25\n authkestra-op              0.11.1    recent=8561     upd=2026-09-15  OpenID Provider (OP) support for the authkestra framework\n oxide-auth-axum            0.6.0     recent=8180     upd=2025-01-09"

The only issuing-side entries in the whole top-20 are the `oxide-auth` family, `authkestra-op`
and `oauth2-test-server`. That is the state of the art.

### 2.2 `oxide-auth`: fits the tree, supplies the cheap half, supplies none of the expensive half

The pleasant surprise first: it is **not** disqualified on the axum version. `oxide-auth-axum`
0.6.0 declares `axum ^0.8` with default features off, and the lock holds axum 0.8.9.
(source: https://crates.io/api/v1/crates/oxide-auth-axum/0.6.0/dependencies, 2026-09-16)

- "=== oxide-auth-axum/0.6.0\n axum                   ^0.8           opt=False kind=normal default_features=False features=['form', 'query']\n oxide-auth             ^0.6           opt=False kind=normal default_features=True features=[]"

It is also pure Rust — no `openssl-sys`, no `aws-lc-sys`, no `cmake` — so it builds in the current
image. What it costs is three duplicate majors against crates the workspace already pins higher,
plus three genuinely new crates.
(source: https://crates.io/api/v1/crates/oxide-auth/0.6.1/dependencies, 2026-09-16)

- "=== oxide-auth/0.6.1\n base64                 ^0.21          opt=False kind=normal\n chrono                 ^0.4           opt=False kind=normal\n hmac                   ^0.12.0        opt=False kind=normal\n once_cell              ^1.3.1         opt=False kind=normal\n rand                   ^0.8           opt=False kind=normal\n rmp-serde              ^1.1           opt=False kind=normal\n rust-argon2            ^2.0           opt=False kind=normal\n sha2                   ^0.10.1        opt=False kind=normal\n subtle                 ^2.4.1         opt=False kind=normal\n url                    ^2.2.2         opt=False kind=normal"

The lock holds `base64` 0.23.1, `sha2` 0.11.0, and `rand` 0.9.5 *and* 0.10.2 — so `rand ^0.8` is a
third major. `once_cell` 1.21.4 and `async-trait` 0.1.92 are already present and cost nothing;
`hmac`, `rust-argon2` and `rmp-serde` are new.

**The decisive fact.** A case-insensitive grep for `well-known`, `jwks`, `rfc ?8414`, `rfc ?7591`,
`8707` and `at+jwt` across the non-test sources of `oxide-auth` 0.6.1 **and** `oxide-auth-async`
0.2.1 returns nothing.
(source: https://static.crates.io/crates/oxide-auth/oxide-auth-0.6.1.crate, extracted and grepped,
2026-09-16)

- "$ grep -rniE \"well-known|jwks|rfc ?8414|rfc ?7591|8707|at\\+jwt\" oxide-auth-0.6.1/src oxide-auth-async-0.2.1/src --include='*.rs'\n[exit 1]"

So after adopting it, willikins still writes: the RFC 8414 metadata document, the JWKS route, the
registration path, the RFC 8707 resource→audience binding, the `iss` parameter, and the RFC 9068
JWT issuer. Section 1's entire must-build list survives the adoption. The audience gap is the
worst of these — `Grant` has no audience or resource field at all:
(source: same tarball, `src/primitives/grant.rs`, 2026-09-16)

- "pub struct Grant {\n    /// Identifies the owner of the resource.\n    pub owner_id: String,\n\n    /// Identifies the client to which the grant was issued.\n    pub client_id: String,\n\n    /// The scope granted to the client.\n    pub scope: Scope,\n\n    /// The redirection uri under which the client resides.\n    pub redirect_uri: Url,\n\n    /// Expiration date of the grant (Utc).\n    pub until: Time,\n\n    /// Encoded extensions existing on this Grant\n    pub extensions: Extensions,\n}"

An audience could ride in the `Extensions` blob via a custom `Extension` impl, but `Extensions` is
serialised with `rmp-serde` as an opaque MessagePack side-channel — an untyped string where the
willikins invariant says ports carry domain types from `willikins-types`.

Its open issues corroborate the gaps, and they are years old.
(source: https://api.github.com/repos/197g/oxide-auth/issues?state=open&per_page=40, 2026-09-16)

- "IS  2022-07-13 Support RFC 7591 - Dynamic Client Registration\nIS  2022-01-29 JWT and JWE compatible Self-Encoded Access Tokens\nIS  2021-06-18 Implementing token backend persistency\nIS  2022-11-25 grant_type password not support"

### 2.3 Where `oxide-auth` is right, and where its defaults fight OAuth 2.1

Right: the implicit grant is structurally absent (`response_type` is accepted only when it equals
the literal `"code"`), and there is no password grant at all — the sole occurrence of the literal
`"password"` in the whole `src` tree is a scope *name* inside `#[cfg(test)] mod tests`.
(source: same tarball, `src/code_grant/authorization.rs` and a grep over `src`, 2026-09-16)

- "        match request.response_type() {\n            Some(ref method) if method.as_ref() == \"code\" => (),\n            _ => {\n                let prepared_error = ErrorUrl::with_request(\n                    request,\n                    (*bound_client.redirect_uri).to_url(),\n                    AuthorizationErrorType::UnsupportedResponseType,\n                );"

Wrong, in three places, all of them RFC 6749-era defaults:

- PKCE is an opt-in `GrantExtension`. `Pkce::optional()` exists and accepts a request with no
  challenge; `challenge()` defaults an absent method to `"plain"`; and `allow_plain()` is a public
  method that re-enables the method §1.5 shows draft-16 does not define.
  (source: same tarball, `src/code_grant/extensions/pkce.rs`, 2026-09-16)
  - "    /// Allow usage of the less secure `plain` verification method. This method is NOT secure\n    /// an eavesdropping attacker such as rogue processes capturing a devices requests.\n    pub fn allow_plain(&mut self) {\n        self.allow_plain = true;\n    }\n...\n    pub fn challenge(\n        &self, method: Option<Cow<str>>, challenge: Option<Cow<str>>,\n    ) -> Result<Option<Value>, ()> {\n        let method = method.unwrap_or(Cow::Borrowed(\"plain\"));"
- Redirect matching defaults to **semantic**, not exact, through the conversion a caller reaches
  for first.
  (source: same tarball, `src/primitives/registrar.rs`, 2026-09-16)
  - "pub enum RegisteredUrl {\n    /// An exact URL that must be match literally when the client uses it.\n    ...\n    Exact(ExactUrl),\n    /// An URL that needs to match the redirect URL semantically.\n    Semantic(Url),\n...\nimpl From<Url> for RegisteredUrl {\n    fn from(url: Url) -> Self {\n        RegisteredUrl::Semantic(url)\n    }\n}"
- The authorization response carries exactly two query pairs, and `iss` is not one of them. This
  is affirmative evidence, not a negative grep: these are *all* the pairs the crate appends.
  (source: same tarball, `src/code_grant/authorization.rs`, 2026-09-16)
  - "        url.query_pairs_mut()\n            .append_pair(\"code\", grant.as_str())\n            .extend_pairs(self.state.map(|v| (\"state\", v)))\n            .finish();\n        Ok(url)"

Conformance is *reachable* — `Pkce::required()` with `allow_plain` false does fail closed, and
`RegisteredUrl::Exact` exists — but it is a configuration obligation enforced by nothing in the
type system. That is exactly the correct-by-remembering pattern the design doc rejects elsewhere
when it says redaction is by construction.

### 2.4 What adopting it would cost, measured in lines

(source: line counts over the published tarballs, 2026-09-16)

- "     575 oxide-auth-0.6.1/src/code_grant/authorization.rs\n     730 oxide-auth-0.6.1/src/code_grant/accesstoken.rs\n     186 oxide-auth-0.6.1/src/code_grant/extensions/pkce.rs\n    1491 total\n-- unwanted grants:\n     569 refresh.rs\n     621 client_credentials.rs\n     424 resource.rs\n    1614 total\n--- whole src 13560; oxide-auth-async-0.2.1 src 4857; oxide-auth-axum-0.6.0 src 315"

**18,732 lines of dependency across three crates to obtain roughly 1,500 lines of wanted logic**,
with every document in §2.2 still unwritten, three duplicate majors, and an async path that costs
a boxed future per primitive call. Two further frictions: `Issuer` and `Authorizer` — the two
primitives willikins would implement most heavily — return `Result<_, ()>`, so no reason for a
failure ever crosses the boundary, which collides with plan decision 1's per-check
`AuthFailedReason`; and the bundled issuers are discarded anyway because willikins' token format
is already frozen as an RFC 9068 asymmetric JWT.
(source: same tarball, `src/primitives/authorizer.rs` and `issuer.rs`, 2026-09-16)

- "pub trait Authorizer {\n    fn authorize(&mut self, _: Grant) -> Result<String, ()>;\n\n    /// Retrieve the parameters associated with a token, invalidating the code in the process. In\n    /// particular, a code should not be usable twice (there is no stateless implementation of an\n    /// authorizer for this reason).\n    fn extract(&mut self, token: &str) -> Result<Option<Grant>, ()>;\n}"

Freshness: last release **2024-06-02**, last commit 2026-01-31, no declared MSRV, 783 stars, 30
open issues (of which 10 are PRs), not archived.
(source: https://api.github.com/repos/HeroicKatora/oxide-auth, 2026-09-16)

- "{\n \"full_name\": \"197g/oxide-auth\",\n \"stargazers_count\": 783,\n \"open_issues_count\": 30,\n \"pushed_at\": \"2026-01-31T17:31:10Z\",\n \"archived\": false,\n \"default_branch\": \"master\"\n}"

### 2.5 `oauth-as` 0.9.4: the closest fit found, and the reason not to take it yet

Stated honestly because it is the single most on-target crate in the ecosystem. Its module layout
carries exactly the documents §1 requires: `metadata.rs` (RFC 8414, 568 lines), `registration.rs`
(RFC 7591, 1,474), `cimd.rs` (991), `par.rs` (RFC 9126), `jwt.rs` (RFC 9068), `rate_limit.rs`,
`consent.rs`, `events.rs`. PKCE is S256-only with `plain` and the implicit grant refused by
construction.
(source: https://static.crates.io/crates/oauth-as/oauth-as-0.9.4.crate, `src/lib.rs`, 2026-09-16)

- "#![forbid(unsafe_code)]\n...\n//! An embeddable OAuth 2.1 Authorization Server library.\n//!\n//! This crate is the AUTHORIZATION SERVER half of OAuth: it registers clients, runs grant state\n//! machines, issues and introspects tokens, and produces exactly the wire shapes the RFCs define.\n//! It is a LIBRARY, not a server binary\n...\n//! - The authorization code grant with MANDATORY PKCE ([`authorization`]), the OAuth 2.1 stance:\n//!   validation, single-use codes, exact redirect-URI matching, and replay detection that revokes\n//!   the whole issued family."

Its dependency hygiene is better than anything else surveyed: four always-compiled crates, no
`rsa`, no openssl, no cmake, MSRV 1.75, MIT OR Apache-2.0, axum 0.8 behind a feature.
(source: https://crates.io/api/v1/crates/oauth-as/0.9.4/dependencies, 2026-09-16)

- " base64           ^0.22      opt=False kind=normal\n getrandom        ^0.3       opt=False kind=normal\n serde            ^1         opt=False kind=normal\n sha2             ^0.10      opt=False kind=normal\n axum             ^0.8       opt=True  kind=normal\n p256             ^0.13      opt=True  kind=normal"

And its maturity is not.
(source: https://api.github.com/repos/MattJackson/oauth-as, 2026-09-16)

- "{\n \"full_name\": \"MattJackson/oauth-as\",\n \"stargazers_count\": 1,\n \"open_issues_count\": 5,\n \"created_at\": \"2026-08-08T15:45:18Z\",\n \"pushed_at\": \"2026-09-06T23:28:29Z\",\n \"archived\": false,\n \"forks_count\": 0,\n \"subscribers_count\": 0\n}"

**Six weeks old. Pre-1.0, six releases in a month. One author, one star, zero forks, no audit.
36,230 lines of non-test security-critical source.** Its signer trait is `Es256Signer` — ES256
only — which would freeze willikins' signing algorithm.

Putting the whole token-issuing security of willikins on it today would be a larger unreviewed
trust surface than the vendor product the operator rejected, and a moving one. The right call is:
**do not depend on it in milestone 2c; read it as prior art** — its storage and signer conformance
suites and its module decomposition are directly useful — and record a dated re-evaluation gated
on 1.0, a second maintainer, or an audit.

### 2.6 The two rejected on objective grounds

`authkestra-op` 0.11.1 is not a library but a slice of a framework: adopting it means adopting
authkestra. 38 versions since 2026-07-24, no declared MSRV.
(source: https://crates.io/api/v1/crates/authkestra-op/0.11.1/dependencies, 2026-09-16)

- " async-trait              ^0.1         opt=False kind=normal\n authkestra-crypto-util   ^0.11.1      opt=False kind=normal\n authkestra-engine        ^0.11.1      opt=False kind=normal\n authkestra-resource      ^0.11.1      opt=False kind=normal\n jsonwebtoken             ^11          opt=False kind=normal"

`mcp-oauth` 0.3.0 declares `rust_version: 1.92`, **above the workspace's declared MSRV of 1.88**,
so it is disqualified before any other consideration; it also has 112 all-time downloads and has
not been touched since 2026-04-05.
(source: https://crates.io/api/v1/crates/mcp-oauth, 2026-09-16)

- "{\"num\": \"0.3.0\", \"created_at\": \"2026-04-05T02:20:08.335642Z\", \"rust_version\": \"1.92\", \"edition\": \"2024\", \"yanked\": false, \"license\": \"MIT OR Apache-2.0\"}"

### 2.7 Where the verdict actually turns: the registration path

The line-count comparison understates the real argument. §1.9 gives three registration options and
**neither `oxide-auth` nor a direct build gives CIMD or DCR for free** — only `oauth-as` does, and
§2.5 declines it. So the choice is between:

- **CIMD**: an outbound fetch of a client-chosen URL (§1.10's SSRF surface and the invariant
  clarification), portable client ids across a later AS swap (§7), and evidence that at least one
  real client probes for it (§6.7).
- **DCR**: an unauthenticated write endpoint, a persisted client registry, a garbage collector and
  a rate limit (§6.10 gives the shape of that cost).
- **Pre-registration**: one environment entry per client, zero attack surface, zero new endpoints,
  and clients that must all re-register the day the advertised AS changes (§1.2).

For one or a few human operators and a handful of agent clients, pre-registration is not a
compromise; it is the right answer, and it is what makes the in-house AS genuinely small. CIMD is
the one to add if and when a client that needs it must connect. This is a plan decision and the
evidence for each branch is above; it is not settled here.


## 3. Issuing the token: signing, JWKS, and key lifecycle

The good news in this whole note lives here: **the minting half costs zero new crates.**
`jsonwebtoken` 11, which the plan already pins for validation, does all of it.

### 3.1 Signing needs nothing new

`encode(&Header, &claims, &EncodingKey)` is the whole API, and it fails closed twice on an
algorithm/key-family mismatch. `Header.typ` and `Header.kid` are plain public `Option<String>`
fields, so RFC 9068's `typ: "at+jwt"` and a `kid` are field assignments — note `Header::new(alg)`
defaults `typ` to `"JWT"`, so it must be overwritten. `EncodingKey` derives `Zeroize,
ZeroizeOnDrop` and its `Debug` prints `content: "[redacted]"`.
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/encoding.rs, 2026-09-16)

- "pub fn encode<T: Serialize>(header: &Header, claims: &T, key: &EncodingKey) -> Result<String> {\n    if key.family != header.alg.family() {\n        return Err(new_error(ErrorKind::InvalidAlgorithm));\n    }\n\n    let signing_provider = (CryptoProvider::get_default().signer_factory)(&header.alg, key)?;\n\n    if signing_provider.algorithm() != header.alg {\n        return Err(new_error(ErrorKind::InvalidAlgorithm));\n    }"

Both crypto backends ship the same four modules and the same algorithm surface: ES256/ES384,
EdDSA over Ed25519 only, RSA (PKCS#1 v1.5 and PSS), HS*. No P-521, no Ed448, no `ring` backend.
(source:
https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/crypto/rust_crypto/mod.rs,
2026-09-16)

- "fn new_signer(algorithm: &Algorithm, key: &EncodingKey) -> Result<Box<dyn JwtSigner>, Error> {\n    let jwt_signer = match algorithm {\n        Algorithm::HS256 => ...\n        Algorithm::ES256 => Box::new(ecdsa::Es256Signer::new(key)?) as Box<dyn JwtSigner>,\n        Algorithm::ES384 => ...\n        Algorithm::RS256 => ...\n        Algorithm::PS256 => ...\n        Algorithm::EdDSA => Box::new(eddsa::EdDSASigner::new(key)?) as Box<dyn JwtSigner>,\n    };"

Both asymmetric signers want **PKCS#8 DER**, which is what makes key generation a question about
which crate emits PKCS#8.
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/crypto/rust_crypto/eddsa.rs,
2026-09-16)

- "use ed25519_dalek::pkcs8::DecodePrivateKey;\nuse ed25519_dalek::{Signature, SigningKey, VerifyingKey};\n...\n            SigningKey::from_pkcs8_der(encoding_key.as_bytes())"

### 3.2 The JWKS endpoint needs nothing new either

This closes the gap the existing note left when it only quoted `JwkSet::find`. `jsonwebtoken` 11
derives the **public** JWK from the **private** key, and both `Jwk` and `JwkSet` derive
`Serialize`, so `/jwks.json` is `serde_json::to_string(&JwkSet { keys })`. The `key_utils`
functions it calls are implemented by **both** backends, so the path is not a stub under either
feature choice.
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/jwk.rs, 2026-09-16)

- "    /// Create a `JWK` from an `EncodingKey`.\n    pub fn from_encoding_key(key: &EncodingKey, alg: Algorithm) -> errors::Result<Self> {\n        Ok(Self {\n            common: CommonParameters { key_algorithm: Some(alg.into()), ..Default::default() },\n            algorithm: match key.family() {\n...\n                AlgorithmFamily::Ec => {\n                    let (curve, x, y) = (CryptoProvider::get_default()\n                        .key_utils\n                        .ec_pub_components_from_private_key)(\n                        key.as_bytes(), alg\n                    )?;"

It also computes an RFC 7638 thumbprint natively, for EC and for OKP/Ed25519, in the required
lexicographic member order — so `kid` can be derived from the public key alone, with no registry
for the two halves of one process to agree a name in.
(source: same file, 2026-09-16)

- "    /// Compute the thumbprint of the JWK.\n    ///\n    /// Per [RFC-7638](https://datatracker.ietf.org/doc/html/rfc7638)\n    pub fn thumbprint(&self, hash_function: ThumbprintHash) -> errors::Result<String> {\n        let pre = match &self.algorithm {\n            AlgorithmParameters::EllipticCurve(a) => match a.curve {\n                EllipticCurve::P256 | EllipticCurve::P384 | EllipticCurve::P521 => {\n                    format!(\n                        r#\"{{\"crv\":{},\"kty\":{},\"x\":\"{}\",\"y\":\"{}\"}}\"#,"

(source: https://www.rfc-editor.org/rfc/rfc8037.txt, §2, 2026-09-16)

- "   When calculating JWK Thumbprints [RFC7638], the three public key\n   fields are included in the hash input in lexicographic order: \"crv\",\n   \"kty\", and \"x\"."

### 3.3 Two traps in the free path, each worth an acceptance test

**Trap 1: `from_encoding_key` leaves `kid` as `None`, and `JwkSet::find` skips keys without one.**
So a JWKS built straight from `from_encoding_key` contains a key the resource-server half can
never select, and plan decision 1's `kid` step would fail every token willikins minted itself. The
issuer must set `jwk.common.key_id` and `Header.kid` to the same value.
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/jwk.rs, 2026-09-16)

- "            common: CommonParameters { key_algorithm: Some(alg.into()), ..Default::default() },"
- "    pub fn find(&self, kid: &str) -> Option<&Jwk> {\n        self.keys\n            .iter()\n            .find(|jwk| jwk.common.key_id.is_some() && jwk.common.key_id.as_ref().unwrap() == kid)\n    }"

**Trap 2, the most surprising fact in this note: an Ed25519 key that signs correctly cannot
produce a JWKS entry unless it is PKCS#8 *v1*.** `Jwk::from_encoding_key` matches the Ed key
length against exactly 48 bytes.
(source: same file, 2026-09-16)

- "                AlgorithmFamily::Ed => {\n                    // Get the curve type based off the encoding key length\n                    // Note: here we will receive a DER key which contains a 16 byte ANS.1 header\n                    let curve_type: EllipticCurve = match key.as_bytes().len() {\n                        // 16 byte header + 32 byte Ed25519 key\n                        48 => Ok(EllipticCurve::Ed25519),\n                        _ => Err(Error::from(ErrorKind::InvalidEddsaKey)),\n                    }?;"

Every default Ed25519 generator examined emits **v2**, which is longer. `ed25519-dalek`'s
`EncodePrivateKey` always sets a public key field:
(source: https://static.crates.io/crates/ed25519/ed25519-2.2.3.crate, 2026-09-16)

- "        let private_key_info = PrivateKeyInfo {\n            algorithm: ALGORITHM_ID,\n            private_key: &private_key,\n            public_key: self.public_key.as_ref().map(|pk| pk.0.as_slice()),\n        };"

and so does `ring`, whose own doc comment records that `openssl genpkey -algorithm ED25519`
emits v1 — confirming v1 is a normal encoding, just not ring's output:
(source: https://static.crates.io/crates/ring/ring-0.17.14.crate, 2026-09-16)

- "    /// The PKCS#8 document will be a v2 `OneAsymmetricKey` with the public key,\n    /// as described in [RFC 5958 Section 2]\n...\n    /// `openssl genpkey -algorithm ED25519` generates PKCS# v1 keys, which\n    /// require the use of `Ed25519KeyPair::from_pkcs8_maybe_unchecked()`"

The fixes are to build `KeypairBytes { secret_key, public_key: None }` by hand, or to use
`aws-lc-rs`'s `generate_pkcs8v1`, the only first-class v1 API found:
(source: https://static.crates.io/crates/aws-lc-rs/aws-lc-rs-1.15.0.crate, 2026-09-16)

- "    /// The PKCS#8 document will be a v1 `PrivateKeyInfo` structure (RFC5208). Use this method\n    /// when needing to produce documents that are compatible with the OpenSSL CLI.\n...\n    pub fn generate_pkcs8v1(_rng: &dyn SecureRandom) -> Result<Document, Unspecified> {"

**ES256 has no equivalent trap** — the EC path parses with `from_pkcs8_der` and has no length
check. That is a quiet argument for ES256 as the in-house issuer's algorithm.

### 3.4 RUSTSEC-2023-0071 now applies, and the fix is a normative constraint

The `rust_crypto` feature is one all-or-nothing bundle with `dep:rsa` inside it, so choosing it to
get ES256 or EdDSA compiles `rsa` into the binary whether or not any RSA algorithm is used.
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/Cargo.toml, 2026-09-16)

- "rust_crypto = [\n    \"dep:ed25519-dalek\",\n    \"dep:hmac\",\n    \"dep:p256\",\n    \"dep:p384\",\n    \"dep:rand\",\n    \"dep:rsa\",\n    \"dep:sha2\",\n]"

(source: https://raw.githubusercontent.com/rustsec/advisory-db/main/crates/rsa/RUSTSEC-2023-0071.md,
2026-09-16)

- "[versions]\npatched = []\n...\n### Impact\nDue to a non-constant-time implementation, information about the private key is leaked through timing information which is observable over the network.\n\n### Patches\nNo patch is yet available."

The old dismissal ("a resource server only verifies with public keys") is void: an issuer signs,
and the advisory is a private-key signing-timing leak. The dismissal survives **only** on a new
and narrower ground the plan must state normatively: **willikins never uses the RSA algorithm
family at all.** That is enforceable, because the families are disjoint and `encode` rejects a
mismatch:
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/algorithms.rs,
2026-09-16)

- "            Self::Rsa => [RS256, RS384, RS512, PS256, PS384, PS512],\n            Self::Ec => &[Algorithm::ES256, Algorithm::ES384],\n            Self::Ed => &[Algorithm::EdDSA],"

### 3.5 RFC 9068 says an RS256-conforming server MUST support RS256 — a real collision

This is a plan decision, not a research conclusion, and it must be made deliberately.
(source: https://www.rfc-editor.org/rfc/rfc9068.txt, §2.1, 2026-09-16)

- "   JWT access tokens MUST be signed.  Although JWT access tokens can use\n   any signing algorithm, use of asymmetric cryptography is RECOMMENDED\n   ...  JWT access tokens MUST NOT use\n   \"none\" as the signing algorithm.\n...\n   Authorization servers and resource servers conforming to this\n   specification MUST include RS256 (as defined in [RFC7518]) among\n   their supported signature algorithms."

Three exits are visible from the fetched evidence: (a) document the non-conformance and ship
ES256 or EdDSA only; (b) take the `aws_lc_rs` backend, which avoids the `rsa` crate entirely, and
pay the Dockerfile change §8 records; (c) keep RS256 in the **validator's** allowlist — where the
old reasoning does hold, because verification uses only public keys — while the **issuer** signs
solely ES256 or EdDSA. Exit (c) satisfies "supported signature algorithms" on the resource-server
side and dodges the timing leak on the issuing side, and is the one this note recommends. One
constraint from the existing note §4.2 makes (c) non-trivial: `jsonwebtoken` rejects a
`Validation` whose algorithm list spans key families, so (c) is implementable only as **per-`kid`
algorithm selection**, never as one global list — which the JWKS already supports, since each key
carries its own `alg`. That is a design consequence, not a footnote.

### 3.6 Key generation: which crate, and the RNG mismatch nobody expected

`p256` 0.13.2 and `p384` are **already pulled** by `rust_crypto`, and `jsonwebtoken` declares
them without `default-features = false`, so `arithmetic`, `pem` and `pkcs8` are already on:
generation and PKCS#8 PEM output are available with no feature change at all.
(source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/Cargo.toml, 2026-09-16)

- "p256 = { version = \"0.13.2\", optional = true, features = [\"ecdsa\"] }\np384 = { version = \"0.13.0\", optional = true, features = [\"ecdsa\"] }\nrand = { version = \"0.8.5\", optional = true, features = [\n    \"std\",\n], default-features = false }"

(source: https://static.crates.io/crates/elliptic-curve/elliptic-curve-0.13.8.crate, 2026-09-16)

- "    /// Generate a random [`SecretKey`].\n    #[cfg(feature = \"arithmetic\")]\n    pub fn random(rng: &mut impl CryptoRngCore) -> Self\n...\n    fn to_pkcs8_der(&self) -> pkcs8::Result<der::SecretDocument> {\n        let algorithm_identifier = pkcs8::AlgorithmIdentifierRef {\n            oid: ALGORITHM_OID,\n            parameters: Some((&C::OID).into()),\n        };\n\n        let ec_private_key = self.to_sec1_der()?;\n        let pkcs8_key = pkcs8::PrivateKeyInfo::new(algorithm_identifier, &ec_private_key);"

The trap: RustCrypto's `CryptoRngCore` bound is **`rand_core` 0.6**, which neither of the tree's
`rand` majors (0.9.5, 0.10.2) satisfies.
(source: same tarball, `Cargo.toml`, 2026-09-16)

- "[dependencies.rand_core]\nversion = \"0.6.4\"\ndefault-features = false"

The zero-new-crate route is to declare `rand = "0.8"` directly — the same major `rust_crypto`
already pulls — and pass `rand::rngs::OsRng`. `ring`'s `SystemRandom` has no such bound and is
already in the lock via rustls, which is a genuine point in its favour for EC generation; whether
its deliberately `parameters`-free inner `ECPrivateKey` round-trips through
`EncodingKey::from_ec_der` was **not** verified and is a §9 item.
(source: https://static.crates.io/crates/ring/ring-0.17.14.crate, 2026-09-16)

- "    /// The PKCS#8 document will be a v1 `OneAsymmetricKey` with the public key\n    /// included in the `ECPrivateKey` structure, as described in\n    /// [RFC 5958 Section 2] and [RFC 5915].  The `ECPrivateKey` structure will\n    /// not have a `parameters` field so the generated key is compatible with\n    /// PKCS#11."

### 3.7 Rotation: what the RFCs actually mandate, which is almost nothing

RFC 7517 §4.5 is the only normative statement, and it prescribes no overlap length.
(source: https://www.rfc-editor.org/rfc/rfc7517.txt, 2026-09-16)

- "   The \"kid\" (key ID) parameter is used to match a specific key.  This\n   is used, for instance, to choose among a set of keys within a JWK Set\n   during key rollover.  The structure of the \"kid\" value is\n   unspecified.  When \"kid\" values are used within a JWK Set, different\n   keys within the JWK Set SHOULD use distinct \"kid\" values."

RFC 9068 §4 supplies the reason the **published set** is the trust boundary, and the warning that
publishing more keys widens the compromise surface — so two live keys is a ceiling during a
rotation, not a steady state.
(source: https://www.rfc-editor.org/rfc/rfc9068.txt, 2026-09-16)

- "   Given\n   that resource servers have no way of knowing what key should be used\n   to validate JWT access tokens in particular, they have to accept\n   signatures performed with any of the keys published in AS metadata or\n   OpenID Connect discovery; consequently, an attacker just needs to\n   compromise any key among the ones published to be able to generate\n   and sign JWTs that will be accepted as valid by the resource server."

So the mechanism is fully determined and the numbers are willikins': publish both keys with
distinct `kid`s (a thumbprint needs no registry), sign with the new one from the moment it is
published, keep the old one until every token it signed has expired, then drop it. **The overlap
window is exactly the maximum access-token lifetime**, which no specification sets.

### 3.8 Where the private key lives, and what happens if it does not persist

Railway's constraints are tight and one of them is already handled in this repo.
(source: https://docs.railway.com/reference/volumes, 2026-09-16)

- "Here are some limitations of which we are currently aware: Each service can only have a single volume Replicas cannot be used with volumes To prevent data corruption, we prevent multiple deployments from being active and mounted to the same service. This means that there will be a small amount of downtime when re-deploying a service that has a volume attached ... Docker images that run as a non-root UID by default will have permissions issues when performing operations within an attached volume."

The Dockerfile already runs as root on `gcr.io/distroless/cc-debian12` precisely so the mounted
volume stays writable, and the volume already holds the journal. So a PKCS#8 PEM at mode 0600
beside the journal, generated on first boot if absent, is consistent with what the deployment
already does. The alternative — the PEM as an environment variable through the existing Doppler
integration — matches the project rule that all secrets live in Doppler, at the cost of a rotation
being a variable change plus a redeploy. Both fit; this is a plan decision.

**If the key is not persisted, every outstanding access token dies silently on every restart.** A
fresh key has a different thumbprint, so the published JWKS after the restart contains only the new
`kid`, `JwkSet::find` returns `None` for every older token, and RFC 9068 §4 requires the resource
server to answer `invalid_token`. Functionally that is a zero-overlap rotation forced on every
deploy — and Railway redeploys a volume-attached service with downtime anyway, so restarts are not
rare. Two consequences for this milestone: the frozen claims fixture and the in-process fake
authorization server need a **fixed test key**, never a generated one, or the fixture's `kid`
changes on every run; and an ephemeral key also breaks the `/approvals` session if that session
ever depends on a signed artefact derived from it.

### 3.9 The claims do not collapse just because one process plays both roles

RFC 9068 has no single-party carve-out. `iss`, `exp`, `aud`, `sub`, `client_id`, `iat` and `jti`
are all REQUIRED — `jti` being REQUIRED here even though it is only OPTIONAL in plain RFC 7519.
(source: https://www.rfc-editor.org/rfc/rfc9068.txt, §2.2, 2026-09-16)

- "   iss  REQUIRED - as defined in Section 4.1.1 of [RFC7519].\n\n   exp  REQUIRED ...\n\n   aud  REQUIRED ...\n\n   sub  REQUIRED ...\n\n   client_id  REQUIRED - as defined in Section 4.3 of [RFC8693].\n\n   iat  REQUIRED ...\n\n   jti  REQUIRED - as defined in Section 4.1.7 of [RFC7519]."

And `iss` and `aud` do **not** collapse to one string when issuer and resource share an origin:
(source: same RFC, §3 and §5, 2026-09-16)

- "   If the request includes a \"resource\" parameter (as defined in\n   [RFC8707]), the resulting JWT access token \"aud\" claim SHOULD have\n   the same value as the \"resource\" parameter in the request.\n...\n   If the request does not include a \"resource\" parameter, the\n   authorization server MUST use a default resource indicator in the\n   \"aud\" claim."
- "   To prevent cross-JWT confusion, authorization servers MUST use a\n   distinct identifier as an \"aud\" claim value to uniquely identify\n   access tokens issued by the same issuer for distinct resources."

`iss` is the RFC 8414 issuer identifier; `aud` is always a resource identity. Two different
strings, in one process.

### 3.10 The metadata document is a serde struct, not a crate

Three crates.io searches — "oauth authorization server metadata", "rfc8414", "authorization
server metadata" — surface nothing maintained and widely used that *serves* this document. The
nearest hits are clients, deserialisers, or three-figure-download projects.
(source: https://crates.io/api/v1/crates?q=oauth%20authorization%20server%20metadata&per_page=15,
2026-09-16)

- "atproto-oauth-aip 0.14.5 7754 | ATProtocol AIP OAuth tools\n...\nrustauth-oauth-provider 0.3.0 166 | OAuth 2.1 and OpenID Connect provider support for RustAuth.\n...\nturul-mcp-oauth 0.4.0 883 | OAuth 2.1 Resource Server support for Turul MCP framework"

A `#[derive(Serialize)]` struct behind an axum handler is the whole implementation — which is
already true of the RFC 9728 document the plan hand-rolls.


## 4. Authenticating the human

§1.12 records that MCP says nothing at all about this. It is the half every MCP-shaped
implementation refuses to write (§6.9), and it is where rauthy's other ~77,000 lines went (§6.2).
This section costs more judgement than any other.

**Recommendation: passkeys, with saved recovery codes, and a host-side break-glass.** Not
primarily for phishing resistance — for the three costs below that a password path cannot pay in
this deployment.

### 4.1 `webauthn-rs` is the only serious Rust relying party, and it fits the MSRV exactly

0.5.5 is the current stable release (2026-04-30); 0.6.1-dev is a pre-release and must not be cited
as current. `rust-version = 1.88` is **exactly** the workspace floor. MPL-2.0.
(source: https://crates.io/api/v1/crates/webauthn-rs, 2026-09-16)

- "webauthn-rs 0.6.1-dev 0.5.5 6814418 2495231 2026-04-30T02:03:55.653656Z / 0.5.5 2026-04-30T01:56:25.731940Z 1335253 1.88 False"

There is no alternative. A crates.io search sorted by downloads returns the kanidm family as the
only relying-party implementation; the 1Password `passkey-*` family is the **other** half of the
protocol (client and authenticator), and Mozilla's `authenticator` is a CTAP client.
(source: https://crates.io/api/v1/crates?q=webauthn&per_page=15&sort=downloads, 2026-09-16)

- "6814418 2495231 webauthn-rs 0.5.5 | Webauthn Framework for Rust Web Servers ... 3987520 994354 passkey-client 0.5.0 | Webauthn client in Rust. ... 3993492 994750 passkey-authenticator 0.5.0 | A webauthn authenticator supporting passkeys."

### 4.2 The finding that changes a frozen file: it needs OpenSSL

`webauthn-rs-core` depends **unconditionally** on `openssl` and `openssl-sys`, with no pure-Rust
backend and no feature to turn it off. No cmake, no aws-lc-sys.
(source: https://static.crates.io/crates/webauthn-rs-core/webauthn-rs-core-0.5.5.crate,
`Cargo.toml`, 2026-09-16)

- "[dependencies.openssl]\nversion = \"^0.10.75\"\n\n[dependencies.openssl-sys]\nversion = \"^0.9.111\""

(source:
https://raw.githubusercontent.com/kanidm/webauthn-rs/d2c10d53ca5ef033d37ee6462e936e9eb72ad98c/OpenSSL.md,
2026-09-16)

- "**tl;dr: the libraries in this package require OpenSSL v3.x.**\n...\n## Linux\n\nInstall `libssl-dev`, `openssl-dev` and/or `openssl-devel` from your package\nmanager. As long as you have OpenSSL v3.x, this should _Just Work_™."

Three consequences the plan must carry, because they are the kind of thing discovered at 3am in a
deploy rather than in a plan:

1. The Dockerfile builder comment — "no `aws-lc-sys`, `openssl-sys`, or `cmake` anywhere in the
   tree" — becomes **false** and must be rewritten.
2. The builder stage needs `libssl-dev` and `pkg-config` beside `gcc` and `libc6-dev`. Still no
   cmake, still not `build-essential`.
3. The runtime image is *probably* fine as-is, but this is the weakest citation in the note — an
   unpinned `main`-branch README that names no version. Treat as a §9 verify item.
   (source: https://raw.githubusercontent.com/GoogleContainerTools/distroless/main/base/README.md,
   2026-09-16)
   - "Most other applications (and Go apps that require libc/cgo) should start with `gcr.io/distroless/base`, which contains all\nof the packages in `gcr.io/distroless/static`, and \n\n* glibc\n* libssl"

### 4.3 The ceremonies, and the one obligation that decides the store shape

Four calls, two round trips each: `start_passkey_registration(user_unique_id: Uuid, user_name,
user_display_name, exclude_credentials) -> (CreationChallengeResponse, PasskeyRegistration)` then
`finish_passkey_registration(...) -> Passkey`; `start_passkey_authentication(&[Passkey])` then
`finish_passkey_authentication(...) -> AuthenticationResult`.

The in-progress state **must** be server-side, and the crate enforces it by refusing to derive
serde on it by default.
(source: https://static.crates.io/crates/webauthn-rs/webauthn-rs-0.5.5.crate, `src/lib.rs`,
2026-09-16)

- "This value *MUST* be persisted on the server. If you store this in a cookie or some other form of client side stored value, the client can replay a previous authentication state and signature without possession of, or interaction with the authenticator, bypassing pretty much all of the security guarantees of webauthn. Because of this risk by default these states are *not* allowed to be serialised which prevents them from accidentally being placed into a cookie."

That is exactly the shape milestone 2c already specifies for the pending-login store —
`__Host-willikins-login`, 300 s TTL, pruned on insert, capped at 1,024, 503 when full — and the
crate's `DEFAULT_AUTHENTICATOR_TIMEOUT` is 300 s, matching that TTL. **So on the passkey branch the
pending-login store stays.** §5.3 records that another reader wanted it deleted; that
recommendation is conditional on a form login and does not survive a passkey ceremony.

The caller also has an obligation the crate cannot enforce: after `finish_passkey_registration`
it "MUST assert that the registered `CredentialID` has not previously been registered to any
other account".

### 4.4 The relying-party identifier is the one string that cannot ever change

`WebauthnBuilder::new(rp_id, rp_origin)` refuses unless rp_id is an effective domain of the origin.
(source: same tarball, `src/lib.rs`, 2026-09-16)

- "/// rp_id is what Credentials (Authenticators) bind themselves to - rp_id can NOT be changed\n/// without breaking all of your users' associated credentials in the future!\n...\n/// rp_id *must* be an effective domain of rp_origin. This means that if you are hosting\n/// `https://idm.example.com`, rp_id must be `idm.example.com`, `example.com` or `com`."

The operator's "nothing is forever" rule bites here harder than anywhere else in the plan: the
audience-migration path decision 12 gives for the resource identifier **has no analogue**. WebAuthn
Level 3's Related Origin Requests does not solve a domain *change* — it lets several origins share
one common RP ID, which must still be chosen once.
(source: https://www.w3.org/TR/webauthn-3/, §5.11, 2026-09-16, tag-stripped)

- "Such Relying Parties MUST choose a common RP ID to use across all ceremonies from related origins. A JSON document MUST be hosted at the webauthn well-known URL [RFC8615] for the RP ID , and MUST be served using HTTPS."

And `webauthn-rs` 0.5.5 has **no** Related Origin Requests support: the strings `related origin`
and `.well-known/webauthn` do not appear anywhere in the 0.5.5 sources of `webauthn-rs` or
`webauthn-rs-core`. If a second origin is ever needed, willikins serves the well-known document
itself and uses `append_allowed_origin` server-side.

### 4.5 The policy defaults, and the one knob the safe API does not expose

Attestation is `None` by design, user verification is `Required`, and synced authenticators are
accepted:
(source: https://static.crates.io/crates/webauthn-rs/webauthn-rs-0.5.5.crate, `src/lib.rs`,
2026-09-16)

- "            .attestation(AttestationConveyancePreference::None)\n            .credential_algorithms(self.algorithms.clone())\n            .require_resident_key(false)\n            .authenticator_attachment(None)\n            .user_verification_policy(UserVerificationPolicy::Required)\n            .reject_synchronised_authenticators(false)\n            .exclude_credentials(exclude_credentials)"

`UserVerificationPolicy::Required` is **stricter than NIST**, which asks only that UV be preferred
and the flag inspected — and it is what makes the crate's claim true that a passkey is
self-contained MFA, so no password is needed alongside it.
(source: https://pages.nist.gov/800-63-4/sp800-63b.html, 2026-09-16, tag-stripped)

- "The User Verified flag indicates that the authenticator has locally authenticated the user using one of the available “user verification” methods. Verifiers SHALL indicate that UV is preferred and SHALL inspect responses to confirm the value of the UV flag. This indicates whether the authenticator can be treated as a multi-factor cryptographic authenticator. If the user is not verified, agencies SHALL treat the authenticator as a single-factor cryptographic authenticator."

**Refusing cloud-synced passkeys is not reachable through the safe API** —
`reject_synchronised_authenticators(false)` is hardcoded and the knob lives on
`webauthn-rs-core`'s `new_challenge_register_builder`, behind `WebauthnCore::new_unsafe_experts_only`.
What willikins *can* do is observe: `backup_eligible` and `backup_state` are stored per credential
and returned on every authentication. Recommendation: record the flags, do not refuse — refusing a
synced credential also refuses the most likely recovery path for a single operator who loses a
device, and NIST agrees the flag is for policy, not for blanket rejection.

### 4.6 Counter regression and multiple credentials

`require_valid_counter_value` is true: a counter less than or equal to the stored one aborts with
`CredentialPossibleCompromise`, while 0-on-both-sides (normal for synced passkeys) is tolerated.
(source: https://static.crates.io/crates/webauthn-rs-core/webauthn-rs-core-0.5.5.crate,
`src/core.rs`, 2026-09-16)

- "            let counter_shows_compromise = auth_data.counter <= cred.counter;\n\n            if counter > cred.counter {\n                needs_update = true;\n            }\n\n            if self.require_valid_counter_value && counter_shows_compromise {\n                return Err(WebauthnError::CredentialPossibleCompromise);\n            }"

The application must then persist the update via `Passkey::update_credential`. Multiple
credentials are native on both sides — authentication takes a slice, registration takes
`exclude_credentials`:
(source: https://static.crates.io/crates/webauthn-rs/webauthn-rs-0.5.5.crate, `src/lib.rs`,
2026-09-16)

- "/// `exclude_credentials` ensures that a set of credentials may not participate in this registration.\n/// You *should* provide the list of credentials that are already registered to this user's account\n/// to prevent duplicate credential registrations. These credentials *can* be from different\n/// authenticator classes since we only require the `CredentialID`"

### 4.7 Testing without a browser, the same way the plan already tests OAuth

`webauthn-authenticator-rs` 0.5.5 (same repo, same MSRV) ships `SoftPasskey`, a software
authenticator, behind a feature whose deps land in dev-dependencies only. `SoftPasskey::new(falsify_uv:
bool)` exists specifically so a test can present a credential that lies about user verification,
and it keeps its own counter — which is what makes a counter-regression test possible.
(source:
https://raw.githubusercontent.com/kanidm/webauthn-rs/d2c10d53ca5ef033d37ee6462e936e9eb72ad98c/webauthn-authenticator-rs/Cargo.toml,
2026-09-16)

- "# TODO: allow running softpasskey without softtoken\nsoftpasskey = [\"crypto\", \"softtoken\"]\nsofttoken = [\"crypto\", \"ctap2\"]"

### 4.8 The browser cost: the first `<script>` on the approvals page

The ceremony runs through `navigator.credentials.create()`/`.get()`, and the page must base64url
decode the challenge, `user.id` and every credential id before the call and re-encode five fields
for the POST back. The upstream tutorial's `auth.js` is about 100 lines doing exactly that.
(source:
https://raw.githubusercontent.com/kanidm/webauthn-rs/d2c10d53ca5ef033d37ee6462e936e9eb72ad98c/tutorial/server/axum/assets/js/auth.js,
2026-09-16)

- "        credentialCreationOptions.publicKey.challenge = Base64.toUint8Array(credentialCreationOptions.publicKey.challenge);\n        credentialCreationOptions.publicKey.user.id = Base64.toUint8Array(credentialCreationOptions.publicKey.user.id);\n        credentialCreationOptions.publicKey.excludeCredentials?.forEach(function (listItem) {\n            listItem.id = Base64.toUint8Array(listItem.id)\n        });\n\n        return navigator.credentials.create({\n            publicKey: credentialCreationOptions.publicKey\n        });"

willikins' approvals page contains **no** `<script>` today — the only occurrence of the string in
`crates/willikins-server/src/http/approvals.rs` is an escaping fixture. So this adds the first
script to willikins' HTML surface, and milestone 2c's CSP (`frame-ancestors 'none'`,
`form-action 'self'`) **must grow a `script-src`** with a per-response nonce or hash. Inlining
keeps it one asset and avoids a second route.

It also needs a username step, because `start_passkey_authentication` takes the account's own
credentials — so the server must know which account before it can build the challenge. The
usernameless alternative needs the preview `conditional-ui` feature, which the authors advise
against:
(source: https://static.crates.io/crates/webauthn-rs/webauthn-rs-0.5.5.crate, `src/lib.rs`,
2026-09-16)

- "User testing has shown that these conditional UI flows in most browsers are hard to activate\n//! and may be confusing to users, as they attempt to force users to use caBLE/hybrid. We don't\n//! recommend conditional UI as a result."

For a single operator that is one text field, but it makes the login endpoint an
account-existence oracle unless it answers uniformly.

### 4.9 The password alternative, priced honestly

The crates are good and the defaults are already right. `argon2` 0.6.0 over `password-hash`
0.6.1 — note **0.6.0 of `password-hash` is yanked** — defaults to Argon2id with (m=19456 KiB,
t=2, p=1), which is byte-for-byte OWASP's second recommended configuration, so `Argon2::default()`
is already OWASP-compliant.
(source: https://static.crates.io/crates/argon2/argon2-0.6.0.crate, `src/params.rs`, 2026-09-16)

- "impl Params {\n    /// Default memory cost.\n    pub const DEFAULT_M_COST: u32 = 19 * 1024;\n...\n    /// Default number of iterations (i.e. \"time\").\n    pub const DEFAULT_T_COST: u32 = 2;\n...\n    /// Default degree of parallelism.\n    pub const DEFAULT_P_COST: u32 = 1;"

(source:
https://raw.githubusercontent.com/OWASP/CheatSheetSeries/8aaf426de610ea603c21cf6a75222a6b7a30f820/cheatsheets/Password_Storage_Cheat_Sheet.md,
2026-09-16)

- "Out of the three Argon2 versions, use the  Argon2id variant since it provides a balanced approach to resisting both side-channel and GPU-based attacks.\n...\n- m=47104 (46 MiB), t=1, p=1 (Do not use with Argon2i)\n- m=19456 (19 MiB), t=2, p=1 (Do not use with Argon2i)"

**The cost that kills it is not the crate. It is three things the deployment cannot pay:**

1. **Memory on an unauthenticated endpoint.** 19 MiB per in-flight verification, on a host whose
   own CLAUDE.md says builds must use `-j 2` because 11 GB is shared. The plan already caps the
   pending-login store at 1,024; 1,024 concurrent Argon2id verifications at OWASP parameters ask
   for roughly 19 GiB. A password path therefore needs a hashing semaphore and per-account attempt
   counters *before* it is safe to expose. A passkey authentication is one signature verification
   with no tunable memory cost. This is a security property, not a performance note.
2. **There is no password reset, at all.** Every OWASP method is side-channel bound — URL tokens by
   email, PINs by SMS — and the only method that is not is just pre-issued recovery identifiers,
   i.e. the same recovery codes a passkey deployment needs anyway. willikins has no email and no
   SMS.
   (source:
   https://raw.githubusercontent.com/OWASP/CheatSheetSeries/8aaf426de610ea603c21cf6a75222a6b7a30f820/cheatsheets/Forgot_Password_Cheat_Sheet.md,
   2026-09-16)
   - "In order to allow a user to request a password reset, you will need to have some way to identify the user, or a means to reach out to them through a side-channel.\n...\nOffline methods differ from other methods by allowing the user to reset their password without requesting a special identifier (such as a token or PIN) from the backend."
3. **The lockout mechanism's own escape hatch is unavailable.** OWASP requires the counter be per
   account and warns the mechanism is itself a denial-of-service vector whose usual mitigation is
   to allow the forgotten-password flow to log in even while locked out — which, per (2), willikins
   cannot offer. On top, NIST's blocklist-of-compromised-passwords requirement is SHALL-level and
   needs either third-party egress or a self-hosted corpus, which is the class of dependency the
   operator rejected; and a pepper "cannot be changed without knowledge of a user's password", so
   rotating it forces a reset this deployment cannot perform.

### 4.10 Recovery codes are needed on **either** branch, and they bring `argon2` with them

NIST gives implementable numbers.
(source: https://pages.nist.gov/800-63-4/sp800-63b.html, §4.2.1.1, 2026-09-16, tag-stripped)

- "At enrollment, a CSP that supports this recovery option SHOULD issue a recovery code to the subscriber. The recovery code SHALL include at least 64 bits from an approved random bit generator. ... Saved recovery codes SHALL be stored in the subscriber account in hashed form using an approved one-way function, as described in Sec. 3.1.1.2 . Following the use of a saved recovery code, the CSP SHALL invalidate that recovery code and SHALL issue a new saved recovery code to the subscriber."

There is no crate for this and none is needed — `rand`, `argon2` or `sha2`, `subtle` and `base64`
are the whole toolkit, and the feature is perhaps 150 lines plus tests. §9 records an unresolved
tension between §4.2.1.1 and §3.1.2.2 about whether a ≥112-bit code may use a plain approved hash;
the recommendation that satisfies either reading is **≥128-bit codes hashed with Argon2id**.
**Note the consequence: `argon2` enters the tree even on the passkey branch.** It is not a
password-only dependency.

Prevention comes before recovery, and both NIST and the crate's own docs treat multi-credential
enrollment as the relying party's job, so the rule is: **the first enrollment enrolls two
credentials before it is complete**, or the operator is nagged until a second exists.

### 4.11 What a deployment does when the only operator loses their only credential

Three layers, and the third is the honest one: a CLI subcommand on the host that mints a one-time
enrollment URL to stdout. That is consistent with the design's own trust model — the boundary is
the network, and whoever holds host, environment and Doppler access already controls the
deployment — and it is the same bootstrap shape decision 4 already uses for the allowlist. It must
be journalled as loudly as an approval is, with its own `AuthFailedReason`/event, never silently.
What a deployment must **not** do is add a fourth layer that reintroduces a static shared secret,
which trust boundary 5 forbids.

### 4.12 The structural consequence nobody can skip: durable identity state

Milestone 2c's session and pending-login stores are in-memory by design and lost on redeploy. **A
`Passkey` record cannot be.** It is the first durable identity state willikins would own, and the
journal is append-only and is not a credential store. The Railway volume is the obvious home, and
that makes the credential file a backup-and-restore concern the project has not had.

Second consequence: with an in-house issuer the `sub` is a Uuid **willikins itself generates at
enrollment**, not a provider-generated identifier the operator reads off a 403 page. So
`WILLIKINS_APPROVER_SUBJECTS` either becomes redundant — the enrolled-credential store *is* the
allowlist — or the enrollment CLI must print the Uuid for the operator to paste into the variable.
Both are defensible; the plan must pick one, and either way the environment-variable table changes.


## 5. Sessions, cookies and CSRF, now that willikins runs the login

The plan's existing decision — a `__Host-`prefixed signed cookie over a bounded in-memory store,
a login-binding cookie, a per-plan nonce — is **right and mostly survives the pivot**. What
changes is that three of its properties are now unjustified by their own stated reasons, and three
requirements appear that delegated OAuth did not have.

Both OWASP cheat sheets were fetched from `master` and then pinned by resolving the last commit
touching each file; cite the sha-pinned URLs given below, never the `master` one.

### 5.1 `tower-sessions`: right conclusion, wrong reason, four fetched replacements

The existing note's reason is false — `memory-store` is a **default** feature and the store is 60
lines of `Arc<Mutex<HashMap>>` with no Redis or Postgres in sight.
(source: https://static.crates.io/crates/tower-sessions/tower-sessions-0.15.0.crate, `Cargo.toml`,
2026-09-16)

- "[features]\naxum-core = [\"tower-sessions-core/axum-core\"]\ndefault = [\n    \"axum-core\",\n    \"memory-store\",\n]\nmemory-store = [\"tower-sessions-memory-store\"]"

The four real reasons:

**(a) The store is unbounded and self-documented as unfit.** `save` inserts unconditionally,
`load` filters expired records on read without removing them, and nothing prunes; expired records
are only deleted by the opt-in `ExpiredDeletion` machinery. Adopting it would silently undo
decision 4's 1,024-entry cap and its 503-when-full behaviour — the exact memory-exhaustion path
the plan closes, on an endpoint any anonymous caller can reach.
(source:
https://static.crates.io/crates/tower-sessions-memory-store/tower-sessions-memory-store-0.15.0.crate,
`src/lib.rs`, 2026-09-16)

- "/// A session store that lives only in memory.\n///\n/// This is useful for testing but not recommended for real applications.\n...\npub struct MemoryStore(Arc<Mutex<HashMap<Id, Record>>>);\n\n#[async_trait]\nimpl SessionStore for MemoryStore {\n    async fn create(&self, record: &mut Record) -> session_store::Result<()> {\n        let mut store_guard = self.0.lock().await;\n        while store_guard.contains_key(&record.id) {\n            // Session ID collision mitigation.\n            record.id = Id::default();\n        }"

**(b) `SessionStore::create` has no way to refuse**, so a full store must evict or return an error
that surfaces as a 500, not decision 4's chosen 503.

**(c) `Expiry` offers idle *or* absolute, never both** — and its idle variant counts
*modifications*, not reads, so an operator sitting on the approvals page reading pending plans
would expire under active use.
(source: https://static.crates.io/crates/tower-sessions-core/tower-sessions-core-0.15.0.crate,
`src/session.rs`, 2026-09-16)

- "pub enum Expiry {\n    /// Expire on [current session end][current-session-end], as defined by the\n    /// browser.\n    OnSessionEnd,\n\n    /// Expire on inactivity.\n    ///\n    /// Reading a session is not considered activity for expiration purposes.\n    /// [`Session`] expiration is computed from the last time the session was\n    /// _modified_.\n    OnInactivity(Duration),"

**(d) It routes cookies through `tower-cookies` 0.11** — a second cookie stack beside the
`axum-extra` `SignedCookieJar` the plan already adopts.
(source: https://static.crates.io/crates/tower-sessions/tower-sessions-0.15.0.crate, `Cargo.toml`,
2026-09-16)

- "[dependencies.tower-cookies]\nversion = \"0.11.0\""

Two things are worth copying without the dependency: its cookie defaults (which the plan already
matches), and its `create`-time ID-collision check. Its session ID is 128 bits from a CSPRNG,
which is the benchmark to match:
(source: same core tarball, `src/session.rs`, 2026-09-16)

- "/// ID type for sessions.\n///\n/// Wraps an array of 16 bytes.\n#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, Hash, PartialEq)]\npub struct Id(pub i128); // TODO: By this being public, it may be possible to override the\n                         // session ID, which is undesirable.\n\nimpl Default for Id {\n    fn default() -> Self {\n        use rand::prelude::*;\n\n        Self(rand::rng().random())\n    }\n}"

### 5.2 `axum-login`: decline the crate, copy two of its ideas

Latest is 0.18.0, **2025-07-20 — fourteen months before this research**, and it pins
`tower-sessions = "0.14.0"` while 0.15.0 is current. Per tower-sessions' own CHANGELOG the skew is
not cosmetic: 0.15.0 carries "Fix memory ordering race. #254" and the rand 0.9 update, so 0.14
lacks both and adds a third `rand` major.
(source: https://static.crates.io/crates/axum-login/axum-login-0.18.0.crate, `Cargo.toml`, and
https://static.crates.io/crates/tower-sessions/tower-sessions-0.15.0.crate, `CHANGELOG.md`,
2026-09-16)

- "[dependencies.tower-sessions]\nversion = \"0.14.0\"\ndefault-features = false\n--- and, from the tower-sessions CHANGELOG ---\n# 0.15.0\n\n- Update rand to v0.9. #238\n- Fix memory ordering race. #254"

Its two assumptions — that every user exposes a stable credential-derived hash, and that a user
directory can be looked up by id on **every** request — fit willikins badly under delegated OAuth.
The irony worth recording: after this pivot they fit *better*, which is precisely why the two
ideas should be copied and the dependency declined.
(source: https://static.crates.io/crates/axum-login/axum-login-0.18.0.crate, `src/backend.rs`,
2026-09-16)

- "    /// Returns a hash that's used by the session to verify the session is\n    /// valid.\n    ///\n    /// For example, if users have passwords, this method might return a\n    /// cryptographically secure hash of that password.\n    fn session_auth_hash(&self) -> &[u8];"

**Idea 1: rotate the session id at login.** Note the wrinkle — it is guarded, so logging in over an
already-authenticated session does **not** rotate. OWASP requires regeneration after *any* privilege
change, so willikins should rotate unconditionally. **Idea 2: bind the session to a hash of the
credential-as-of-login and constant-time compare it on every request**, flushing on mismatch. That
is the mechanism that kills every live session when a credential changes — the revocation the plan
says it does not have.
(source: same tarball, `src/session.rs`, 2026-09-16)

- "        if self.data.auth_hash.is_none() {\n            self.session.cycle_id().await?; // Session-fixation\n                                            // mitigation.\n        }\n...\n            let session_verified = data\n                .auth_hash\n                .as_ref()\n                .is_some_and(|auth_hash| auth_hash.ct_eq(session_auth_hash).into());\n            if !session_verified {\n                user = None;\n                data = Data::default();\n                session.flush().await?;\n            }"

### 5.3 What the pivot makes unjustified

- **`SameSite=Lax` loses its stated reason.** The plan requires Lax because the provider callback
  302s to `/approvals` at the end of a cross-site redirect chain. With an in-house login there is
  no cross-site hop and OWASP prefers `Strict`. **Do not simply flip it.** With an in-house AS,
  `/authorize` is reached by a top-level navigation an MCP client launched, and the consent page
  needs the human's session cookie; whether `Strict` withholds it on an externally-initiated
  navigation is a browser fact nobody fetched (§9.2). Decide it deliberately and in writing, because
  it is also the one cookie attribute that leaks through the adapter seam (§7).
- **The `__Host-willikins-login` cookie plus a 1,024-entry server-side pending-login store is one
  mechanism too many — *if* the login is a form.** A stateless signed pre-auth cookie echoed as a
  hidden field is OWASP's pre-session-plus-token remedy with one fewer anonymously-fillable store.
  **But §4.3 shows a passkey ceremony *requires* the server-side store.** So this is conditional on
  §4's choice, not a free deletion.

### 5.4 What the pivot makes newly required

**(a) Session fixation is a new surface.** Under OAuth the session was born fresh at the callback.
A login that sets any pre-authentication cookie creates the classic target.
(source:
https://raw.githubusercontent.com/OWASP/CheatSheetSeries/7deb20b3217026015921ee88293bb7384b30247d/cheatsheets/Session_Management_Cheat_Sheet.md,
2026-09-16)

- "The session ID must be renewed or regenerated by the web application after any privilege level change within the associated user session. The most common scenario where the session ID regeneration is mandatory is during the authentication process ... the old or previous session ID must be destroyed."
- "A complementary recommendation is to use a different session ID or token name (or set of session IDs) pre and post authentication"
- "Web applications should never accept a session ID they have never generated, and in case of receiving one, they should generate and offer the user a new valid session ID. Additionally, this scenario should be detected as a suspicious activity and an alert should be generated."

**(b) There is no idle timeout.** `WILLIKINS_SESSION_TTL_SECONDS` (default 3600) is absolute only,
so an unattended browser can approve for a full hour. OWASP requires **both**, and a page whose one
action is approving a plan that provisions secrets is a high-value application by its own framing.
(source: same sha-pinned file, 2026-09-16)

- "All sessions should implement an idle or inactivity timeout.\n...\nAll sessions should implement an absolute timeout, regardless of session activity.\n...\nCommon idle timeouts ranges are 2-5 minutes for high-value applications and 15-30 minutes for low risk applications."

Split into two variables; bump `last_seen_at` on **reads** as well as writes — the mistake
tower-sessions' `OnInactivity` makes; keep the existing "capped below the approval window" rule on
the absolute one.

**(c) `Cache-Control: no-store`, not `no-cache`.** This is also the concrete answer to the caching
gap the existing note already flagged from the Railway/Cloudflare side.
(source: same sha-pinned file, 2026-09-16)

- "Session identifiers must never be cached. To prevent this, it is highly recommended to include the `Cache-Control: no-store` directive in responses containing session IDs. Unlike `no-cache`, which allows caching but requires revalidation, `no-store` ensures that the response (including headers like `Set-Cookie`) is never stored in any cache."

**(d) Say the cookie is non-persistent, and pin the ID entropy.** The plan says "an opaque session
id" and names no size; pin 128 bits from a CSPRNG. OWASP wants no `Max-Age`/`Expires` on a session
cookie, and the plan never says which.
(source: same sha-pinned file, 2026-09-16)

- "It is recommended to use the session ID created by your language or framework. If you need to create your own sessionID, use a cryptographically secure pseudorandom number generator (CSPRNG) with a size of at least 128 bits and ensure that each sessionID is unique."
- "it is highly recommended to use non-persistent cookies for session management purposes"
- "- `__Host-` — the cookie must be set with `Secure`, must not have a `Domain` attribute, and must use `Path=/`. Prevents subdomain forgery and HTTPS downgrade attacks. **Recommended for session IDs.**"

### 5.5 The per-plan nonce is load-bearing, not defence in depth

The plan calls the nonce "defence in depth here, not the only defence". That understates it. OWASP
lists five ways SameSite fails, and one applies to this deployment by construction: SameSite is
scoped to the **registrable domain**, so every other host under the same parent domain is
same-site to `willikins.bandeabonnot.com`.
(source:
https://raw.githubusercontent.com/OWASP/CheatSheetSeries/be333201dc8bbf9380327dd755c1deff1525f9b3/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.md,
2026-09-16)

- "- **`Lax` only blocks unsafe methods.** ... If any state-changing operation in the application is reachable via a `GET` request, `SameSite=Lax` will not stop it. This is the single most common way `SameSite`-based defenses fail in practice.\n- **`SameSite` is scoped to the registrable domain, not the origin.** A cookie set on `app.example.com` with any `SameSite` value is still considered \"same-site\" when the request originates from `anything.example.com`."

The pattern the plan chose is also the one OWASP selects for stateful software, and it adds a
binding requirement the plan should state: the nonce must be bound to the **session**, not merely
to the plan, and must never reach the journal or a log line.
(source: same sha-pinned file, 2026-09-16)

- "- **Stateful software should use the [synchronizer token pattern](#synchronizer-token-pattern)**\n...\nA CSRF token should not be transmitted in a cookie for synchronized patterns. A CSRF token must not be leaked in the server logs or in the URL.\n...\nAlways bind the CSRF token explicitly to session-specific data."

It also validates the login-cookie binding decision 4 already makes, and adds the fixation rule to
it:
(source: same sha-pinned file, 2026-09-16)

- "Login CSRF can be mitigated by creating pre-sessions (sessions before a user is authenticated) and including tokens in login form. ... Remember that pre-sessions cannot be transitioned to real sessions once the user is authenticated - the session should be destroyed and a new one should be made to avoid session fixation attacks."

### 5.6 Two additions the plan predates

**`Sec-Fetch-Site`.** OWASP now lists Fetch Metadata as a first-class CSRF defence, >98% coverage
since March 2023, with a fallback to Origin/Referer as a **mandatory** requirement — which
willikins already has. So it is purely additive: reject non-safe methods when
`Sec-Fetch-Site: cross-site`, add `Vary: Sec-Fetch-Site, Origin`.
(source: same sha-pinned CSRF file, 2026-09-16)

- "Because some legacy browsers do not send `Sec-Fetch-*` headers, a fallback to standard origin verification headers **is a mandatory requirement** for any Fetch Metadata implementation. `Sec-Fetch-*` is supported in all major browsers since March 2023.\n...\n   1.1. Treat cross-site as untrusted for state-changing actions. By default, reject non-safe methods (POST / PUT / PATCH / DELETE) when `Sec-Fetch-Site: cross-site`.\n...\n- Include an appropriate `Vary` header ... For example, `Vary: Sec-Fetch-Site, Origin`."

**Two details for the existing Origin check**, which task 9b already touches to make IPv6-aware:
match through the trailing `/` so `willikins.bandeabonnot.com.attacker.com` cannot pass, and block
when neither header is present.
(source: same sha-pinned CSRF file, 2026-09-16)

- "make sure the target origin check is strong. For example, if your site is `example.org` make sure `example.org.attacker.com` does not pass your origin check (i.e, match through the trailing / after the origin to make sure you are matching against the entire origin).\n\nIf neither of these headers are present, you can either accept or block the request. We recommend **blocking**."

### 5.7 The signed jar, its footgun, and what the `Key` costs

Signed is the right pick — the cookie carries only an opaque id, and encrypting it would add
`aes-gcm` for nothing.
(source: https://raw.githubusercontent.com/tokio-rs/axum/axum-extra-v0.12.6/axum-extra/src/extract/cookie/signed.rs,
2026-09-16)

- "/// All cookies will be signed and verified with a [`Key`]. Do not use this to store private data\n/// as the values are still transmitted in plaintext."
- "/// Note that methods like [`SignedCookieJar::add`], [`SignedCookieJar::remove`], etc updates the\n/// [`SignedCookieJar`] and returns it. This value _must_ be returned from the handler as part of\n/// the response for the changes to be propagated."

That footgun fails **silently** in the two places that matter most — the handler that sets the
session cookie and `POST /approvals/logout`, where a dropped jar leaves a stale cookie in the
browser and no test that only checks the store would notice. The acceptance tests must assert the
`Set-Cookie` **header**, not the store's contents.

The `Key` is 512 bits, panics on anything shorter with `from`, and the upstream doc example carries
a caution against regenerating it per start.
(source: https://static.crates.io/crates/cookie/cookie-0.18.2.crate, `src/secure/key.rs`, and
https://raw.githubusercontent.com/tokio-rs/axum/axum-extra-v0.12.6/axum-extra/src/extract/cookie/private.rs,
2026-09-16)

- "    /// The supplied key must be at least 512-bits (64 bytes). For security, the\n    /// master key _must_ be cryptographically random.\n    ///\n    /// # Panics\n    ///\n    /// Panics if `key` is less than 64 bytes in length."
- "///     // Generate a secure key\n///     //\n///     // You probably don't wanna generate a new one each time the app starts though"

Generating it at startup is a defensible single-instance answer, but it has a consequence the plan
does not state: **a second replica has a different `Key`, so a session minted on replica A is a
forgery on replica B.** Railway can scale a service. Say out loud that `/approvals` assumes exactly
one instance, or make the `Key` a configured secret.

Build impact: the `cookie` crate's `signed` feature is pure-Rust RustCrypto and its only
build-dependency is `version_check`, so it builds in the current image unchanged. The cost to
record is two more duplicate majors — `base64` 0.22 beside the tree's 0.23.1, `rand` 0.8 beside
0.9.5 and 0.10.2.
(source: https://static.crates.io/crates/cookie/cookie-0.18.2.crate, `Cargo.toml`, 2026-09-16)

- "signed = [\n    \"hmac\",\n    \"sha2\",\n    \"base64\",\n    \"rand\",\n    \"subtle\",\n]"

### 5.8 Smaller, and worth a sentence each in the plan

- **Logout has no nonce**, so its only defences are SameSite and the Origin check. A forced logout
  is a nuisance, not a compromise — but either give it a token or accept it in writing. OWASP also
  specifies the clearing form: empty value, past expiry.
- **Audit every `GET` handler on `/approvals` for state changes.** OWASP calls a state-changing GET
  the most common way SameSite defences fail. The plan says approvals are POST-only; this note did
  not read the handlers, so it is an acceptance-test item.


## 6. Prior art, and what bit the people who shipped it

Read as prior art, depended on by nothing. Two of these are Rust identity servers other people
self-host; three are MCP reference implementations. The most useful thing in this section is not
the architecture — it is the list of checks that were forgotten, by people who knew better.

### 6.1 rauthy is the shape the operator's constraint asks for

Apache-2.0, single licence, no edition split, funded by NLnet/NGI Zero Core rather than a SaaS
parent. It is readable as prior art without inheriting a vendor — which is exactly the point.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/Cargo.toml,
2026-09-16)

- "license = \"Apache-2.0\"\ndescription = \"Single Sign-On Identity & Access Management via OpenID Connect, OAuth 2, and PAM\""

### 6.2 Its size is the honest estimate, once you split it

**84,583 lines of Rust across 13 crates, 777 packages in `Cargo.lock`, MSRV 1.95** (above
willikins' 1.88 floor). But the split is the useful number: the whole `oidc` subtree of
`src/service` — authorize, five grant types, validation, token_set, userinfo, logout, backchannel
logout, revocation, token info — is **5,662 lines**, of which the authorization-code grant is
**209**. Add `src/jwt` (1,079) and `src/middlewares` (605) and the protocol machinery is roughly
**7,300 lines**. The other ~77,000 are users, passwords, passkeys, magic links, MFA, SCIM, PAM,
events, i18n, admin UI and the data layer.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/src/service/src/oidc/grant_types/authorization_code.rs,
2026-09-16)

- "pub async fn grant_type_authorization_code(\n    req: HttpRequest,\n    req_data: TokenRequest,\n) -> Result<(TokenSet, Vec<(HeaderName, HeaderValue)>), ErrorResponse> {"

**The 84k number is not the number. The ~7,300 one is** — and §4 is where the other 77,000 went.

### 6.3 It pays for its own JWT crate with a C toolchain

`rauthy-jwt`, the crate that mints and validates tokens, depends directly on `openssl` and
`openssl-sys`; the lock additionally carries `aws-lc-rs`, two `aws-lc-sys` entries, `cmake`,
`ring`, `rsa` and `josekit`. This is the concrete warning for §3: rauthy wrote its own 1,079-line
JWT crate instead of using `jsonwebtoken`, and paid for it with the three native dependencies the
Dockerfile does not have.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/src/jwt/Cargo.toml,
2026-09-16)

- "chrono = { workspace = true }\nopenssl = { workspace = true }\nopenssl-sys = { workspace = true }\nserde = { workspace = true }"

### 6.4 The issuance checks that a from-scratch implementation must not forget

Each is a fetched line of a shipped server, and each is cheap to write and silent when missing.

**The private key is encrypted at rest with a redacted `Debug`** — the same discipline willikins
already applies to `Credential`/`Value`, applied to a signing key. It appears the moment the server
holds a key at all, which trust boundary 1 today says it does not.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/src/data/src/entity/jwk.rs,
2026-09-16)

- "/**\nThe Json Web Keys are saved encrypted inside the database. The encryption is the same as for a\nClient secret -> *ChaCha20Poly1305*\n */"

**The authorization code is claimed by an atomic get-and-remove, not a find then a delete** — a
find-then-delete leaves a window in which two concurrent token requests redeem the same code.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/src/data/src/entity/auth_codes.rs,
2026-09-16)

- "    // Claims an Authorization code from the cache\n    pub async fn find(id: String) -> Result<Option<Self>, ErrorResponse> {\n        Ok(DB::hql().get_remove(Cache::AuthCode, id).await?)\n    }"

**Its own expiry is checked even though the cache already has a TTL** — belt and braces against
clock skew, restore-from-backup, or a cache serving a stale entry.
(source: same repo,
`src/service/src/oidc/grant_types/authorization_code.rs`, 2026-09-16)

- "    if code.exp < Utc::now().timestamp() {\n        warn!(\"The Authorization Code has expired\");\n        return Err(ErrorResponse::new(\n            ErrorResponseType::SessionExpired,\n            \"The Authorization Code has expired\",\n        ));\n    }"

**Client secrets are compared in constant time, after a fixed-length precondition, then zeroized**
— and every one of those four steps is there because an auditor took the earlier version apart
(§6.6).
(source: same repo, `src/data/src/entity/clients.rs`, 2026-09-16)

- "        let a = <&[u8; 64]>::try_from(cleartext.as_ref()).unwrap();\n        let b = <&[u8; 64]>::try_from(secret.as_bytes()).unwrap();\n        if constant_time_eq::constant_time_eq_64(a, b)\n...\n            secret.zeroize();\n            return Ok(());\n        }"

Kanidm reaches the same answer independently, via a `CtSecret` newtype whose only comparison
method is `ct_eq`. Two Rust authorization servers converging, one of them post-audit, makes this
settled rather than rediscovered.
(source: https://github.com/kanidm/kanidm/blob/v1.11.2/server/lib/src/idm/oauth2.rs, 2026-09-16)

- "    fn ct_eq(&self, rhs: &str) -> bool {\n        self.inner.as_bytes().ct_eq(rhs.as_bytes()).unwrap_u8() == 1"

**The `resource` chosen at `/authorize` is carried in the code and may only be *matched* at the
token request — never widened or introduced.** The book page names MCP as the motivating case.
(source: same repo,
`src/service/src/oidc/grant_types/authorization_code.rs` and `book/src/work/resource_indicators.md`,
2026-09-16)

- "    // RFC 8707: a `resource` on the token request may only narrow (here: match) the\n    // resource granted at the authorization request, never widen or introduce a new one.\n    let resource = match (req_data.resource.as_deref(), code.resource.as_deref()) {\n        (Some(requested), Some(granted)) if requested == granted => Some(granted.to_string()),"
- "This is especially useful when Rauthy acts as the authorization server for multiple resource\nservers, for instance a fleet of MCP servers."

**`aud` is a single string for one audience and a JSON array for two or more**, both valid per RFC
7519. Directly relevant to plan decision 1 step 5: a validator that accepts only one shape will
reject legitimate tokens.

**PKCE is per-client and `plain` is still honoured** — which §1.5 shows draft-16 does not define at
all. rauthy's own default for a new client is S256, but the code path survives. The contrast is the
MCP TypeScript SDK, which types `plain` out of existence.
(source: same repo, `src/service/src/oidc/validation.rs`, 2026-09-16)

- "            // 'plain' is the default method to be assumed by the OAuth specification when it is not\n            // further specified.\n            let method = if let Some(m) = code_challenge_method {\n                m.to_owned()\n            } else {\n                String::from(\"plain\")\n            };"

### 6.5 The code→redirect_uri binding, stated carefully

Three verified facts about rauthy and one norm, and this note deliberately does **not** call it a
vulnerability. (1) The `AuthCode` struct has no `redirect_uri` field. (2) `AuthCode::new` is not
passed one. (3) At the token request, `validate_redirect_uri` checks only that the presented value
is in the client's **registered set**, matched with `wildcard_prefix_match` as well as equality.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/src/data/src/entity/auth_codes.rs,
2026-09-16)

- "pub struct AuthCode {\n    pub id: String,\n    pub exp: i64,\n    pub client_id: String,\n    pub user_id: String,\n    pub session_id: Option<String>,\n    pub challenge: Option<String>,\n    pub challenge_method: Option<String>,\n    pub nonce: Option<String>,\n    pub scopes: Vec<String>,"

The norm this differs from is §1.7's RFC 6749 §4.1.3 ("their values MUST be identical") and
draft-16 §4.1.2 ("The authorization code is bound to the client identifier, code challenge and
redirect URI"), both fetched in this session. The contrast implementation is the MCP Python SDK,
which records `redirect_uri_provided_explicitly` on the code precisely to reproduce the RFC's shape:
(source: https://github.com/modelcontextprotocol/python-sdk/blob/v2.2.0/src/mcp/server/auth/handlers/token.py,
2026-09-16)

- "                # verify redirect_uri doesn't change between /authorize and /tokens\n                # see https://datatracker.ietf.org/doc/html/rfc6749#section-10.6\n                if auth_code.redirect_uri_provided_explicitly:"

### 6.6 What bit people, case 1: a check deleted by a rewrite, missed by an audit

GHSA-7qh2-3hc5-2vqp (2026-08-08, medium, CWE-287/863, >=0.30.0 <0.36.1): the refresh-token grant
accepted a caller-supplied client without checking it equalled the token's `azp`, so a public
client with no secret could redeem a confidential client's refresh token. **It was a regression
introduced by a rewrite** that dropped the equality check while keeping the comment claiming it
happened — and the independent professional audit running during the same window did not find it.
(source: https://github.com/sebadob/rauthy/security/advisories/GHSA-7qh2-3hc5-2vqp, 2026-09-16)

- "The rewrite dropped that equality check while keeping the reassuring comment. First affected release: **v0.30.0**. Still present on `main`."
- "Architecturally the `azp` claim is the *only* record of the issuing client: the stored `RefreshToken` row has no client_id column"

The storage lesson is the transferable one: **put the binding in the store, not only in a claim**,
so a rewrite cannot silently remove the last copy of it. This is the single best argument for
willikins' own convention — a negative fixture per case, each naming its acceptance test and the
exact error it must produce — applied to every binding (code→client, code→redirect_uri,
code→resource, token→audience).

### 6.7 What bit people, cases 2 and 3

CVE-2026-69199 / GHSA-x8jp-v2j6-6vjf (<0.36.0): WebAuthn authentication state stored under a code
but **not bound to a user id**, with `auth_finish` taking the target user from a separate argument
— so an attacker could sign a challenge for their own account and finish against a victim's. This
is the same class as the approvals login-cookie fix in decision 4, and a direct warning for §4.
(source: https://github.com/sebadob/rauthy/security/advisories/GHSA-x8jp-v2j6-6vjf, 2026-09-16)

- "WebauthnData stores a code and authentication state, but not the user id. auth_finish loads state by code and the target user from the user_id argument."

Issue #1645 (closed) is the most on-point bug for an MCP deployment: **the RFC 8707 `resource` was
dropped on a second, less-travelled authorize path**, breaking audience binding for returning
users. Audience binding must be re-verified on *every* path that mints a token, not only the one
the tests drive.
(source: https://api.github.com/repos/sebadob/rauthy/issues?state=all&labels=bug, 2026-09-16)

- "1645 closed RFC 8707: `resource` is dropped on /oidc/authorize/refresh (LoginRefreshRequest has no resource field) — audience binding breaks for returning users"

And an interoperability trap whose code comment names the client that found it: because rauthy's
issuer carries a path, RFC 8414 §3.1 requires the metadata URL be formed by **inserting** the
well-known segment between host and path. Without three routes for one document the request fell
through to the SPA catch-all and the client silently fell back to DCR. **willikins' resource
identifier is `<WILLIKINS_PUBLIC_URL>/mcp`** — an identifier with a path component — and the same
rule governs RFC 9728 metadata.
(source:
https://github.com/sebadob/rauthy/blob/ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4/src/api/src/oidc.rs,
2026-09-16)

- "// RFC 8414 §3.1 path-insertion alias for the AS-metadata document. Because\n// rauthy's issuer carries a path component (`https://<host>/auth/v1/`), an\n// RFC-compliant client forms the metadata URL by INSERTING\n// `/.well-known/oauth-authorization-server` between host and issuer path ... claude.ai\n// probes exactly this path-insertion form; without it the request falls through\n// to the SPA catch-all (301 -> HTML), so the client never reads\n// `client_id_metadata_document_supported` and falls back to Dynamic Client\n// Registration."

That comment is also the only **evidence in this whole note that a real MCP client probes for
CIMD**, which bears directly on §2.7's registration choice.

### 6.8 What an independent audit of a Rust identity provider actually found

Radically Open Security audited rauthy v0.32 (2025-07-14 to 2025-08-22, NGI Zero Core funded): **1
Elevated, 3 Low, 3 N/A**, and **none of them in the OAuth grant logic**. RAUTHY-007 stored XSS via
an SVG profile image; RAUTHY-005 a non-constant-time client-secret comparison; RAUTHY-006 a
reachable `unwrap()` panicking when a request was cancelled mid-await; RAUTHY-009 a private key
committed to the repository. Two of the four are **Rust-shaped, not protocol-shaped** — a timing
oracle and a reachable unwrap — which is a useful calibration for where the real risk sits.
(source:
https://raw.githubusercontent.com/sebadob/rauthy/refs/heads/main/assets/security_audit_report_v0.32.pdf,
2026-09-16)

- "RAUTHY-005\nLow\nType: Timing oracle\nStatus: resolved\nClient secrets are compared with a non-constant time string comparison, theoretically\nenabling timing attacks to learn the client secret."
- "rauthy explicitly disables unsafe code at the module level. The only unsafe blocks are for reading environment variables;\nit is good to see such minimal use of unsafe."

### 6.9 Kanidm: the minimum profile, the escape hatches, and the test ratio

Its **entire** OAuth2 implementation is one file: 8,769 lines with `mod tests` beginning at line
3,602 — roughly **3,600 implementation lines against 5,160 test lines, a 1.4:1 ratio**. That ratio
is the most transferable number here: it is what a mature project spends to keep an authorization
server correct.

Its required profile is the clearest statement of a minimum in the corpus: one grant, mandatory
PKCE S256, one signing algorithm, client authentication always.
(source: https://github.com/kanidm/kanidm/blob/v1.11.2/book/src/integrations/oauth2.md, 2026-09-16)

- "In general, Kanidm **requires** that your service supports three things:\n\n- HTTP basic authentication to the authorisation server (Kanidm)\n\n- PKCE `S256` code verification (`code_challenge_methods_supported`)\n\n- If it uses OIDC, `ES256` for token signatures (`id_token_signing_alg_values_supported`)"

And it is the honest data point about what a self-hosted authorization server gets **asked for**.
It added escape hatches, and put the warning in the command name — a cheap convention willikins
could reuse for any future `danger_` configuration. Two limits survived the pressure: PKCE cannot
be disabled for public clients at all, and localhost redirects are enabled only where PKCE is
enforced.
(source: same page, 2026-09-16)

- "To disable PKCE for a confidential client:\n\n```bash\nkanidm system oauth2 warning-insecure-client-disable-pkce <client name>\n```"

One invariant it refuses to bend, relevant to milestone 3's credential routing across several
GitHub organizations and Doppler workplaces:
(source: same page, 2026-09-16)

- "Sharing OAuth2 client configurations between applications **FUNDAMENTALLY BREAKS** the OAuth2 security model and is\n**NOT SUPPORTED** as a configuration. The Kanidm Project **WILL NOT** support you if you attempt this."

### 6.10 The MCP reference implementations: none of them writes the login

The TypeScript SDK hands the raw response object back to the application; Cloudflare requires an
application-owned `authorizeEndpoint`; Python's `authorize()` is a protocol method the application
implements. **All three implement the OAuth protocol surface and declare the login itself out of
scope.** That is the split §6.2's estimate turns on.
(source: https://github.com/modelcontextprotocol/typescript-sdk/blob/v1.29.0/src/server/auth/provider.ts,
2026-09-16)

- "    /**\n     * Begins the authorization flow, which can either be implemented by this server itself or via redirection to a separate authorization server.\n     *\n     * This server must eventually issue a redirect with an authorization response or an error response to the given redirect URI."

Cloudflare's `workers-oauth-provider` (MIT, 1,870 stars) is the size datum for a **complete** MCP
authorization server with nothing delegated: `src/oauth-provider.ts` is **5,967 lines**,
implementing DCR, CIMD, PKCE, refresh, revocation, RFC 8693 token exchange, RFC 9728 metadata and
per-resource token binding — everything except the human login. Roughly 6,000 lines in a language
with no type-level help; kanidm's ~3,600 implementation lines is the Rust comparison.

Its storage rule is the most portable single sentence in the corpus:
(source: https://github.com/cloudflare/workers-oauth-provider/blob/v0.10.3/README.md, 2026-09-16)

- "Sensitive values are not stored in plaintext:\n\n- Access tokens, refresh tokens, authorization codes, and client secrets are stored only by hash.\n- `props` are encrypted with AES-GCM using key material wrapped by the corresponding secret token.\n- Grant `userId` and `metadata` are not encrypted because applications use them to enumerate and revoke grants."

Its audience rule is directly usable in plan decision 1 step 5, which today compares the audience
as a plain string:
(source: same README, 2026-09-16)

- "ASCII case differences in the URI scheme and host are accepted, but port, path, query, trailing slash, and array cardinality remain strict. The authorization server always stores and returns the configured lowercase scheme-and-host spelling."

And its DCR defaults are the shape of what hosting registration costs: off by default, a 90-day
TTL on dynamically registered clients, a `disallowPublicClientRegistration` switch, and a
documented manual sweep for orphaned records.
(source: same README, 2026-09-16)

- "- `clientRegistrationTTL` controls the lifetime of dynamically registered clients. The default is 90 days."

### 6.11 Three rules from the SDKs that a hand-rolled `/authorize` gets wrong

**Errors split into two phases and a phase-1 error may never be redirected** — redirecting one is an
open redirect.
(source: https://github.com/modelcontextprotocol/typescript-sdk/blob/v1.29.0/src/server/auth/handlers/authorize.ts,
2026-09-16)

- "        // In the authorization flow, errors are split into two categories:\n        // 1. Pre-redirect errors (direct response with 400)\n        // 2. Post-redirect errors (redirect with error parameters)\n\n        // Phase 1: Validate client_id and redirect_uri. Any errors here must be direct responses."

**Make the weak modes unrepresentable rather than branched on** — the cheap version of the same
discipline willikins already applies with domain newtypes:
(source: same file, 2026-09-16)

- "const RequestAuthorizationParamsSchema = z.object({\n    response_type: z.literal('code'),\n    code_challenge: z.string(),\n    code_challenge_method: z.literal('S256'),"

**The loopback rule, from someone who had to get it right** — and note the clause a hand-rolled
matcher invents wrongly:
(source: same file, 2026-09-16)

- "    // RFC 8252 relaxes the port only — scheme, host, path, and query must\n    // still match exactly. Note: hostname must match exactly too (the RFC\n    // does not allow localhost↔127.0.0.1 cross-matching)."

Two more worth copying: `/authorize` and `/token` carry **default** rate limits (100 and 50 per 15
minutes) and set `Cache-Control: no-store` before anything else — the same argument the plan
already makes for its per-reason token bucket, applied to endpoints any anonymous caller can reach;
and a code or refresh token presented by the wrong client is answered `invalid_grant` with
"authorization code does not exist", so a wrong-client redemption is indistinguishable from an
unknown code. A refresh may only ever **shrink** what a token can do:
(source: https://github.com/modelcontextprotocol/python-sdk/blob/v2.2.0/src/mcp/server/auth/handlers/token.py,
2026-09-16)

- "                for scope in scopes:\n                    if scope not in refresh_token.scopes:\n                        return self.response(\n                            TokenErrorResponse(\n                                error=\"invalid_scope\","

### 6.12 The reference implementation is not evidence a check is safe

Worth recording precisely because it is the reference: the TypeScript SDK compares client secrets
with a plain JavaScript `!==`, and checks expiry **after** the comparison rather than before.
rauthy's auditor filed exactly this shape as RAUTHY-005; kanidm uses `subtle`.
(source: https://github.com/modelcontextprotocol/typescript-sdk/blob/v1.29.0/src/server/auth/middleware/clientAuth.ts,
2026-09-16)

- "                if (client.client_secret !== client_secret) {\n                    throw new InvalidClientError('Invalid client_secret');\n                }\n                if (client.client_secret_expires_at && client.client_secret_expires_at < Math.floor(Date.now() / 1000)) {\n                    throw new InvalidClientError('Client secret has expired');\n                }"

### 6.13 Two things the Python SDK settles that willikins already half-decided

Its principal is the same triple plan decision 2 derives, with the same reasoning about `sub` being
unique only per issuer — independent convergence:
(source: https://github.com/modelcontextprotocol/python-sdk/blob/v2.2.0/src/mcp/server/auth/provider.py,
2026-09-16)

- "def principal_components(token: AccessToken) -> tuple[str, str | None, str | None]:\n    \"\"\"The (client_id, issuer, subject) triple identifying the principal a token represents.\n\n    The single source for \"who is this token's principal\": session ownership and\n    request-state binding both build on it."

And the TypeScript SDK's bearer middleware answers **401 for a missing or non-`Bearer`
`Authorization` header**, not 400, and refuses a token whose `exp` is not a number — matching
decision 1's `required_spec_claims` choice. That is a data point for the plan's verify item 13,
though it is an SDK's behaviour and not RFC 6750 text.
(source: https://github.com/modelcontextprotocol/typescript-sdk/blob/v1.29.0/src/server/auth/middleware/bearerAuth.ts,
2026-09-16)

- "            // Check if the token is set to expire or if it is expired\n            if (typeof authInfo.expiresAt !== 'number' || isNaN(authInfo.expiresAt)) {\n                throw new InvalidTokenError('Token has no expiration time');\n            } else if (authInfo.expiresAt < Date.now() / 1000) {\n                throw new InvalidTokenError('Token has expired');\n            }"


## 7. The seam, stated once

The operator banked pluggable auth as a later improvement. The brief's own observation is correct
and is the foundation: **the resource-server half is the natural seam, because a token is
validated against an issuer and a key set whoever minted it.** Five readers found five layers of
that seam; they do not conflict, they nest. Here is the single statement.

**The seam is three things, in order of how much code they cost.**

1. **A data seam, costing nothing, and it already exists.** It is the issuer URL, the key-set
   location, and the `authorization_servers[]` array of the RFC 9728 document — the configuration
   block plan decision 10 already isolates. With the in-house AS the issuer is willikins' own
   origin; an adapter fills the same fields with someone else's. RFC 9068 §4 confirms this is the
   mechanism the specification intends, not an invention:
   (source: https://www.rfc-editor.org/rfc/rfc9068.txt, §4, 2026-09-16)
   - "   Authorization servers SHOULD use OAuth 2.0 Authorization Server Metadata\n   [RFC8414] to advertise to resource servers their signing\n   keys via \"jwks_uri\" and what \"iss\" claim value to expect via the\n   \"issuer\" metadata value."

2. **A code seam that is one method wide, and it must not be short-circuited.** Define a token
   verifier with a single operation — validate a bearer credential, return
   `(subject, issuer, client_id, scopes, expiry)` — and build `/mcp`, the approvals session and
   everything downstream against only that. The plan already has the discipline: the browser
   login's token runs through `oauth::validate`, "the same function and the same configuration the
   `/mcp` middleware uses: one validation path, not two". Keep exactly that shape by having the
   in-house login mint a token and validate it through the **same** function, so the session layer
   never learns who issued.
   This is not a hopeful design; it is a shipped one. The MCP TypeScript SDK splits at precisely
   this line:
   (source: https://github.com/modelcontextprotocol/typescript-sdk/blob/v1.29.0/src/server/auth/provider.ts,
   2026-09-16)
   - "/**\n * Slim implementation useful for token verification\n */\nexport interface OAuthTokenVerifier {\n    /**\n     * Verifies an access token and returns information about it.\n     */\n    verifyAccessToken(token: string): Promise<AuthInfo>;\n}"

   **Three rules keep that seam real.** First, *publish a `jwks_uri` even though RFC 8414 makes it
   OPTIONAL and a co-hosted AS could verify in-process from a shared key*: the day the issuer
   changes, the RS half must read a key set from a stranger, and a resource server that only ever
   knew how to read a key out of its own process is the rewrite. Second, *never let the RS half
   skip validation because it minted the token* — the fixed-order RFC 9068 checks must run
   identically for an in-house and an external token, which they already do. Third — the one the
   readers disagreed about until it was reconciled — *abstract the `JwkSet` **source**, not the
   fetch*: when issuer and validator share a process the RS must **not** `ureq`-fetch its own
   `jwks_uri` over the loopback, so the in-house issuer supplies the set in-process and an adapter
   supplies it through the fetch-and-cache the plan already specifies. The overlap window differs
   accordingly: in-process it is exactly the maximum token lifetime (§3.7); with an external IdP the
   JWKS cache TTL adds to it.

3. **One genuinely new trait: authenticate a human, return a validated subject and granted
   scopes.** Everything in (1) and (2) is already provider-agnostic. §4's credential store, ceremony
   and login UI is the only part an external identity provider would replace wholesale, and it is
   the abstraction this milestone introduces. Concretely, the in-house path is: ceremony yields a
   verified `user_unique_id` → the issuer mints an RFC 9068 JWT with that as `sub`, this server's
   derived resource as `aud`, the configured issuer as `iss`, and the granted scope → `oauth::validate`
   accepts it exactly as it would a provider's.

**Two adapter shapes exist, and prior art ships both.** Either swap the issuer — the TS SDK's
`ProxyOAuthServerProvider implements OAuthServerProvider`, forwarding `/authorize`, `/token`,
`/revoke` and `/register` upstream — or keep the in-house AS as the token issuer and add a grant
that trusts an external assertion (§1.13's Enterprise-Managed Authorization). Note that even the
standards-track enterprise story chooses the second.
(source: https://github.com/modelcontextprotocol/typescript-sdk/blob/v1.29.0/src/server/auth/providers/proxyProvider.ts,
2026-09-16)

- "/**\n * Implements an OAuth server that proxies requests to another OAuth server.\n */\nexport class ProxyOAuthServerProvider implements OAuthServerProvider {\n    protected readonly _endpoints: ProxyEndpoints;\n    protected readonly _verifyAccessToken: (token: string) => Promise<AuthInfo>;\n...\n    skipLocalPkceValidation = true;"

**What leaks through the seam, named so it is a design input and not a 3am discovery.**

- **One flag, in the proxy shape:** `skipLocalPkceValidation`, because the upstream server holds
  the code challenge. That is the entire delta in a production SDK, and it should be designed in
  now rather than found later.
- **`SameSite`.** An adapter's cross-site callback landing needs `Lax`; an in-house-only deployment
  could take `Strict` (§5.3). Decide once, in writing, as a seam constraint rather than a cookie
  detail.
- **Client registrations.** Pre-registered and DCR client ids are per-authorization-server and must
  all re-register when the advertised AS changes (§1.2); **CIMD ids port**, because they are
  self-hosted URLs resolved on demand. A willikins that leans on pre-registration bakes its own AS
  into every client's configuration — which is the one real argument for CIMD that is not about
  convenience.
- **Passkeys do not port at all.** They are bound to the RP ID (§4.4), so switching to an external
  IdP orphans every credential and switching back re-enrols. Nothing mitigates this; it is the
  price of the in-house default and it should be written down rather than discovered.

**And the part the seam does not rescue.** Building the issuing side directly does not make the
future adapter harder — a framework would add a second thing to unwind, not a first thing to reuse.
But the credential store, the signing key and their backups are state the adapter cannot absorb.
The seam makes the *protocol* swappable. It does not make the *history* portable.

## 8. Pin table

One row per crate considered. **No cargo command was run**, so every "Dockerfile" cell is
dependency-graph inference from fetched manifests, not a build. Licence, MSRV and date cells come
from crates.io version metadata or the crate's own `Cargo.toml` at the pinned version, fetched
2026-09-16.

| Crate | Version | Licence | Maintenance signals | What it would do here | Risk | Dockerfile builds it? |
| --- | --- | --- | --- | --- | --- | --- |
| `jsonwebtoken` | 11.0.0 (2026-07-24) | MIT | Already the plan's pin; badge says `passively-maintained`; MSRV 1.88 = workspace floor, zero headroom | **Take.** Signs (`encode`), publishes the JWKS (`Jwk::from_encoding_key`), derives `kid` (`thumbprint`), and validates — the whole issuing half for **zero new crates** | Two silent traps (§3.3): `kid` is `None` by default and `JwkSet::find` skips such keys; Ed25519 needs PKCS#8 **v1**. `rust_crypto` drags `rsa` in unconditionally (§3.4) | Yes with `rust_crypto` (pure Rust); `aws_lc_rs` needs `g++` added |
| `oxide-auth` | 0.6.1 (2024-06-02) | MIT OR Apache-2.0 | Last **release** 2 yr 3 mo ago; last commit 2026-01-31; 783 stars; no declared MSRV; the RFC 7591 and JWT issues open since 2022 | The authorization-code + PKCE state machine, ~1,500 wanted lines | **Reject.** No RFC 8414, JWKS, 7591, 8707 or `at+jwt` anywhere (§2.2); `Grant` has no audience; `Issuer`/`Authorizer` return `Result<_,()>`; PKCE opt-in and `plain`-defaulting; redirect matching semantic by default | Yes (pure Rust), but costs `base64` 0.21, `sha2` 0.10, `rand` 0.8 as duplicate majors + `hmac`, `rust-argon2`, `rmp-serde` |
| `oxide-auth-async` | 0.2.1 (2024-06-02) | MIT OR Apache-2.0 | Same repo, same staleness | Async re-declaration of the whole trait set, 4,857 lines | **Reject** with its parent; a boxed future per primitive call | Yes |
| `oxide-auth-axum` | 0.6.0 (2025-01-09) | MIT OR Apache-2.0 | 8.2k recent downloads; no declared MSRV | axum 0.8 adapter, 315 lines | **Reject** with its parent — but note it declares `axum ^0.8`, so the framework branch was **not** disqualified on compatibility | Yes |
| `oauth-as` | 0.9.4 (2026-09-06) | MIT OR Apache-2.0 | **Six weeks old**; 6 releases in a month; 1 author, 1 star, 0 forks, 0 watchers; no audit; MSRV 1.75 | Everything §1 requires: RFC 8414, 7591, CIMD, 8707→`aud`, 9068, PAR, rate limiting, consent, S256-only | **Do not depend yet; read as prior art.** 36,230 lines of unreviewed security-critical source by one author is a larger trust surface than the product the operator rejected. ES256-only signer would freeze the algorithm. Re-evaluate on 1.0, a second maintainer, or an audit | Yes — 4 always-compiled deps, `#![forbid(unsafe_code)]`, no rsa/openssl/cmake |
| `authkestra-op` | 0.11.1 (2026-09-15) | — | 38 versions since 2026-07-24; no declared MSRV | OpenID Provider support | **Reject.** Non-optional deps on three sibling `authkestra-*` crates: adopting it means adopting authkestra | Not evaluated (rejected earlier) |
| `mcp-oauth` | 0.3.0 (2026-04-05) | MIT OR Apache-2.0 | 112 all-time downloads; untouched 5 months | MCP-shaped OAuth | **Reject.** `rust_version: 1.92` > workspace MSRV 1.88 — disqualified before any other consideration | n/a |
| `webauthn-rs` (+ `-core`, `-proto`) | 0.5.5 (2026-04-30) | MPL-2.0 | 6.8M all-time / 2.5M recent; **the only** Rust relying party; MSRV 1.88 = workspace floor; no third-party audit claimed | **Take, if §4's passkey branch is chosen.** Registration and authentication ceremonies, `Passkey` persistence, UV Required, counter-regression detection | `webauthn-rs-core` depends **unconditionally** on `openssl` + `openssl-sys` (§4.2). No Related Origin Requests, so the RP ID can never change. Ceremony state must be server-side | **No, not as-is.** Builder needs `libssl-dev` + `pkg-config`; the "no openssl-sys anywhere" comment becomes false. Runtime probably fine (distroless base carries libssl) — §9 item |
| `webauthn-authenticator-rs` | 0.5.5 | MPL-2.0 | Same repo, same MSRV | **Dev-dependency.** `SoftPasskey` drives the acceptance tests with no browser; `new(falsify_uv)` and its own counter make the UV-lie and counter-regression tests possible | Pulls tokio/hex/serde_bytes into dev-deps only | Dev-only; inherits the openssl requirement already present |
| `passkey-authenticator` | 0.5.0 | MIT OR Apache-2.0 | 1Password; MSRV 1.85.1 | Alternative test authenticator (p256 + coset, pure Rust) | Would keep openssl out of the **dev**-dependency graph but not out of the build, since `webauthn-rs-core` needs it anyway | Yes (pure Rust) |
| `argon2` | 0.6.0 | MIT OR Apache-2.0 | RustCrypto; 53.2M downloads; MSRV 1.85, edition 2024 | **Take on either branch.** Hashes the saved recovery codes (§4.10); hashes passwords only if §4.9 is chosen against the recommendation | `Argon2::default()` is already OWASP's config — pin it with a test rather than trusting the default to stay put. 19 MiB **per concurrent hash** makes an unauthenticated login endpoint a memory-exhaustion surface without a semaphore | Yes (pure Rust) |
| `password-hash` | **0.6.1** | MIT OR Apache-2.0 | RustCrypto; **0.6.0 is yanked** — pin 0.6.1 | PHC string encoding, so a later parameter increase is a per-hash migration | Low | Yes |
| `tower-sessions` | 0.15.0 (2026-02-01) | MIT | Active; 3.62M all-time | Session middleware | **Decline** — for the four reasons in §5.1, *not* the reason the existing note gives. Copy its cookie defaults and its `create`-time ID-collision check | Yes, but adds a second cookie stack (`tower-cookies` 0.11) + `async-trait` + `time` |
| `axum-login` | 0.18.0 (2025-07-20) | MIT | **No release in 14 months**; pins `tower-sessions` 0.14 against a current 0.15 carrying a memory-ordering fix | Login/session glue | **Decline.** Version skew, plus `get_user` on every request presupposes a user directory. Copy two ideas: `cycle_id()` at login and a constant-time `auth_hash` compare for credential-change revocation | Yes |
| `axum-extra` | 0.12.6 (2026-04-14) | MIT | Already the plan's pin | **Take.** `SignedCookieJar` for the approvals session | The jar must be **returned** from the handler or `Set-Cookie` is silently dropped (§5.7) — assert the header in tests. A startup-generated `Key` makes sessions replica-local | Yes; `cookie`'s `signed` is pure RustCrypto, only build-dep is `version_check`. Adds `base64` 0.22 and `rand` 0.8 as duplicate majors |
| `p256` / `p384` / `elliptic-curve` | 0.13.2 / 0.13.0 / 0.13.8 | Apache-2.0 OR MIT | RustCrypto; **already pulled** by `jsonwebtoken`'s `rust_crypto` with defaults on | ES256 key generation → PKCS#8 (`SecretKey::random`, `to_pkcs8_der`/`to_pkcs8_pem`) | Binds `rand_core` **0.6**, which neither of the tree's `rand` majors satisfies — declare `rand = "0.8"` directly (§3.6) | Yes, and needs no feature change |
| `ed25519-dalek` | 2.1.1 | BSD-3-Clause | RustCrypto-adjacent; already pulled by `rust_crypto` with `features = ["pkcs8"]` only | EdDSA generation, if EdDSA is chosen over ES256 | Needs the `rand_core` feature added to generate; its `to_pkcs8_der` emits **v2**, which `Jwk::from_encoding_key` refuses (§3.3) | Yes |
| `ring` | 0.17.14 | ISC-style + others | **Already in the lock** via rustls → ureq | Alternative key factory; `SystemRandom` sidesteps the `rand_core` 0.6 mismatch | EC output is PKCS#8 v1 but with no inner `parameters` field — round-trip through `EncodingKey::from_ec_der` **unverified** (§9). Ed25519 output is v2, so unusable for JWKS | Yes — it is the tree's existing native dependency |
| `aws-lc-rs` | 1.15.0 (jsonwebtoken's pin; 1.18.1 newest, 2026-09-01) | ISC AND (Apache-2.0 OR ISC) | 224.8M all-time; MSRV 1.71.0 | Alternative `jsonwebtoken` backend; **the only** crate with a first-class Ed25519 `generate_pkcs8v1`; avoids `rsa` entirely, which is exit (b) of §3.5 | Its README (unpinned `main`) says a C/**C++** compiler is required; CMake, bindgen and Go are **never** required for non-FIPS | **No, not as-is.** The blocker is **`g++`**, not cmake — the builder installs `gcc` only, deliberately avoiding `build-essential`. Correct the note's earlier framing |
| `rsa` | 0.9.6 | MIT OR Apache-2.0 | RUSTSEC-2023-0071, **`patched = []`** | Nothing — it is an unavoidable transitive of `jsonwebtoken`'s `rust_crypto` feature | Now a **real** exposure, not paperwork (§3.4). Survives only on the ground that willikins never signs with the RSA family. A `cargo-audit`/`cargo-deny` gate needs a documented ignore with a **rewritten** justification | Yes (pure Rust) |
| `reqwest` | 0.13.5 | — | — | Nothing | **Reject**, unchanged from the existing note: a second async HTTP+TLS stack in an image that has none | n/a |

Already in `Cargo.lock` and reusable at no new cost, unchanged from the existing note: `axum`
0.8.9, `http` 1.5.0, `hyper` 1.11.1, `tokio` 1.53.1, `tower` 0.5.3, `tower-http` 0.7.1, `rmcp`
3.3.0, `rustls` 0.23.44, `ring` 0.17.14, `webpki-roots` 1.0.9, `ureq` 3.4.2, `cookie` 0.18.2, `url`
2.5.8, `base64` 0.23.1, `sha2` 0.11.0, `subtle`, `zeroize` 1.9.0, `rand` 0.9.5 and 0.10.2,
`serde_json` 1.0.151, `chrono` 0.4.45, `once_cell` 1.21.4, `async-trait` 0.1.92.

**Genuinely new on the recommended path:** `webauthn-rs` (+ core/proto), `argon2`,
`password-hash`, `rand` 0.8 as a direct dependency, and `webauthn-authenticator-rs` as a
dev-dependency. `jsonwebtoken` and `axum-extra` were already the plan's. **No authorization-server
framework.**


## 9. Verify with a browser before relying on them

Everything here is either a claim that could not be fetched verbatim, a source that could not be
pinned, or a point where two readers disagreed. Where they disagreed, the source was re-fetched in
this session and the entry says which reading the fetched text supports. **Nothing in this section
may enter frozen code.**

### 9.1 Disagreements re-fetched and settled in this session

- **Is `plain` PKCE a thing an OAuth 2.1 authorization server must handle?** One reader read
  draft-13 §7.5.2 and RFC 7636's `plain` default and concluded the AS must explicitly reject an
  omitted method; another read `oxide-auth` defaulting to `plain` and called it a configuration
  hazard. **Both were reading older text.** Fetched draft-16 §4.1.1 directly: `code_challenge_method`
  is **REQUIRED** and its value is `S256` or a future extension — there is no `plain` and no default
  to fall back to, an unsupported transformation is answered `invalid_request`, and a request with
  no `code_challenge` from a public client MUST be rejected. Advertise `["S256"]`, accept only
  `S256`, and the "defaults open" hazard disappears. Quoted in §1.5.
- **Is `iss` in the authorization response a SHOULD or a MUST?** MCP grades it SHOULD and says a
  future revision is expected to raise it. **OAuth 2.1 draft-16 §4.1.2 already lists `iss` as
  REQUIRED**, and RFC 9207 §2 makes emission a MUST for any server supporting it, with §2.3 making
  the metadata boolean a MUST alongside. Emit it. Quoted in §1.6.
- **Is rauthy's missing code→redirect_uri binding a deviation?** The reader refused to claim it
  without the norm, correctly. Fetched RFC 6749 §4.1.3 ("their values MUST be identical") and
  draft-16 §4.1.2 ("The authorization code is bound to the client identifier, code challenge and
  redirect URI"). **The fetched text supports the deviation reading** — re-checking the registered
  set is not the same obligation. Recorded in §6.5 as a norm plus three code facts, not as a CVE.
- **Does RFC 9068's "MUST include RS256" survive not signing with RSA?** One reader said exclude
  RS*/PS* from the allowlist outright; another quoted §2.1 verbatim. **Both are right about
  different halves.** §3.5 records exit (c) — RS256 stays in the *validator's* allowlist (where the
  original public-key-only reasoning genuinely holds), the *issuer* signs ES256 or EdDSA only.
  **New constraint discovered in reconciling them:** the existing note §4.2 shows `jsonwebtoken`
  rejects a `Validation` whose algorithm list spans key families, so exit (c) is implementable
  **only as per-`kid` algorithm selection**, never as one global list. That is a design consequence,
  not a footnote, and it is unverified by build.
- **Keep or delete the 1,024-entry pending-login store?** One reader said delete it (a stateless
  signed pre-auth cookie suffices for a form login); another said reuse it verbatim.
  **The fetched text settles it conditionally:** `webauthn-rs` requires the ceremony state to be
  server-side and refuses to derive serde on it by default, so **on the passkey branch the store
  stays**; on a form-login branch it can go. §4.3 and §5.3.
- **Was the aws-lc-rs blocker cmake?** No. The fetched README says CMake, bindgen and Go are
  **never** required for a non-FIPS build, and a **C/C++** compiler is. The Dockerfile installs
  `gcc` only, deliberately avoiding `build-essential` and therefore `g++`. **The blocker is `g++`.**
  §8. (The README itself is unpinned — see 9.3.)
- **`oxide-auth`'s licence.** crates.io version metadata says `MIT OR Apache-2.0`; the GitHub API
  response carries no licence field. **The published-crate metadata is what cargo builds**; the gap
  is repository hygiene, not a licensing question.
- **Where is the seam?** Three readers named three different things — the decision-10 configuration
  block, a one-method token verifier, and a `(sub, iss, scopes)` triple consumed by the session
  layer. They do not conflict; they nest. §7 states the reconciliation once and no reader's version
  alone is sufficient.
- **The two corrections to `2026-09-16-m2c-authorization.md`**, restated here so they are in the
  verify list as well as the header: `tower-sessions`' `memory-store` **is** a default feature, so
  the stated rejection reason is false (right conclusion, §5.1 supplies four real reasons); and the
  RUSTSEC-2023-0071 dismissal in §4.3 is void now that willikins signs (§3.4 restates it on a
  narrower ground). Both files are coordinator-owned and were not edited.

### 9.2 Disagreements the fetched text does **not** settle

- **`SameSite=Strict` versus `Lax` for the session cookie.** The argument for Strict is that the
  cross-site callback is gone. But with an in-house AS, `/authorize` is reached by a top-level
  navigation an MCP client launched, and the consent page needs the session cookie. **Whether
  `Strict` withholds a cookie on an externally-initiated top-level navigation to `/authorize` is a
  browser behaviour nobody fetched.** Measure it in a browser before flipping. Related and also
  unfetched: whether any mainstream browser still applies Chrome's historical two-minute
  "Lax+POST" grace (it applies only to cookies with **no** explicit `SameSite`, which is not this
  case) — do not freeze a test asserting "Lax blocks a forged cross-site POST" without checking.
- **Whether `localhost` joins `127.0.0.1` and `[::1]` in the port-relaxed redirect set.** OAuth 2.1
  writes the port exception for the IP literals and says `localhost` is NOT RECOMMENDED; the MCP
  CIMD example document registers `http://localhost:3000/callback` **alongside** the IP form; the
  TS SDK relaxes the port for all three while forbidding cross-matching between them; `oxide-auth`
  has an `IgnorePortOnLocalhost` variant. Four sources, three behaviours. Decide deliberately and
  fixture it.
- **CIMD versus pre-registration.** Not a disagreement about facts, but the readers pull opposite
  ways: pre-registration is right for a handful of clients and needs no storage; CIMD is the only
  option whose client ids survive an AS swap and the only one with evidence a real client probes for
  it (rauthy's code comment naming claude.ai, §6.7). §2.7 lays out all three branches; the plan must
  choose.
- **Whether `__Host-`prefixed cookies are honoured on `http://localhost`.** The prefix requires
  `Secure`, and browsers treat localhost as a secure context, but no primary source was fetched for
  the prefix specifically. Decides whether a hand-run `serve --http` on loopback can exercise the
  real session path.
- **Refresh tokens: draft-16's non-normative §10 says public-client refresh tokens "must" be
  sender-constrained or one-time use, while normative §4.3 says "SHOULD".** The existing note
  recorded this as biting "only on the self-hosted-AS branch". **That is now the only branch**, so
  it is a live plan decision rather than a deferred one. §1.11 shows issuing none is conformant and
  is the cleanest resolution.

### 9.3 Claims that could not be fetched verbatim, and sources that could not be pinned

Three reader findings were self-reported as not verbatim. **Two of them were re-fetched and
settled in this session** and are now verbatim; the third remains.

- **Settled.** The design doc's trust-model line, verified against the local file
  (`docs/plans/2026-09-11-willikins-design.md:22`, read 2026-09-16): "Remote-first. Willikins is an
  MCP server over Streamable HTTP running on its own host. The\n  trust boundary is a network
  boundary, so the agent's shell access on its own machine is\n  irrelevant to the butler's
  credentials." This is the grounding for §4.11's break-glass argument and it holds.
- **Settled.** `webauthn-rs`' own cost claim, verified against the extracted 0.5.5 tarball
  (`webauthn-rs-0.5.5/src/lib.rs:14-20`, read 2026-09-16): "//! In the simplest case where you just
  want to replace passwords with strong self contained multifactor\n//! authentication, you should
  use our passkey flow.\n//!\n//! Remember, no other authentication factors are needed. A passkey
  combines inbuilt user\n//! verification (pin, biometrics, etc) with a hardware cryptographic
  authenticator."
- **Not settled.** NIST SP 800-63B-4's phishing-resistance sentence for syncable authenticators
  ("Achieved: Properly configured syncable authenticators create a unique public or private key
  pair whose use is constrained to the domain in which it was created…") was reported as not
  verbatim. It is a supporting argument in §4, not a load-bearing one; confirm in a browser before
  quoting it in a plan.

Sources whose provenance is weaker than the rest, each flagged by the reader that used it:

- **NIST SP 800-63B-4 and W3C WebAuthn Level 3** were fetched as HTML and tag-stripped locally.
  Quotes are verbatim in wording but **not in whitespace**, and section numbers inside them
  ("Sec. 3.1.1.2") are the documents' own cross-references.
- **The aws-lc-rs build-prerequisites README and the distroless base README were fetched from
  `main`, not a tag.** The published aws-lc-rs 1.15.0 `.crate` tarball omits README.md, so a
  version-pinned statement of the C/C++-compiler requirement was not obtainable. The distroless
  README names no libssl version at all — **verify that `gcr.io/distroless/cc-debian12` actually
  ships the `libssl.so.3` that `openssl-sys` 0.9.114 links against** by inspecting the image. This
  is build-blocking for §4, not a design question.
- **"Debian bookworm ships OpenSSL 3.0"** and **"MPL-2.0 is compatible with an AGPL-3.0-or-later
  application as a dependency"** are inferences, not fetched statements.
- **The MCP TypeScript SDK was read at v1.29.0**, the last 1.x tag; `releases/latest` now returns a
  2.x monorepo whose auth module was **not** read. The `OAuthServerProvider`/`OAuthTokenVerifier`
  split §7 leans on may have changed shape there — **re-read the 2.x auth module before freezing
  the seam.**
- **rauthy was read at `main` (`ce7082b5d36fbf2b170d08b05aad1562aeb6ecd4`), not at release tag
  v0.36.2.** Every rauthy quote is therefore from unreleased code.
- **`webauthn-rs` has no `v0.5.5` git tag** (tags stop at v0.5.2); repository-level files were
  fetched at `d2c10d53ca5ef033d37ee6462e936e9eb72ad98c`, the sha recorded in the 0.5.5 tarball's own
  `.cargo_vcs_info.json`. No third-party security audit is claimed in its README or SECURITY.md —
  an absence, not a finding against it, and it is the only Rust relying party regardless.
- **OWASP cheat sheets** are pinned by resolving the last commit touching each file
  (`7deb20b…` for session management, `be33320…` for CSRF, `8aaf426…` for password storage, forgot
  password and MFA). Cite the sha-pinned URLs, never `master`.
- **`oauth-as`' provenance was checked only to crates.io ownership and GitHub repository metadata.**
  No commit history was read and no code was reviewed for correctness. That is exactly why §2.5
  recommends reading it, not depending on it.

### 9.4 Facts nobody could settle, which the plan must carry as verify items

- **Which `jsonwebtoken` crypto backend builds in this Dockerfile.** Still the existing note's open
  task-1 item, now with the blocker correctly identified as `g++` rather than cmake. No build was
  run.
- **Whether `ring`'s EC PKCS#8 output round-trips into `EncodingKey::from_ec_der`**, given ring
  deliberately omits the inner `parameters` field. Decides whether ring can be the ES256 key
  factory instead of `p256`. Not built, not fetched.
- **Whether `EncodingKey::from_ec_pem` accepts the PEM `elliptic_curve::SecretKey::to_pkcs8_pem`
  emits.** Both sides are documented as PKCS#8 and the labels match; the round trip was not executed.
- **Where the signing private key lives and how it rotates** — Railway volume file, or a
  Doppler-injected variable. Both fit the fetched platform facts (§3.8); the project rule that all
  secrets live in Doppler pulls one way and first-boot self-provisioning pulls the other.
- **The Railway volume's mount path.** `WILLIKINS_JOURNAL_PATH` names the journal file; no
  `railway.json`/`railway.toml` is in the tree, so the mount path is written down nowhere fetchable.
- **Whether `/approvals` will ever run as more than one replica.** A startup-generated cookie `Key`
  and an in-memory store make sessions replica-local. No Railway command was run (forbidden).
- **The maximum access-token lifetime**, which is also the minimum key-rotation overlap window
  (§3.7) and which no specification sets. Likewise the two session timeouts (§5.4b).
- **Whether the `/approvals` session depends on any artefact derived from the signing key.** If it
  does, a lost key logs every human out as well as invalidating every token.
- **Whether the AS routes (`/authorize`, `/token`, the well-known paths) must sit outside
  `allowed_hosts`.** The same question the existing note raises for the RFC 9728 route, now with
  three more unauthenticated paths.
- **Whether any `GET` handler on `/approvals` mutates state** (§5.8). The handlers were not read by
  this task.
- **Whether any browser in the operator's environment refuses `navigator.credentials`** on the
  deployment's domain. Untested; the only mitigation in the plan's shape is the recovery-code path,
  which is a reason to build recovery codes in the **same** milestone rather than after it.
- **NIST SP 800-63B-4 §3.2.2 (throttling)** was referenced by both the recovery-code and password
  rules but not read verbatim, so the recovery-code throttle number is unpinned. Likewise the
  apparent tension between §4.2.1.1 (≥64 bits, hashed as a password verifier) and §3.1.2.2
  (look-up secrets **shorter** than 112 bits SHALL use a password hashing scheme). §4.10 follows the
  reading that satisfies both: **≥128-bit codes hashed with Argon2id**; the plan should say it is
  following that reading.
- **Documents cited but not fetched**, each of which should be read before the code it governs is
  written: RFC 9700 (the Security BCP that CIMD §4.5 attributes the exact-match rule to), RFC 7009
  (only if a revocation endpoint is offered), RFC 8693 §4.2 and §4.3 (the `scope` and `client_id`
  claim definitions RFC 9068 §2.2 delegates to), OpenID Connect Discovery 1.0 (only if OIDC-shaped
  metadata is ever published), `draft-ietf-oauth-identity-assertion-authz-grant` (the standards-track
  shape of the later external-IdP adapter — read before freezing the seam), and the IANA OAuth
  Dynamic Client Registration client-metadata registry (the property set a CIMD parser must tolerate).
- **`draft-ietf-oauth-client-id-metadata-document-00`, which MCP pins in every citation, expired on
  11 April 2026.** The working-group document is at revision 02 (2026-07-06, expires 2027-01-07);
  the -00→-02 diff was not read. A CIMD implementation written from -00 could be wrong against a
  later MCP revision that repins.
  (source: https://datatracker.ietf.org/api/v1/doc/document/draft-ietf-oauth-client-id-metadata-document/?format=json,
  2026-09-16)
  - "{'name': 'draft-ietf-oauth-client-id-metadata-document', 'rev': '02', 'time': '2026-07-06T19:55:37Z', 'title': 'OAuth Client ID Metadata Document', 'expires': '2027-01-07T19:55:37Z'}"
- **Every claim taken from an Internet-Draft**, carried forward from the existing note: draft-13 and
  draft-16 are works in progress and inappropriate to cite otherwise. Pin the revision in every
  citation; draft-16 is the one fetched here and it expires 7 March 2027.
- **No build, size, compile-time or memory figure anywhere in this note is measured.** Every cost is
  a dependency count or a line count over a fetched tarball.


## 10. What this settles

Fifteen bullets. The operator asked for a judgement, not encouragement, so the size bullets come
first.

1. **The protocol half is a bounded four-to-six-thousand-line job in Rust, plus roughly 1.4× that
   in tests.** rauthy's 84,583 lines are not the number: its OAuth machinery is ~7,300 and the
   other ~77,000 is the human half. Kanidm spends ~3,600 implementation lines against ~5,160 test
   lines. Cloudflare's complete MCP authorization server is 5,967 lines **with the login declared
   out of scope**. This is a milestone, as the plan already says, and it is not a week.
2. **The human half is where the real cost is, and no OAuth crate supplies any of it.** All three
   MCP reference implementations refuse to write it. Everything §4 describes — ceremony, credential
   store, recovery codes, break-glass, enrollment CLI — is willikins' own, and MCP says nothing
   whatsoever about it.
3. **Milestone 2c acquires things the tree has never had, and each has a maintenance tail:** the
   first durable identity state (a credential store on a volume that was journal-only, now a
   backup-and-restore concern); a signing key with a lifecycle (generate, encrypt at rest, rotate,
   retain for an overlap window, sweep); single-use authorization codes that must survive a
   redeploy or not exist; the first `<script>` on the approvals page and a `script-src` CSP to go
   with it; `argon2` even on the passkey branch, for recovery codes; and **no password-reset path
   that this deployment can ever build**.
4. **Trust boundary 1 is overturned clause by clause and must be rewritten, not amended.** "It
   holds no signing key, mints no access token, runs no `/authorize`, `/token` or `/register`
   endpoint" becomes false in every clause. What replaces it is not weaker, but it is different, and
   the plan must say so in the same breath as the decision. The **resource-server** half of the
   boundary — the token is a credential for this server and nothing else, never forwarded, never
   journalled, stripped before the handler — survives untouched, which is the other reason the seam
   falls where §7 puts it.
5. **Adopt no authorization-server framework.** `oxide-auth` is the only established one; it costs
   18,732 lines across three crates and three duplicate majors to obtain ~1,500 wanted lines, and
   supplies none of RFC 8414, JWKS, RFC 7591, RFC 8707 or `at+jwt` — so §1's entire must-build list
   survives adopting it. Its `Grant` has no audience field, which is the single filter that
   eliminated the hosted providers.
6. **`oauth-as` 0.9.4 is the closest fit in the ecosystem and must not be depended on yet.** Six
   weeks old, one author, one star, no audit, 36,230 lines of security-critical source. Read it as
   prior art; record a dated re-evaluation gated on 1.0, a second maintainer, or an audit.
7. **The token-minting half costs zero new crates.** `jsonwebtoken` 11 signs, derives the public
   JWK from the private key, computes an RFC 7638 thumbprint for the `kid`, and already validates.
   Two traps need acceptance tests: `from_encoding_key` leaves `kid` as `None` while `JwkSet::find`
   skips such keys (willikins would fail to validate its own tokens), and Ed25519 needs PKCS#8 **v1**
   where every default generator emits v2. ES256 has neither trap.
8. **RUSTSEC-2023-0071's dismissal must be rewritten, and the replacement is a normative
   constraint:** willikins never uses the RSA algorithm family. RS256 stays in the *validator's*
   allowlist to satisfy RFC 9068 §2.1; the *issuer* signs ES256 or EdDSA only — implementable only
   as per-`kid` algorithm selection, because `jsonwebtoken` rejects a `Validation` spanning key
   families.
9. **Issue no refresh tokens.** MCP grades every refresh obligation below MUST and says the AS
   retains discretion, so not issuing them is conformant — and it deletes the only stateful AS
   requirement, the rotation-plus-family-revocation store. Every one of rauthy's worst problems
   lived on the refresh path: the `azp` regression a rewrite introduced and an audit missed, the
   RFC 8707 `resource` dropped on a second path, and a grace window that exists only because HA
   latency trips the reuse detector.
10. **Pre-registration is the right default for a handful of clients; CIMD is the one to add when a
    client needs it.** No registration mechanism is a server MUST. DCR is deprecated and is an
    anonymous write endpoint with a garbage collector. CIMD stores nothing and its client ids are
    the only ones that survive a later AS swap — and rauthy's code comment naming claude.ai is the
    only evidence in this note that a real client probes for it. If CIMD is taken, its outbound
    fetch needs a narrowly typed fetcher with private/loopback refusal and a 5 KB cap, and the plan
    must state that the "no raw URLs" invariant governs **tool ports**, not protocol machinery.
11. **Advertise and enforce `S256` only, and emit `iss`.** Draft-16 removes `plain` entirely —
    `code_challenge_method` is REQUIRED with value `S256` — and makes `iss` REQUIRED in the
    authorization response where MCP still says SHOULD. Omitting `code_challenge_methods_supported`
    from the metadata makes every conformant client refuse to proceed. `grant_types_supported` and
    `token_endpoint_auth_methods_supported` must be emitted explicitly, because their RFC defaults
    describe an OAuth 2.0 server that willikins is not.
12. **Passkeys over passwords, for three reasons that are not phishing resistance:** a password
    login costs 19 MiB of memory per attempt on an unauthenticated endpoint that the plan already
    caps at 1,024 entries; this deployment has no side channel and therefore **no password reset**,
    which also removes the lockout mechanism's own escape hatch; and NIST's compromised-password
    blocklist is SHALL-level and needs either third-party egress or a corpus download. Recovery
    codes (≥128 bits, Argon2id-hashed, single-use, reissued) and a host-side break-glass are needed
    on either branch.
13. **`webauthn-rs` brings OpenSSL, and the Dockerfile comment saying otherwise becomes false.**
    Builder needs `libssl-dev` and `pkg-config`; still no cmake. Separately, the `aws-lc-rs`
    backend's blocker is **`g++`**, not cmake. Both are corrections to frozen text, and the runtime
    image's libssl is a build-blocking verify item.
14. **The session decision survives, with three additions and one reason replaced.** `__Host-`,
    signed, bounded, 503-when-full and the per-plan nonce are all right — and the nonce is
    **load-bearing**, not defence in depth, because SameSite is scoped to the registrable domain and
    every sibling host under `bandeabonnot.com` is same-site. Missing: unconditional session-id
    rotation at login (fixation is a new surface now that willikins runs the login), an idle timeout
    beside the absolute one, `Cache-Control: no-store`, 128-bit ids, and credential-change
    revocation. `tower-sessions` is still declined, for four real reasons rather than the false one.
15. **The seam is three layers and one of them is new.** The configuration block and the RFC 9728
    `authorization_servers[]` array are already provider-agnostic; a one-method token verifier —
    the `oauth::validate` the plan already routes both surfaces through — is the code seam, provided
    the `JwkSet` **source** is abstracted so the RS never fetches its own `jwks_uri`; and
    authenticate-a-human is the only genuinely new trait. Publish a `jwks_uri` from day one even
    though it is OPTIONAL for a co-hosted AS, because a resource server that only knows how to read
    a key out of its own process **is** the rewrite. What leaks through and must be designed in now:
    `SameSite`, one `skipLocalPkceValidation`-shaped flag, non-portable client registrations, and
    passkeys that no adapter can carry across an RP-ID change.
