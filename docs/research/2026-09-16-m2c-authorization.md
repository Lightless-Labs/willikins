# Milestone 2c authorization research

**Created:** 2026-09-16
**Plan:** milestone 2c — OAuth 2.1 on the MCP Streamable HTTP transport and a browser login on
the approvals page. The plan does not exist yet; this note is the material it is written from.
**Feeds:** `docs/plans/2026-09-11-willikins-design.md` (the 2026-09-15 milestone 2c addendum and
its milestone list), `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` ("Trust
boundaries", the `willikins-server` section, the out-of-scope bullet on OAuth 2.1, the "Verify"
entry on the authorization spec, the 2026-09-15 task 13 addendum), and
`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md` ("Handed to milestone 3 (or 2c, the
OAuth milestone)").
**Previous:** `docs/research/2026-09-12-m2-dependencies.md`

Six research passes run in parallel on 2026-09-16, in the same format as the milestone 2 note:
facts, each carrying a source URL and a verbatim quote, then an unresolved list per section.
Every URL below was fetched on **2026-09-16**; the date is repeated in each source parenthetical
so a quote stays dated when it is lifted out of this file.

Fetch discipline, as the task set it. Prefer a primary source fetched verbatim — a spec's `.md`
twin, an RFC's plain text, a crate's own source at the pinned version, a provider's own
documentation page — over any rendered paraphrase. No mechanism name, spec revision, RFC number,
crate version or provider capability below is written from memory. Anything that could not be
fetched verbatim, and every point where two readers disagreed, is in section 7, "Verify with a
browser before relying on them", and must not enter frozen code until it is settled.

Method notes worth keeping, since they cost time to discover:

- `modelcontextprotocol.io` publishes a `.md` twin for every specification page at the same path
  plus `.md`. Quotes below were matched as exact substrings against those raw files, not against
  a summarizing fetcher's rendering.
- Auth0 is the only identity provider surveyed that publishes agent-readable docs: an
  `llms.txt`, a `.md` twin per page, and a machine-readable Authentication API OpenAPI
  description. Zitadel, Keycloak, authentik, Logto and Ory publish none; their documentation
  source lives in git and was fetched from the release tag, which is the stronger source anyway.
- `gh api repos/<r>/git/trees/<ref>?recursive=1` silently truncates on large repositories and
  will produce a false "no such docs directory" conclusion. Walk `repos/<r>/contents/<dir>`.
  `gh api search/...` needs `-f q='...'`; an inline `?q=a+b` returns zero results for a query
  that matches.

## Two corrections to the brief this research was commissioned under

Both were found by reading the repository rather than a source, and both are load-bearing enough
that carrying the brief's version forward would put a wrong number in the plan.

- **`reqwest` is not in the tree, at any version.** The brief said the providers already pull it.
  `grep -c 'name = "reqwest"' Cargo.lock` returns `0`. The only HTTP client is `ureq` 3.4.2
  (synchronous), reached through `willikins-providers-http`, over `rustls` 0.23.44 with `ring`
  0.17.14 and `webpki-roots` 1.0.9. There is no `tokio-rustls`, `hyper-rustls`, `hyper-tls`,
  `rustls-native-certs` or `rustls-platform-verifier` in the lock: the server has **no async TLS
  client at all**. A JWKS fetch should reuse `ureq` inside `tokio::task::spawn_blocking`, the
  bridge `crates/willikins-server/src/mcp.rs` already documents for every Butler call.
- **The MSRV to measure a candidate crate against is 1.88, not 1.97.** `Cargo.toml` line 21
  declares `rust-version = "1.88"` with `edition = "2024"` in `[workspace.package]`; 1.97 is the
  installed toolchain and the Dockerfile's pinned build image. A crate with an MSRV between 1.89
  and 1.97 would build on this host and still raise the declared workspace floor.

Sections:

1. The MCP authorization specification at the negotiated revision
2. The RFCs the specification delegates to
3. `rmcp`'s server-side story
4. Rust crates, with a pin table
5. Railway exposure
6. Identity providers, one row per provider
7. Verify with a browser before relying on them
8. What this settles for the plan


## 1. The MCP authorization specification at the negotiated revision

### 1.1 Which revision is the anchor

Two revisions are in play at once, and the plan must satisfy both or close the divergence
deliberately.

- `rmcp` 3.3.0 — the version `Cargo.lock` pins — sets `ProtocolVersion::LATEST` to
  `V_2025_11_25`, so a default rmcp client and server negotiate **2025-11-25**. Verified in this
  session by reading the crate source cargo checksum-verified against the lock.
  (source: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.3.0/src/model.rs:170-186`,
  mirrored at https://docs.rs/crate/rmcp/3.3.0/source/src/model.rs, 2026-09-16)
  - `pub const V_2026_07_28: Self = Self(Cow::Borrowed("2026-07-28"));` /
    `pub const V_2025_11_25: Self = Self(Cow::Borrowed("2025-11-25"));` /
    `pub const LATEST: Self = Self::V_2025_11_25;` /
    `pub const STANDARD_HEADERS: Self = Self::V_2026_07_28;`
- `willikins` already advertises the *later* revision in `get_info`. So the revision clients
  negotiate is 2025-11-25 and the revision willikins claims is 2026-07-28.
  (source: `crates/willikins-server/src/mcp.rs:721-722`, read 2026-09-16)
  - `ServerInfo::new(ServerCapabilities::builder().enable_tools().build())` /
    `.with_protocol_version(ProtocolVersion::V_2026_07_28)`
- The specification site's own version list names 2026-07-28 as current.
  (source: https://modelcontextprotocol.io/specification/versioning.md, 2026-09-16)
  - "The **current** protocol version is [**2026-07-28**](/specification/2026-07-28/)."

Section 1.2 states the 2025-11-25 obligations, which is what a default client negotiates today.
Section 1.3 lists only what 2026-07-28 changes.

### 1.2 What 2025-11-25 requires

**Authorization is optional overall; stdio is explicitly excluded.** This ratifies both
`serve --stdio` having no authentication and today's bearer-hash scheme: an HTTP deployment
without OAuth is not spec-violating, it is non-conforming to an optional part.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "Authorization is **OPTIONAL** for MCP implementations. When supported:\n\n* Implementations using an HTTP-based transport **SHOULD** conform to this specification.\n* Implementations using an STDIO transport **SHOULD NOT** follow this specification, and\n  instead retrieve credentials from the environment.\n* Implementations using alternative transports **MUST** follow established security best\n  practices for their protocol."

This closes the milestone 2 plan's open question exactly as the plan framed it. The plan recorded
the current 401 as the spec's "custom authentication strategy" and said to "revisit when the
authorization spec verification says a bearer token is not acceptable for a private deployment".
The spec never says that. It says that *if* you conform, the token must be an OAuth-issued,
audience-bound one and the 401 must point at protected resource metadata. The trigger for 2c is
therefore the public-domain decision plus interoperability with OAuth-capable MCP clients, not a
spec prohibition.
(source: `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md:53-55`, read 2026-09-16)

- "This is the MCP specification's\n\"custom authentication strategy\" (authorization is optional in the spec and this server\ndoes not implement the OAuth 2.1 framework), so the 401 carries no `resource_metadata`\nchallenge; recorded as a decision."

**The server MUST publish RFC 9728 protected resource metadata containing
`authorization_servers`, and MUST make it discoverable one of two ways.** Clients must support
both and prefer the header.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "MCP servers **MUST** implement the OAuth 2.0 Protected Resource Metadata ([RFC9728](https://datatracker.ietf.org/doc/html/rfc9728))\nspecification to indicate the locations of authorization servers. The Protected Resource Metadata document returned by the MCP server **MUST** include\nthe `authorization_servers` field containing at least one authorization server."
- "MCP servers **MUST** implement one of the following discovery mechanisms to provide authorization server location information to MCP clients:\n\n1. **WWW-Authenticate Header**: Include the resource metadata URL in the `WWW-Authenticate` HTTP header under `resource_metadata` when returning `401 Unauthorized` responses, as described in [RFC9728 Section 5.1](https://datatracker.ietf.org/doc/html/rfc9728#name-www-authenticate-response).\n\n2. **Well-Known URI**: Serve metadata at a well-known URI as specified in [RFC9728](https://datatracker.ietf.org/doc/html/rfc9728). This can be either:\n   * At the path of the server's MCP endpoint: `https://example.com/public/mcp` could host metadata at `https://example.com/.well-known/oauth-protected-resource/public/mcp`\n   * At the root: `https://example.com/.well-known/oauth-protected-resource`"

**The 401 shape, verbatim, and the status-code table.** Today willikins answers `401` with
`WWW-Authenticate: Bearer realm="willikins"` and deliberately no `resource_metadata`; adding
`resource_metadata` and `scope` is precisely the 2c wire delta.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "Example 401 response with scope guidance:\n\n```http theme={null}\nHTTP/1.1 401 Unauthorized\nWWW-Authenticate: Bearer resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource\",\n                         scope=\"files:read\"\n```"
- "Servers **MUST** return appropriate HTTP status codes for authorization errors:\n\n| Status Code | Description  | Usage                                      |\n| ----------- | ------------ | ------------------------------------------ |\n| 401         | Unauthorized | Authorization required or token invalid    |\n| 403         | Forbidden    | Invalid scopes or insufficient permissions |\n| 400         | Bad Request  | Malformed authorization request            |"

**The 403 insufficient-scope shape.** Note the "recommended approach" that accompanies this at
2025-11-25 is reversed at 2026-07-28 (section 1.3).
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "Example insufficient scope response:\n\n```http theme={null}\nHTTP/1.1 403 Forbidden\nWWW-Authenticate: Bearer error=\"insufficient_scope\",\n                         scope=\"files:read files:write user:profile\",\n                         resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource\",\n                         error_description=\"Additional file write permission required\"\n```"

**Audience binding: what the client sends and what the server must check.** This fixes the
audience to whatever public origin 2c gives the service, which is undecided (section 5).
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "MCP clients **MUST** implement Resource Indicators for OAuth 2.0 as defined in [RFC 8707](https://www.rfc-editor.org/rfc/rfc8707.html)\nto explicitly specify the target resource for which the token is being requested. The `resource` parameter:\n\n1. **MUST** be included in both authorization requests and token requests.\n2. **MUST** identify the MCP server that the client intends to use the token with.\n3. **MUST** use the canonical URI of the MCP server as defined in [RFC 8707 Section 2](https://www.rfc-editor.org/rfc/rfc8707.html#name-access-token-request)."
- "MCP servers, acting in their role as an OAuth 2.1 resource server, **MUST** validate access tokens as described in\n[OAuth 2.1 Section 5.2](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-13#section-5.2).\nMCP servers **MUST** validate that access tokens were issued specifically for them as the intended audience,\naccording to [RFC 8707 Section 2](https://www.rfc-editor.org/rfc/rfc8707.html#section-2)."

This sentence is the one place the resource-server audience check is a MUST. RFC 8707's own text
is weaker; see section 2.4 for the reconciliation, which two readers initially disagreed about.

What ships today, read from the repository rather than relayed: the `/mcp` 401 carries
`Bearer realm="willikins"` and no `resource_metadata`, and the single production site that
attaches an outgoing `Authorization` header is `Credential::authorize`, which says so itself and
is enforced by `clippy.toml` plus a guard test — which is what makes the passthrough prohibition
checkable rather than merely intended.
(sources: `crates/willikins-server/src/http/auth.rs:85` and
`crates/willikins-providers-http/src/credential.rs:84-92`, read 2026-09-16)

```rust
// http/auth.rs:85
[(header::WWW_AUTHENTICATE, "Bearer realm=\"willikins\"")],

// credential.rs:84-92
// The one allowed production call site outside the derive's own
// codegen, named in `clippy.toml`'s `disallowed-methods` reason and
// walked by `crates/willikins-core/tests/expose_secret_guard.rs`.
#[allow(clippy::disallowed_methods)]
pub(crate) fn authorize<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
    let token = self.secret.expose_secret();
    request.header("Authorization", format!("Bearer {token}"))
}
```

**The token-passthrough prohibition, stated twice.** This is a negative requirement 2c must not
violate rather than work it must do: willikins' operator-provisioned GitHub and Doppler
credentials already satisfy it structurally, because no caller token ever reaches an upstream
API and `Credential::authorize` is the single site that attaches an outgoing `Authorization`
header.
(sources: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md and
https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices.md, 2026-09-16)

- "MCP clients **MUST NOT** send tokens to the MCP server other than ones issued by the MCP server's authorization server.\n\nMCP servers **MUST** only accept tokens that are valid for use with their\nown resources.\n\nMCP servers **MUST NOT** accept or transit any other tokens."
- "If the MCP server makes requests to upstream APIs, it may act as an OAuth client to them. The access token used at the upstream API is a separate token, issued by the upstream authorization server. The MCP server **MUST NOT** pass through the token it received from the MCP client."
- (security_best_practices.md, "Token Passthrough" → "Mitigation") "MCP servers **MUST NOT** accept any tokens that were not explicitly\nissued for the MCP server."

**How the token travels.** This is exactly willikins' current stateless per-request bearer
middleware, so the transport shape does not change at 2c — only the provenance and validation of
the token.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "1. MCP client **MUST** use the Authorization request header field defined in\n   [OAuth 2.1 Section 5.1.1](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-13#section-5.1.1):\n\n```\nAuthorization: Bearer <access-token>\n```\n\nNote that authorization **MUST** be included in every HTTP request from client to server,\neven if they are part of the same logical session."
- "2. Access tokens **MUST NOT** be included in the URI query string"

**Scopes are server-defined. There is no registry and no reserved name.** The spec defines only
the machinery: `scopes_supported` in the metadata document, the `scope` parameter in the
challenge, and a client selection rule. `mcp:tools-basic` appears only as an example.
(sources: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md and
.../security_best_practices.md, 2026-09-16)

- "The scopes included in the `WWW-Authenticate` challenge **MAY** match `scopes_supported`, be a subset\nor superset of it, or an alternative collection that is neither a strict subset nor\nsuperset. Clients **MUST NOT** assume any particular set relationship between the challenged\nscope set and `scopes_supported`. Clients **MUST** treat the scopes provided in the\nchallenge as authoritative for satisfying the current request."
- (security_best_practices.md, "Scope Minimization" → "Mitigation") "* Minimal initial scope set (e.g., `mcp:tools-basic`) containing only\n  low-risk discovery/read operations"
- (security_best_practices.md, "Common Mistakes") "* Publishing all possible scopes in `scopes_supported`\n* Using wildcard or omnibus scopes (`*`, `all`, `full-access`)"

**Client registration is a client and authorization-server obligation, not a resource-server
one.** Three mechanisms at this revision, none of them a MUST.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "MCP supports three client registration mechanisms. Choose based on your scenario:\n\n* **Client ID Metadata Documents**: When client and server have no prior relationship (most common)\n* **Pre-registration**: When client and server have an existing relationship\n* **Dynamic Client Registration**: For backwards compatibility or specific requirements\n\nClients supporting all options **SHOULD** follow the following priority order:\n\n1. Use pre-registered client information for the server if the client has it available\n2. Use Client ID Metadata Documents if the Authorization Server indicates if the server supports it (via `client_id_metadata_document_supported` in OAuth Authorization Server Metadata)\n3. Use Dynamic Client Registration as a fallback if the Authorization Server supports it (via `registration_endpoint` in OAuth Authorization Server Metadata)\n4. Prompt the user to enter the client information if no other option is available"
- "MCP clients and authorization servers **MAY** support the\nOAuth 2.0 Dynamic Client Registration Protocol [RFC7591](https://datatracker.ietf.org/doc/html/rfc7591)\nto allow MCP clients to obtain OAuth client IDs without user interaction.\nThis option is included for backwards compatibility with earlier versions of the MCP authorization spec."

**PKCE is a client MUST. No PKCE obligation falls on a resource server.** It becomes willikins'
problem only on the branch where willikins hosts its own authorization server, or where the
approvals login makes willikins an OAuth client to an external provider.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "To mitigate this, MCP clients **MUST** implement PKCE according to [OAuth 2.1 Section 7.5.2](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-13#section-7.5.2) and **MUST** verify PKCE support before proceeding with authorization."
- "MCP clients **MUST** use the `S256` code challenge method when technically capable, as required by [OAuth 2.1 Section 4.1.1](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-13#section-4.1.1)."
- "* **OAuth 2.0 Authorization Server Metadata**: If `code_challenge_methods_supported` is absent, the authorization server does not support PKCE and MCP clients **MUST** refuse to proceed."

**The authorization server may be co-hosted or separate, and its implementation is out of
scope.** Nothing in the specification decides the identity-provider question.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "The *authorization server* is responsible for interacting with the user (if necessary) and issuing access tokens for use at the MCP server.\nThe implementation details of the authorization server are beyond the scope of this specification. It may be hosted with the\nresource server or a separate entity."

**The confused-deputy MUSTs have four precise trigger conditions, and willikins meets none of
them today.** They bite the moment 2c introduces a per-user upstream OAuth flow. This is the one
design fork in the whole note.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices.md, 2026-09-16)

- "**MCP Proxy Server**\n: An MCP server that connects MCP clients to third-party APIs, offering\nMCP features while delegating operations and acting as a single OAuth\nclient to the third-party API server."
- "This attack becomes possible when all of the following conditions are\npresent:\n\n* MCP proxy server uses a **static client ID** with a third-party\n  authorization server\n* MCP proxy server allows MCP clients to **dynamically register** (each\n  getting their own client\\_id)\n* The third-party authorization server sets a **consent cookie** after\n  the first authorization\n* MCP proxy server does not implement proper per-client consent before\n  forwarding to third-party authorization"
- "MCP proxy servers **MUST**:\n\n* Maintain a registry of approved `client_id` values per user\n* Check this registry **before** initiating the third-party\n  authorization flow\n* Store consent decisions securely (server-side database, or server\n  specific cookies)"

**The authorization specification does not reach the approvals page.** Its stated scope is "MCP
clients to make requests to restricted MCP servers on behalf of resource owners". The nearest
applicable text is the confused-deputy consent-UI and consent-cookie list, which is a MUST only
for an MCP Proxy Server's consent page. For willikins' approvals page it is the closest analogue
and an adoptable checklist, explicitly **not** a binding requirement — and it already aligns with
the shipped single-use nonce and Origin/Referer defences.
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices.md, 2026-09-16)

- "The MCP-level consent page **MUST**:\n\n* Clearly identify the requesting MCP client by name\n* Display the specific third-party API scopes being requested\n* Show the registered `redirect_uri` where tokens will be sent\n* Implement CSRF protection (e.g., state parameter, CSRF tokens)\n* Prevent iframing via `frame-ancestors` CSP directive or\n  `X-Frame-Options: DENY` to prevent clickjacking"
- "If using cookies to track consent decisions, they **MUST**:\n\n* Use `__Host-` prefix for cookie names\n* Set `Secure`, `HttpOnly`, and `SameSite=Lax` attributes\n* Be cryptographically signed or use server-side sessions\n* Bind to the specific `client_id` (not just \"user has consented\")"

**The normative base list, for cross-checking section 2.**
(source: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization.md, 2026-09-16)

- "* OAuth 2.1 IETF DRAFT ([draft-ietf-oauth-v2-1-13](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-13))\n* OAuth 2.0 Authorization Server Metadata\n  ([RFC8414](https://datatracker.ietf.org/doc/html/rfc8414))\n* OAuth 2.0 Dynamic Client Registration Protocol\n  ([RFC7591](https://datatracker.ietf.org/doc/html/rfc7591))\n* OAuth 2.0 Protected Resource Metadata ([RFC9728](https://datatracker.ietf.org/doc/html/rfc9728))\n* OAuth Client ID Metadata Documents ([draft-ietf-oauth-client-id-metadata-document-00](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-client-id-metadata-document-00))"

The 2025-11-25 authorization page cites **zero** SEPs (grep count 0 over the fetched file). Every
authorization-relevant SEP comes from the 2026-07-28 changelog, below — worth stating so nobody
hunts a cross-reference that does not exist.

### 1.3 What 2026-07-28 changes

willikins already advertises this revision, and an `rmcp` version bump would move `LATEST` under
the implementation, so the deltas are not hypothetical.

**Four changelog entries, all falling on the client or the authorization server.**
(source: https://modelcontextprotocol.io/specification/2026-07-28/changelog.md, 2026-09-16)

- "7. Authorization servers **SHOULD** include the `iss` parameter in authorization responses per\n   [RFC 9207](https://datatracker.ietf.org/doc/html/rfc9207), and MCP clients **MUST** validate a\n   present `iss` against the recorded issuer before redeeming the authorization code\n   ([SEP-2468](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2468))."
- "9. Clarify that client credentials are bound to the authorization server that issued them:\n   clients **MUST** key persisted credentials by the issuer identifier, **MUST NOT** reuse them\n   with a different authorization server, and **MUST** re-register when the authorization server\n   changes ([SEP-2352](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2352))."
- (Deprecated) "4. Deprecate the OAuth 2.0 Dynamic Client Registration Protocol\n   ([RFC7591](https://datatracker.ietf.org/doc/html/rfc7591)) as a client registration\n   mechanism in favor of\n   [Client ID Metadata Documents](/specification/2026-07-28/basic/authorization/client-registration#client-id-metadata-documents)\n   ([PR #2858](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2858)).\n   It remains available for backwards compatibility with authorization servers that do\n   not support Client ID Metadata Documents."

**Four further changes that are not in the changelog and were found by diffing the page.** The
first is a new resource-server MUST — the only one this revision adds. All four were re-verified
against the raw `.md` in this session.
(source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization.md, 2026-09-16)

- (line 406, new server MUST) "Servers **MUST** account for scope hierarchies, where a broader scope implies narrower ones, when\ndeciding whether a token is sufficient for an operation."
- (new client MUST; registration was only SHOULD/MAY at 2025-11-25) "Before initiating the authorization flow, MCP clients **MUST** obtain a client ID through\none of three registration mechanisms: Client ID Metadata Documents, pre-registration, or\nDynamic Client Registration"
- (line 313, new resource-server SHOULD NOT) "**MCP Servers** (Protected Resources) **SHOULD NOT** include `offline_access` in\n`WWW-Authenticate` scope or Protected Resource Metadata `scopes_supported`, as refresh\ntokens are not a resource requirement."
- (the 403 guidance, reversed from 2025-11-25's "include previously granted scopes") "The `scope` attribute describes the scopes necessary to access\nthe requested resource — servers are not required to include\nthe client's previously granted scopes."

**Which OAuth 2.1 draft each revision's section numbers resolve to** — a point two readers
treated differently, settled here by counting references in the raw files. 2025-11-25 cites
draft-13 eighteen times and nothing else. 2026-07-28 cites draft-13 nine times and draft-14 once,
in its new Refresh Tokens section. The current published revision is draft-16 (section 2.6). The
practical question is whether the cited section numbers still mean the same thing, and for the
one that matters most they do: draft-13 §5.2 is "Access Token Validation" and names RFC 7662 and
RFC 9068 as the two standardized routes, the same content the draft-16 §5.2 quoted in section 2.6
carries.
(sources: the two `.md` files above plus https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-13.txt, 2026-09-16)

- (`grep -o 'draft-ietf-oauth-v2-1-[0-9]*' | sort | uniq -c`) 2025-11-25: "18 draft-ietf-oauth-v2-1-13". 2026-07-28: "9 draft-ietf-oauth-v2-1-13" and "1 draft-ietf-oauth-v2-1-14".
- (draft-13, §5.2) "5.2.  Access Token Validation\n\n   After receiving the access token, the resource server MUST check that\n   the access token is not yet expired, is authorized to access the\n   requested resource, was issued with the appropriate scope, and meets\n   other policy requirements of the resource server to access the\n   protected resource.\n...\n   A standardized method to query the authorization server to check the\n   validity of an access token is defined in Token Introspection\n   [RFC7662].\n\n   A standardized method of encoding information in a token string is\n   defined in JWT Profile for Access Tokens [RFC9068]."

**Unresolved**

- Whether milestone 2c targets 2025-11-25 or 2026-07-28, or closes the divergence between what
  rmcp negotiates and what `mcp.rs:722` advertises. 2026-07-28 adds one new resource-server MUST
  (scope hierarchies), one new SHOULD NOT (`offline_access`) and a reversed 403 rule.
- Which scope vocabulary willikins defines. Scopes are entirely server-defined; the specification
  offers no registry and names wildcard or omnibus scopes as a mistake. A concrete set (read
  versus plan versus apply versus approve, or per tool) has to be invented and then frozen into
  `scopes_supported` and the challenge.
- Whether willikins hosts its own authorization server or delegates to an external provider. The
  specification puts this out of scope. The AS-side MUSTs — OAuth 2.1, HTTPS endpoints, exact
  `redirect_uri` validation, refresh-token rotation for public clients,
  `code_challenge_methods_supported` in metadata, CIMD SHOULD — apply only on the self-hosted
  branch.
- Whether 2c stays on the "operator credentials still reach GitHub and Doppler" branch. If it
  does, the confused-deputy MUSTs do not apply and willikins is not an MCP Proxy Server in the
  specification's defined sense. If 2c introduces a per-user GitHub OAuth flow it becomes one,
  and a hard set of MUSTs lands at once. Credential routing across several GitHub organizations
  and Doppler workplaces is milestone 3's, not 2c's, which argues for the first branch.
- The 2026-07-28 sub-pages `/basic/authorization/authorization-server-discovery`,
  `/client-registration` and `/security-considerations` were not fetched. The index says
  `security-considerations` covers "mix-up and confused deputy attacks"; "mix-up" does not appear
  on the 2025-11-25 page, so a further obligation may be uncaptured.
- Whether any extension in `https://github.com/modelcontextprotocol/ext-auth` bears on 2c. The
  specification names the repository; the extensions were not fetched.


## 2. The RFCs the specification delegates to

### 2.1 Status of every document named

All five RFCs the specification's base list names are Proposed Standard on the IETF stream with
empty obsoletes, obsoleted-by, updates and updated-by lists. RFC 9068, which section 2.5 shows is
where the resource-server audience MUST actually lives, is the same. Status came from the RFC
Editor JSON index and the datatracker document API; datatracker state 177 resolves to statetype
`rfc`, name `Published`.
(sources: https://www.rfc-editor.org/rfc/rfc9728.json and siblings for 8707, 7591, 8414, 7636,
9068; https://datatracker.ietf.org/api/v1/doc/document/rfc9728/?format=json and siblings;
https://datatracker.ietf.org/api/v1/doc/state/177/?format=json, 2026-09-16)

- "rfc9728: status= PROPOSED STANDARD obsoleted_by= [] updated_by= [] updates= [] obsoletes= [] pub= April 2025\nrfc8707: status= PROPOSED STANDARD obsoleted_by= [] updated_by= [] updates= [] obsoletes= [] pub= February 2020\nrfc7591: status= PROPOSED STANDARD obsoleted_by= [] updated_by= [] updates= [] obsoletes= [] pub= July 2015\nrfc8414: status= PROPOSED STANDARD obsoleted_by= [] updated_by= [] updates= [] obsoletes= [] pub= June 2018\nrfc7636: status= PROPOSED STANDARD obsoleted_by= [] updated_by= [] updates= [] obsoletes= [] pub= September 2015\n\nstate 177: /api/v1/doc/statetype/rfc/ | Published |"
- (RFC 9068) "status= PROPOSED STANDARD obsoleted_by= [] updated_by= [] pub= October 2021 title= JSON Web Token (JWT) Profile for OAuth 2.0 Access Tokens"

Note that the datatracker `time` field on these documents reads 2026-05-20. That is a record-touch
timestamp, not a publication date.

### 2.2 RFC 9728: the document willikins publishes

**Exactly one field is REQUIRED, and it is not the one the MCP specification insists on.** RFC
9728 requires only `resource`; `authorization_servers` is OPTIONAL in the RFC and MUST in the MCP
specification (section 1.2). The minimum conforming document therefore satisfies both: `resource`
plus `authorization_servers`, with `scopes_supported` RECOMMENDED and
`bearer_methods_supported: ["header"]` the honest declaration for a server that only reads the
header.
(source: https://www.rfc-editor.org/rfc/rfc9728.txt, 2026-09-16)

- "   resource\n      REQUIRED.  The protected resource's resource identifier, as\n      defined in Section 1.2.\n\n   authorization_servers\n      OPTIONAL.  JSON array containing a list of OAuth authorization\n      server issuer identifiers, as defined in [RFC8414], for\n      authorization servers that can be used with this protected\n      resource.\n...\n   scopes_supported\n      RECOMMENDED.  JSON array containing a list of scope values, as\n      defined in OAuth 2.0 [RFC6749], that are used in authorization\n      requests to request access to this protected resource.\n...\n   bearer_methods_supported\n      OPTIONAL.  JSON array containing a list of the supported methods\n      of sending an OAuth 2.0 bearer token [RFC6750] to the protected\n      resource.  Defined values are [\"header\", \"body\", \"query\"],\n      corresponding to Sections 2.1, 2.2, and 2.3 of [RFC6750].  The\n      empty array [] can be used to indicate that no bearer methods are\n      supported.\n...\n   resource_name\n      Human-readable name of the protected resource intended for display\n      to the end user.  It is RECOMMENDED that protected resource\n      metadata include this field.\n...\n   Additional protected resource metadata parameters MAY also be used."

**The well-known URL is built by INSERTION between host and path, not by appending.** This is the
single most likely implementation mistake in the whole milestone. If the resource identifier were
`https://<host>/mcp` the document must be served at
`https://<host>/.well-known/oauth-protected-resource/mcp` — **not** at
`https://<host>/mcp/.well-known/...`. If the identifier is the bare origin it is served at
`https://<host>/.well-known/oauth-protected-resource`. Both are legal; this note does not choose.
(source: https://www.rfc-editor.org/rfc/rfc9728.txt, 2026-09-16)

- "   Resource Identifier:\n      The protected resource's resource identifier, which is a URL that\n      uses the https scheme and has no fragment component.  As specified\n      in Section 2 of [RFC8707], it also SHOULD NOT include a query\n      component"
- "3.  Obtaining Protected Resource Metadata\n\n   Protected resources supporting metadata MUST make a JSON document\n   containing metadata as specified in Section 2 available at a URL\n   formed by inserting a well-known URI string into the protected\n   resource's resource identifier between the host component and the\n   path and/or query components, if any.  By default, the well-known URI\n   string used is /.well-known/oauth-protected-resource."
- "   If the resource identifier value contains a path or query component,\n   any terminating slash (/) following the host component MUST be\n   removed before inserting /.well-known/ and the well-known URI path\n   suffix between the host component and the path and/or query\n   components. ...\n\n     GET /.well-known/oauth-protected-resource/resource1 HTTP/1.1\n     Host: resource.example.com"
- "3.2.  Protected Resource Metadata Response\n\n   A successful response MUST use the 200 OK\n   HTTP status code and return a JSON object using the application/json\n   content type ...\n\n   Parameters with multiple values are represented as JSON arrays.\n   Parameters with zero values MUST be omitted from the response."

**The `resource` value the document returns is checked twice by the client, and a mismatch makes
the whole document unusable.** The second rule binds the document to the URL the client actually
requested, which means the value cannot be a convenient alias.
(source: https://www.rfc-editor.org/rfc/rfc9728.txt, 2026-09-16)

- "3.3.  Protected Resource Metadata Validation\n\n   The resource value returned MUST be identical to the protected\n   resource's resource identifier value into which the well-known URI\n   path suffix was inserted to create the URL used to retrieve the\n   metadata.  If these values are not identical, the data contained in\n   the response MUST NOT be used.\n\n   If the protected resource metadata was retrieved from a URL returned\n   by the protected resource via the WWW-Authenticate resource_metadata\n   parameter, then the resource value returned MUST be identical to the\n   URL that the client used to make the request to the resource server.\n   If these values are not identical, the data contained in the response\n   MUST NOT be used."

**The `resource_metadata` challenge parameter, defined.** It may also be used with schemes other
than Bearer.
(source: https://www.rfc-editor.org/rfc/rfc9728.txt, 2026-09-16)

- "5.1.  WWW-Authenticate Response\n\n   This specification introduces a new parameter in the WWW-Authenticate\n   HTTP response header field to indicate the protected resource\n   metadata URL:\n\n   resource_metadata:\n      The URL of the protected resource metadata.\n...\n   HTTP/1.1 401 Unauthorized\n   WWW-Authenticate: Bearer resource_metadata=\n     \"https://resource.example.com/.well-known/oauth-protected-resource\""

**Why audience restriction matters here specifically.** RFC 9728 says its own features make the
attack "somewhat more likely", which is the security rationale for the MCP specification's
audience MUST.
(source: https://www.rfc-editor.org/rfc/rfc9728.txt, 2026-09-16)

- "7.4.  Audience-Restricted Access Tokens\n\n   If a client expects to interact with multiple resource servers, the\n   client SHOULD request audience-restricted access tokens using\n   [RFC8707], and the authorization server SHOULD support audience-\n   restricted access tokens.\n\n   Without audience-restricted access tokens, a malicious resource\n   server (RS1) may be able to use the WWW-Authenticate header to get a\n   client to request an access token with a scope used by a legitimate\n   resource server (RS2), and after the client sends a request to RS1,\n   then RS1 could reuse the access token at RS2.\n\n   While this attack is not explicitly enabled by this specification and\n   is possible in a plain OAuth 2.0 deployment, it is made somewhat more\n   likely by the use of dynamically configured clients."

### 2.3 RFC 8707: the resource indicator

**Shape of the value, and the multi-tenant rule.** The last is a forward dependency on milestone
3: whichever resource identifier 2c freezes constrains how per-organization resources can be
expressed later.
(source: https://www.rfc-editor.org/rfc/rfc8707.txt, 2026-09-16)

- "   resource\n      Indicates the target service or resource to which access is being\n      requested.  Its value MUST be an absolute URI, as specified by\n      Section 4.3 of [RFC3986].  The URI MUST NOT include a fragment\n      component.  It SHOULD NOT include a query component ...  Multiple \"resource\" parameters MAY\n      be used to indicate that the requested token is intended to be used\n      at multiple resources."
- "   The client SHOULD use the base URI of the API\n   as the \"resource\" parameter value unless specific knowledge of the\n   resource dictates otherwise.  For example, the value\n   \"https://api.example.com/\" would be used for a resource that is the\n   exclusive application on that host; however, if the resource is one\n   of many applications on that host, something like\n   \"https://api.example.com/app/\" would be used as a more specific\n   value."
- "3.  Security Considerations\n\n   Some servers may host user content or be multi-tenant.  In order to\n   avoid attacks where one tenant uses an access token to illegitimately\n   access resources owned by a different tenant, it is important to use\n   a specific resource URI including any portion of the URI that\n   identifies the tenant, such as a path component."
- "   Although multiple occurrences of the \"resource\" parameter may be\n   included in a token request, using only a single \"resource\" parameter\n   is encouraged.  If a bearer token has multiple intended recipients\n   (audiences), then the token is valid at more than one protected\n   resource and can be used by any one of those resources to access any\n   of the others."

**How the parameter becomes an audience, and the error code for a bad one.**
(source: https://www.rfc-editor.org/rfc/rfc8707.txt, 2026-09-16)

- "   The authorization server SHOULD audience-restrict issued access\n   tokens to the resource(s) indicated by the \"resource\" parameter.\n   Audience restrictions can be communicated in JSON Web Tokens\n   [RFC7519] with the \"aud\" claim and the top-level member of the same\n   name provides the audience restriction information in a Token\n   Introspection [RFC7662] response.  The authorization server may use\n   the exact \"resource\" value as the audience or it may map from that\n   value to a more general URI or abstract identifier for the given\n   resource."
- "   invalid_target\n      The requested resource is invalid, missing, unknown, or malformed."

### 2.4 Reconciling the two layering tensions

Two readers appeared to contradict each other. Both were right about their own source; the
apparent conflict is a layering artefact, and it is written out here so the plan does not
re-litigate it.

- **`authorization_servers`: OPTIONAL or MUST?** RFC 9728 §2 makes it OPTIONAL. The MCP
  specification makes it a MUST with at least one entry. A resource server that claims MCP
  conformance publishes both `resource` (the RFC's only REQUIRED field) and
  `authorization_servers` (the specification's). No contradiction: the profile narrows the RFC.
- **The resource-server audience check: SHOULD or MUST?** RFC 8707 §2 imposes a SHOULD, and it is
  on the *authorization server* — "The authorization server SHOULD audience-restrict issued
  access tokens" (quoted in 2.3). RFC 9728 §7.4 likewise only RECOMMENDS. The resource-server
  **MUST** comes from two other places, both re-fetched in this session: the MCP specification's
  own sentence, "MCP servers **MUST** validate that access tokens were issued specifically for
  them as the intended audience, according to RFC 8707 Section 2" (line 474 of the 2025-11-25
  `.md`, quoted in section 1.2), and — for JWT-format tokens specifically — RFC 9068 §4, quoted
  next. So: nothing in RFC 8707's own text obliges willikins to check `aud`; the MCP profile and
  the JWT profile both do.

### 2.5 RFC 9068: what a resource server must check in a JWT access token

OAuth 2.1 §5.2 names RFC 7662 (introspection) and RFC 9068 (JWT) as the two standardized
validation routes (quoted in section 1.3). If the chosen provider issues RFC 9068 JWTs, this list
is the validation the plan must implement, and every failure is `invalid_token`.
(source: https://www.rfc-editor.org/rfc/rfc9068.txt, 2026-09-16)

- "   Authorization servers SHOULD use OAuth 2.0 Authorization Server\n   Metadata [RFC8414] to advertise to resource servers their signing\n   keys via \"jwks_uri\" and what \"iss\" claim value to expect via the\n   \"issuer\" metadata value.\n\n   Resource servers receiving a JWT access token MUST validate it in the\n   following manner.\n\n   *  The resource server MUST verify that the \"typ\" header value is\n      \"at+jwt\" or \"application/at+jwt\" and reject tokens carrying any\n      other value.\n\n   *  The issuer identifier for the authorization server (which is\n      typically obtained during discovery) MUST exactly match the value\n      of the \"iss\" claim.\n\n   *  The resource server MUST validate that the \"aud\" claim contains a\n      resource indicator value corresponding to an identifier the\n      resource server expects for itself.  The JWT access token MUST be\n      rejected if \"aud\" does not contain a resource indicator of the\n      current resource server as a valid audience.\n\n   *  The resource server MUST validate the signature of all incoming\n      JWT access tokens according to [RFC7515] using the algorithm\n      specified in the JWT \"alg\" Header Parameter.  The resource server\n      MUST reject any JWT in which the value of \"alg\" is \"none\".  The\n      resource server MUST use the keys provided by the authorization\n      server.\n\n   *  The current time MUST be before the time represented by the \"exp\"\n      claim."
- "   The resource server MUST handle errors as described in Section 3.1 of\n   [RFC6750].  In particular, in case of any failure in the validation\n   checks listed above, the authorization server response MUST include\n   the error code \"invalid_token\"."

### 2.6 OAuth 2.1: an Internet-Draft, not an RFC

**Status.** Everything drawn from this document belongs in the plan's verify list, never in
frozen code: the section numbers and the split between its non-normative §10 list and its
normative body can both change at the next revision.
(sources: https://datatracker.ietf.org/api/v1/doc/document/draft-ietf-oauth-v2-1/?format=json and
https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, 2026-09-16; revision re-confirmed
in this session)

- "rev= 16 time= 2026-09-03T00:22:00Z intended_std_level= None stream= /api/v1/name/streamname/ietf/ expires= 2027-03-07T00:22:00Z title= The OAuth 2.1 Authorization Framework\n\nstate 1: /api/v1/doc/statetype/draft/ | Active |\nstate 150: /api/v1/doc/statetype/draft-iesg/ | I-D Exists | The IESG has not started processing this draft, or has stopped processing it without publication.\nstate 38: /api/v1/doc/statetype/draft-stream-ietf/ | WG Document |"
- "   Internet-Drafts are draft documents valid for a maximum of six months\n   and may be updated, replaced, or obsoleted by other documents at any\n   time.  It is inappropriate to use Internet-Drafts as reference\n   material or to cite them other than as \"work in progress.\""

**What it removes relative to OAuth 2.0**, and the one removal that is normative in the body
rather than in the non-normative §10 list.
(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, 2026-09-16)

- "   A non-normative list of changes from OAuth 2.0 is listed below:\n\n   *  The authorization code grant is extended with the functionality\n      from PKCE [RFC7636] such that the default method of using the\n      authorization code grant according to this specification requires\n      the addition of the PKCE parameters\n\n   *  Redirect URIs must be compared using exact string matching as per\n      Section 4.1.3 of [RFC9700]\n\n   *  The Implicit grant (response_type=token) is omitted from this\n      specification as per Section 2.1.2 of [RFC9700]\n\n   *  The Resource Owner Password Credentials grant is omitted from this\n      specification as per Section 2.4 of [RFC9700]\n\n   *  Bearer token usage omits the use of bearer tokens in the query\n      string of URIs as per Section 4.3.2 of [RFC9700]\n\n   *  Refresh tokens for public clients must either be sender-\n      constrained or one-time use as per Section 4.14.2 of [RFC9700]\n\n   *  The PKCE plain method is removed"
- (§5.1, normative) "   In particular, clients MUST NOT send the access token in a URI query\n   parameter, and resource servers MUST ignore access tokens in a URI\n   query parameter."

**PKCE, tightened beyond RFC 7636.** Under RFC 7636 `code_challenge_method` is OPTIONAL and
defaults to `plain`; under OAuth 2.1 it is REQUIRED and `plain` is forbidden.
(sources: https://www.rfc-editor.org/rfc/rfc7636.txt and
https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, 2026-09-16)

- (RFC 7636 §4.1, §4.2) "   code_verifier = high-entropy cryptographic random STRING using the\n   unreserved characters [A-Z] / [a-z] / [0-9] / \"-\" / \".\" / \"_\" / \"~\"\n   from Section 2.3 of [RFC3986], with a minimum length of 43 characters\n   and a maximum length of 128 characters.\n...\n   S256\n      code_challenge = BASE64URL-ENCODE(SHA256(ASCII(code_verifier)))\n\n   If the client is capable of using \"S256\", it MUST use \"S256\", as\n   \"S256\" is Mandatory To Implement (MTI) on the server."
- (draft-16 §4.1.1) "   \"code_challenge\":  REQUIRED unless the specific requirements of\n      Section 7.5.1 are met.  Code challenge derived from the code\n      verifier.\n\n   \"code_challenge_method\":  REQUIRED, the value S256 or a value defined\n      by a future extension\n...\n   The plain transformation method defined in [RFC7636] is removed and explicitly\n   prohibited in this specification."
- (draft-16 §4.1.1, redirect URI) "   In particular, the authorization server MUST validate the\n   redirect_uri in the request if present, ensuring that it matches one\n   of the registered redirect URIs previously established during client\n   registration (Section 2).  When comparing the two URIs the\n   authorization server MUST ensure that the two URIs are equal, see\n   Section 6.2.1 of [RFC3986], Simple String Comparison, for details."

**The approvals page as an OAuth client.** A server-rendered page whose credentials stay on the
server matches draft-16 §2.1's "web application" profile, which is a *confidential* client — not
§9's browser-based app, whose normative text is still a TODO placeholder in draft-16 and
therefore cannot be relied on.
(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, 2026-09-16)

- "   \"web application\":  A web application is a client running on a web\n      server.  Resource owners access the client via an HTML user\n      interface rendered in a user agent on the device used by the\n      resource owner.  The client credentials as well as any access\n      tokens issued to the client are stored on the web server and are\n      not exposed to or accessible by the resource owner."
- "9.  Browser-Based Apps\n\n   Browser-based apps are clients that run in a web browser, typically\n   written in JavaScript, also known as \"single-page apps\". ...\n\n   TODO: Bring in the normative text of the browser-based apps BCP when\n   it is finalized."
- (§4.1.2, the authorization response) "   \"iss\":  REQUIRED.  The issuer identifier of the authorization server\n      which the client can use to prevent mix-up attacks, if the client\n      interacts with more than one authorization server."
- (§7.10, CSRF) "   The traditional countermeasure is that clients pass a random value,\n   also known as a CSRF Token, in the state parameter that links the\n   request to the redirect URI to the user agent session as described.\n...  The same protection is provided by the code_verifier parameter or the OpenID Connect nonce value.\n\n   *  Clients MUST ensure that the AS supports the code_challenge_method\n      intended to be used by the client.  If an authorization server\n      does not support the requested method, state or nonce MUST be used\n      for CSRF protection instead."

**A tension recorded, not reconciled.** The non-normative §10 list says refresh tokens for public
clients "must" be sender-constrained or one-time use; the normative §4.3 says the AS "SHOULD".
Which governs is a plan decision. Note also that refresh tokens never reach a resource server at
all, so this only bites on the self-hosted-AS branch.
(source: https://www.ietf.org/archive/id/draft-ietf-oauth-v2-1-16.txt, 2026-09-16)

- (§4.3) "   The authorization server MUST verify the binding between the refresh\n   token and client identity whenever the client identity can be\n   authenticated.  When client authentication is not possible, the\n   authorization server SHOULD issue sender-constrained refresh tokens\n   or use refresh token rotation as described in Section 4.3.1."
- (§1.3.2) "   Unlike access tokens, refresh tokens are intended for\n   use only with authorization servers and are never sent to resource\n   servers."

### 2.7 RFC 8414 and RFC 7591: only if willikins hosts its own authorization server

If willikins delegates, these two describe what the chosen provider must already serve, and
become a capability check against that provider's published metadata (section 6). If willikins
hosts its own, they are implementation requirements.

**RFC 8414 required and useful fields, and the same insertion rule as RFC 9728.**
(source: https://www.rfc-editor.org/rfc/rfc8414.txt, 2026-09-16)

- "   issuer\n      REQUIRED.  The authorization server's issuer identifier, which is\n      a URL that uses the \"https\" scheme and has no query or fragment\n      components. ... The issuer identifier is used to prevent authorization server mix-\n      up attacks\n...\n   token_endpoint\n      URL of the authorization server's token endpoint [RFC6749].  This\n      is REQUIRED unless only the implicit grant type is supported.\n...\n   jwks_uri\n      OPTIONAL.  URL of the authorization server's JWK Set [JWK]\n      document. ... This URL MUST use the \"https\" scheme.\n...\n   response_types_supported\n      REQUIRED.  JSON array containing a list of the OAuth 2.0\n      \"response_type\" values that this authorization server supports.\n...\n   code_challenge_methods_supported\n      OPTIONAL.  JSON array containing a list of Proof Key for Code\n      Exchange (PKCE) [RFC7636] code challenge methods supported by this\n      authorization server. ... If omitted, the authorization server\n      does not support PKCE."
- "3.  Obtaining Authorization Server Metadata\n\n   Authorization servers supporting metadata MUST make a JSON document\n   containing metadata as specified in Section 2 available at a path\n   formed by inserting a well-known URI string into the authorization\n   server's issuer identifier between the host component and the path\n   component, if any.  By default, the well-known URI string used is\n   \"/.well-known/oauth-authorization-server\"."

**Two OAuth-2.0-era defaults in these documents are stale under OAuth 2.1 and must not be
inherited.** Any metadata or registration code written from the 8414/7591 text alone encodes a
grant type OAuth 2.1 deletes.
(sources: https://www.rfc-editor.org/rfc/rfc8414.txt and https://www.rfc-editor.org/rfc/rfc7591.txt, 2026-09-16)

- (RFC 8414 §2) "   grant_types_supported\n      OPTIONAL. ... If omitted, the default value is\n      \"[\"authorization_code\", \"implicit\"]\"."
- (RFC 7591 §5) "   Public clients MAY register with an authorization server using this\n   protocol, if the authorization server's policy allows them.  Public\n   clients use a \"none\" value for the \"token_endpoint_auth_method\"\n   metadata field and are generally used with the \"implicit\" grant type."

**RFC 7591 registration essentials**, for reading a provider's DCR behaviour in section 6.
(source: https://www.rfc-editor.org/rfc/rfc7591.txt, 2026-09-16)

- "   token_endpoint_auth_method\n ...\n      *  \"none\": The client is a public client as defined in OAuth 2.0,\n         Section 2.1, and does not have a client secret.\n      ...\n      If unspecified or omitted, the default is \"client_secret_basic\""
- "   To support open registration and facilitate wider interoperability,\n   the client registration endpoint SHOULD allow registration requests\n   with no authorization (which is to say, with no initial access token\n   in the request).  These requests MAY be rate-limited or otherwise\n   limited to prevent a denial-of-service attack on the client\n   registration endpoint."
- "   client_id\n      REQUIRED.  OAuth 2.0 client identifier string.  It SHOULD NOT be\n      currently valid for any other registered client, though an\n      authorization server MAY issue the same client identifier to\n      multiple instances of a registered client at its discretion."

**Unresolved**

- RFC 9700 (OAuth 2.0 Security Best Current Practice) was not fetched. Draft-16 §10 cites it as
  the authority for every removal — §2.1.2, §2.4, §4.3.2, §4.14.2, §4.1.3. Its number and title
  above are quoted from draft-16's own reference list, not from memory, but its text and status
  were not retrieved. Fetch before any of those section numbers is written into the plan.
- RFC 7662 (Token Introspection) was not fetched. It is the governing document if the chosen
  provider issues opaque tokens, in which case the RFC 9068 checks in 2.5 do not apply and every
  MCP request costs a network round trip inside a handler that has a 30 s timeout.
- draft-16 §4.3.1 (refresh token rotation) and §7.5.1 (the narrow conditions under which
  `code_challenge` may be omitted) were not fetched, though both are cited by text that was.
  §7.5.1 is the exact carve-out behind "REQUIRED unless".
- RFC 9449 (DPoP), RFC 8705 (mutual-TLS bound tokens), RFC 9126 (PAR) and RFC 9207 (issuer
  identification) were named by fetched text but not fetched. They bear on the "sender-constrained"
  option and on the REQUIRED `iss` parameter.
- Which canonical resource URI willikins uses. Both candidates are legal and they produce
  different well-known paths (2.2). Frozen the moment it is published.
- Whether the well-known route sits inside or outside the `allowed_hosts` check. An
  unauthenticated client must reach it before it has a token; `/healthz` is the existing
  precedent for a root-level exemption. No fetched RFC settles it.


## 3. `rmcp`'s server-side story

The one-sentence answer: **rmcp's OAuth support is client-side only, in the pinned version and in
the newest one, and willikins' existing bearer middleware already occupies the exact position
where the resource-server check belongs.** Milestone 2c is a willikins build against a
specification, not an integration against an rmcp API, and it is not blocked on an rmcp upgrade.

### 3.1 What is pinned

- `Cargo.lock` pins rmcp 3.3.0; the workspace manifest requests a caret range, so only the lock
  holds it there. `willikins-server` enables `server`, `macros`, `schemars`, `transport-io`,
  `transport-streamable-http-server`, plus `client` and `transport-io` as dev-dependencies. The
  `auth` feature is **not** enabled anywhere in the workspace.
  (source: `Cargo.lock`, `Cargo.toml:70`, `crates/willikins-server/Cargo.toml`, read 2026-09-16)
  - `name = "rmcp"` / `version = "3.3.0"` / `checksum = "b88db56b8ae316560e9e868b6b978ea940f27cb883323fc90e07435e44158f5c"`
  - `rmcp = { workspace = true, features = ["server", "macros", "schemars", "transport-io", "transport-streamable-http-server"] }` and, in the workspace manifest, `rmcp = "3"`

### 3.2 The `auth` feature is the wrong side of the wire

- The feature gates exactly one module, `transport::auth`, whose entire public surface is client
  machinery. There is no server-side token validator, no protected-resource-metadata document and
  no 401 emitter in the export list.
  (source: https://docs.rs/crate/rmcp/3.3.0/source/src/transport.rs, 2026-09-16)
  - "#[cfg(feature = \"auth\")]\npub mod auth;\n...\npub use auth::{\n    AuthClient, AuthError, AuthorizationManager, AuthorizationRequest, AuthorizationSession,\n    AuthorizedHttpClient, ClientCredentialsConfig, CredentialRefreshGuard, CredentialStore,\n    EXTENSION_OAUTH_CLIENT_CREDENTIALS, InMemoryCredentialStore, InMemoryStateStore,\n    OAuthHttpClient, OAuthHttpClientError, OAuthHttpClientFuture, OAuthHttpRedirectPolicy,\n    OAuthHttpRequest, ScopeUpgradeConfig, StateStore, StoredAuthorizationState, StoredCredentials,\n    WWWAuthenticateParams,\n};"
- rmcp's own OAuth document lists only client responsibilities — discovering metadata, registering
  a client, obtaining and refreshing tokens, and *consuming* a `WWW-Authenticate` challenge. None
  of the list is a resource-server duty.
  (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.3.0/docs/OAUTH_SUPPORT.md, 2026-09-16)
  - "## Features\n\n- Full support for OAuth 2.1 authorization flow with PKCE (S256)\n- RFC 8707 resource parameter binding\n- Protected Resource Metadata discovery (RFC 9728)\n- Authorization Server Metadata discovery (RFC 8414 + OpenID Connect)\n- Dynamic client registration (RFC 7591)\n...\n```toml\n[dependencies]\nrmcp = { version = \"0.1\", features = [\"auth\", \"transport-streamable-http-client\"] }\n```"
- A case-insensitive grep across the whole of rmcp 3.3.0's `src/` for
  `www-authenticate|www_authenticate|oauth-protected-resource|protected_resource|resource_metadata|UNAUTHORIZED`
  returns 245 hits, every one in client code, and **zero** under
  `src/transport/streamable_http_server/`.
  (source: https://docs.rs/crate/rmcp/3.3.0/source/src/transport/auth.rs, 2026-09-16)
  - " 194 src/transport/auth.rs\n  17 src/transport/common/unix_socket.rs\n  16 src/transport/common/reqwest/streamable_http_client.rs\n  12 src/transport/streamable_http_client.rs\n   3 src/service/client.rs\n   1 src/transport/common/http_header.rs\n   1 src/transport/common/auth/streamable_http_client.rs\n   1 src/transport/auth/enterprise.rs"
- The crate ships a test named `test_streamable_http_get_stream_auth_challenge`, which is evidence
  the server does *not* emit a challenge: the test is gated on client features and stands up its
  own hand-written axum mock to produce one.
  (source: https://docs.rs/crate/rmcp/3.3.0/source/tests/test_streamable_http_get_stream_auth_challenge.rs, 2026-09-16)
  - "#![cfg(all(\n    feature = \"transport-streamable-http-client\",\n    feature = \"transport-streamable-http-client-reqwest\",\n    not(feature = \"local\")\n))]\n...\n/// Spin up a minimal axum server whose GET handler always responds with the given\n/// status and optional `WWW-Authenticate` header — no MCP logic involved."
- Neither adjacent feature helps. `request-state` is MRTR request-state sealing, unrelated to
  identity; `auth-enterprise-managed` is a client-side token-exchange helper whose own module doc
  disclaims the server's duties.
  (source: https://docs.rs/crate/rmcp/3.3.0/source/src/transport/auth/enterprise.rs, 2026-09-16)
  - "//! Exchanges an enterprise refresh token for an ID-JAG, then for an MCP access\n//! token. ... This module does not discover servers, log in,\n//! persist credentials, or decide when to reauthenticate.\n...\n//! without automatic retries; server-side replay policy remains the server's responsibility."

### 3.3 There is no server-side authorization hook, and none is needed

- `StreamableHttpServerConfig` has ten fields and not one of them is an authorization hook: no
  `auth` field, no rejection callback, no trait object for pre-dispatch policy.
  (source: https://docs.rs/crate/rmcp/3.3.0/source/src/transport/streamable_http_server/tower.rs, 2026-09-16)
  - "impl Default for StreamableHttpServerConfig {\n    fn default() -> Self {\n        Self {\n            sse_keep_alive: Some(Duration::from_secs(15)),\n            sse_retry: Some(Duration::from_secs(3)),\n            legacy_session_mode: true,\n            json_response: false,\n            cancellation_token: CancellationToken::new(),\n            allowed_hosts: vec![\"localhost\".into(), \"127.0.0.1\".into(), \"::1\".into()],\n            allowed_origins: vec![],\n            session_store: None,\n            max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES,\n            stateless_protocol_metadata_required: false,\n        }\n    }\n}"
- The only rejection rmcp performs before dispatch is Host and Origin validation, the first
  statement of `handle`. It is a DNS-rebinding defence, not an authorization point, and it is not
  extensible.
  (source: same `tower.rs`, 2026-09-16)
  - "    pub async fn handle<B>(&self, request: Request<B>) -> Response<BoxBody<Bytes, Infallible>>\n ...\n    {\n        if let Err(response) =\n            validate_dns_rebinding_headers(request.uri(), request.headers(), &self.config)\n        {\n            return response.into_response();\n        }\n        let method = request.method().clone();"
- willikins' bearer layer sits entirely outside rmcp and entirely before it: the rmcp service is
  nested as a plain tower service under `/mcp`, and *that nested router* is wrapped in the auth
  middleware. So the order is timeout → tracing → `auth::bearer_auth` → rmcp
  `StreamableHttpService::call` → `handle` → host check → dispatch → handler. rmcp's own host
  check runs **after** willikins' authentication, and applies only inside the `/mcp` nest — which
  is why a protected-resource-metadata route will sit outside it exactly as `/healthz` does today.
  A 2c resource-server check replaces the body of `bearer_auth` at this seam; no rmcp API is
  required or available for it.
  (source: `crates/willikins-server/src/http/mod.rs:96-100`, read 2026-09-16)
  - `let mcp_service = build_mcp_service(butler, config);` / `let mcp_router = Router::new().nest_service("/mcp", mcp_service).layer(` / `    axum::middleware::from_fn_with_state(Arc::clone(&auth_tokens), auth::bearer_auth),` / `);`

### 3.4 How the principal reaches a handler — and what rides along with it

- rmcp injects the full `http::request::Parts` (the request minus its consumed body: headers, URI,
  method and the tower/axum extensions map) into `rmcp::model::Extensions`, and documents exactly
  the pattern willikins uses. This is the supported mechanism for carrying an authenticated
  identity from a tower middleware into a tool handler, and it needs no rmcp change for 2c.
  (source: https://docs.rs/crate/rmcp/3.3.0/source/src/transport/streamable_http_server/tower.rs, 2026-09-16)
  - "/// ## Accessing HTTP request data from tool handlers\n///\n/// The service consumes the request body but injects the remaining\n/// [`http::request::Parts`] into [`crate::model::Extensions`], which is\n/// accessible through [`crate::service::RequestContext`].\n...\n///     let parts = ctx.extensions.get::<http::request::Parts>().unwrap();\n///     let state = parts.extensions.get::<AppState>().unwrap();"
- The `part` value is inserted **unmodified**, at four sites. Consequence worth a plan decision:
  because the full headers ride along, the `Authorization` header — today the agent's raw bearer
  token, under 2c an OAuth access token — is readable inside every tool handler. The plan should
  decide whether the auth middleware strips it before `next.run`, so only the derived principal
  crosses the boundary, or whether a live token in handler scope is accepted. This bears on the
  trust boundaries and on the redaction-by-construction invariant.
  (source: same `tower.rs`, 2026-09-16)
  - "                    let peer_info = Self::peer_info_for_stateless_request(&request, &part.headers);\n                    request.request.extensions_mut().insert(part);"
- Every reachable handler entry carries a principal today, and 2c inherits that: willikins sets
  `legacy_session_mode(false)` with a session manager that has no event store, and rmcp's dispatch
  then allows POST only — GET and DELETE return 405 before any handler — while the POST path
  always inserts `Parts`.
  (sources: `crates/willikins-server/src/mcp.rs` and the rmcp `tower.rs` above, 2026-09-16)
  - (willikins) "    fn principal_for(\n        &self,\n        extensions: &rmcp::model::Extensions,\n    ) -> Result<PrincipalId, CallToolResult> {\n        let from_request = extensions\n            .get::<Parts>()\n            .and_then(|parts| parts.extensions.get::<PrincipalId>())\n            .cloned();"
  - (rmcp) "        let allowed_methods = match (self.config.legacy_session_mode, supports_stateless_replay) {\n            (true, _) => \"GET, POST, DELETE\",\n            (false, true) => \"GET, POST\",\n            (false, false) => \"POST\",\n        };"

### 3.5 rmcp 3.4.0 exists and does not change the answer

- 3.4.0 is the newest published version and is not yanked; MSRV stays 1.88 for both, matching the
  workspace floor. Re-confirmed in this session against the crates.io sparse index.
  (source: https://index.crates.io/rm/cp/rmcp, 2026-09-16)
  - "3.2.0 yanked= False rust_version= 1.88\n3.3.0 yanked= False rust_version= 1.88\n3.4.0 yanked= False rust_version= 1.88"
- Its two authorization entries are both client-side. Two entries do touch the server transport,
  neither of them authorization.
  (source: https://api.github.com/repos/modelcontextprotocol/rust-sdk/releases/tags/rmcp-v3.4.0, 2026-09-16)
  - "- *(auth)* let url origin decide prm discovery ([#1264])\n...\n- *(auth)* ignore non-metadata JSON when probing for protected resource metadata ([#1204])\n...\n- *(streamable-http-server)* map handler-generated HeaderMismatch to HTTP 400 ([#1259])\n- *(http)* enforce Origin validation semantics ([#1192])"
- The same grep for `www-authenticate|oauth|UNAUTHORIZED|bearer|protected_resource` over 3.4.0's
  2187-line server `tower.rs` returns nothing. Upgrading does not move the resource-server work
  into rmcp.
  (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/crates/rmcp/src/transport/streamable_http_server/tower.rs, 2026-09-16)
  - "$ grep -n -i 'www-authenticate|oauth|UNAUTHORIZED|bearer|protected_resource' tower340.rs\n(no output)\n$ wc -l tower340.rs\n    2187 tower340.rs"
- 3.4.0 does add one thing a browser-reachable deployment may want: an `enforce_origin_validation()`
  builder that validates Origin even when the allowed-origins list is empty. Today willikins passes
  only `allowed_hosts` and leaves `allowed_origins` at its empty default, so rmcp performs **no**
  Origin check on `/mcp`; willikins' own Origin/Referer check lives on `/approvals` only.
  (source: same 3.4.0 `tower.rs`, 2026-09-16)
  - "    /// Enable Origin validation, including when the allowed Origins list is empty.\n    pub fn enforce_origin_validation(mut self) -> Self {\n        self.validate_empty_origin_allowlist = true;\n        self\n    }"
- An rmcp bump is **not** a pure lock change. 3.4.0 deprecates the `ServerInfo` alias, willikins
  uses it at three sites, and the workspace gate runs clippy with `-D warnings`, so a deprecation
  warning is a hard failure.
  (sources: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/crates/rmcp/src/model.rs
  and `crates/willikins-server/src/mcp.rs`, 2026-09-16)
  - "/// Deprecated alias for [`ServerConfig`].\n...\n#[deprecated(note = \"use `ServerConfig` instead\")]\npub type ServerInfo = InitializeResult;"
  - `crates/willikins-server/src/mcp.rs:55` (the import), `:702` (`fn get_info(&self) -> ServerInfo`), `:721` (`ServerInfo::new(...)`)

**Unresolved**

- Whether `cargo update -p rmcp --precise 3.4.0` resolves cleanly and the four gates pass. No
  cargo command was permitted in this research. The `ServerInfo` → `ServerConfig` rename at three
  sites is the only breakage identifiable statically; 3.4.0 also mentions "route peer cancellation
  by lifecycle" and "run first pre-init request in service loop", whose effect on willikins'
  stateless configuration was not verified. Treat the upgrade as its own verification step.
- Whether willikins should adopt `enforce_origin_validation()` on `/mcp` once a public domain
  exists, and what `allowed_origins` should then contain.
- Whether the auth middleware should strip the `Authorization` header before `next.run`. rmcp
  verifiably passes the full headers into every handler; no willikins code strips it today.
- Whether server-side authorization support is in flight upstream. Releases, tags and the
  changelog were checked (the `[Unreleased]` section on `main` is empty today); open pull requests
  and issues were not.


## 4. Rust crates, with a pin table

Measured against the workspace's declared MSRV of **1.88** and edition 2024, and against a tree
whose only HTTP client is `ureq` 3.4.2 (see "Two corrections to the brief", above). Every
crates.io fact below is from `https://crates.io/api/v1/crates/<name>` with the raw JSON fragment
quoted; every source fact is from the crate's own git tag or from the published `.crate` tarball,
which is what cargo actually builds. No cargo command was run: the task forbade it, so **no weight
or build-time claim below is measured** — they are dependency counts.

### 4.1 Pin table

| Crate | Pin | Version fetched | MSRV / edition | Licence | Status for 2c |
| --- | --- | --- | --- | --- | --- |
| `jsonwebtoken` | `{ version = "11", default-features = false, features = ["<backend>"] }` | 11.0.0, 2026-07-24 | 1.88.0 / 2024 | MIT | Take. Validates the access token. Backend choice unresolved (4.3) |
| `axum-extra` | `{ version = "0.12", default-features = false, features = ["cookie-signed"] }` | 0.12.6, 2026-04-14 | 1.80 / 2021 | MIT | Take. Session cookie for the approvals login. Needs no axum bump |
| JWKS fetch + cache | none — hand-rolled over the tree's `ureq` 3.4.2 inside `spawn_blocking`, parsed into `jsonwebtoken::jwk::JwkSet` | n/a | n/a | n/a | Build. `jsonwebtoken` has no HTTP client (4.2) |
| `josekit` | — | 0.10.3, 2025-05-20 | none declared / 2021 | — | Reject: non-optional `openssl ^0.10.68`, i.e. `openssl-sys` |
| `jwt-simple` | — | 0.13.1, 2026-08-19 | 1.85 / 2018 | — | Reject: BoringSSL by default, and audience unchecked by default |
| `jwks-client` | — | 0.2.0, 2020-01-13 | none / 2018 | — | Reject: abandoned over six years |
| `oauth2` | — | 5.0.0, 2025-01-21 | 1.65 / 2021 | — | Defer: every bundled HTTP client mismatches; `base64` ceiling unsatisfiable |
| `openidconnect` | — | 4.0.1, 2025-07-06 | 1.65 / 2021 | — | Defer: 23 non-optional deps duplicating four majors |
| `tower-sessions` | — | 0.15.0, 2026-02-01 | none declared / 2021 | — | Defer: needs a session store this server does not have. Copy its cookie defaults |
| `reqwest` | — | 0.13.5, 2026-09-08 | — | — | Reject: a second, async HTTP+TLS stack in an image that has none |

Licence and MSRV cells come from the crates.io version metadata, fetched with a User-Agent (the
API returns a non-JSON body without one).
(sources: https://crates.io/api/v1/crates/jsonwebtoken/11.0.0 and
https://crates.io/api/v1/crates/axum-extra/0.12.6, 2026-09-16)

- "jsonwebtoken 11.0.0 license= MIT rust_version= 1.88.0 edition= 2024"
- "axum-extra 0.12.6 license= MIT rust_version= 1.80 edition= 2021"

Already in `Cargo.lock` and reusable at no new cost: `axum` 0.8.9, `axum-core` 0.5.6, `http`
1.5.0, `hyper` 1.11.1, `tokio` 1.53.1, `tower` 0.5.3, `tower-http` 0.7.1, `rmcp` 3.3.0, `rustls`
0.23.44, `ring` 0.17.14, `webpki-roots` 1.0.9, `ureq` 3.4.2, `cookie` 0.18.2, `url` 2.5.8,
`base64` 0.23.1, `sha2` 0.11.0, `subtle`, `zeroize` 1.9.0, `rand` 0.9.5 and 0.10.2, `serde_json`
1.0.151, `chrono` 0.4.45. The lock holds 336 packages (verified in this session). Genuinely new if
adopted: `jsonwebtoken`, `axum-extra`, and whatever a backend pulls.

### 4.2 `jsonwebtoken` 11: the facts that decide how it is configured

- Version, MSRV and edition. MSRV is exactly the workspace floor, with zero headroom: a future
  patch that raises it raises the workspace's declared `rust-version`.
  (source: https://crates.io/api/v1/crates/jsonwebtoken, 2026-09-16)
  - "{\"num\": \"11.0.0\", \"created_at\": \"2026-07-24T10:59:32.987519Z\", \"rust_version\": \"1.88.0\", \"edition\": \"2024\", \"yanked\": false}"
- **Audience validation fails closed when the token carries `aud` and the verifier configured
  none** — but a token with *no* `aud` at all still passes, because `required_spec_claims` defaults
  to `{"exp"}` only. A resource server must therefore do both: `set_audience(...)` **and**
  `set_required_spec_claims(&["exp","aud","iss"])`, plus `set_issuer(...)`.
  (source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/validation.rs, 2026-09-16)
  - "    if !options.validate_aud {\n        return Ok(());\n    }\n    match (claims.aud, options.aud.as_ref()) {\n        // Each principal intended to process the JWT MUST\n        // identify itself with a value in the audience claim. ...\n        (TryParse::Parsed(Audience::Multiple(aud)), None) if !aud.is_empty() => {\n            return Err(new_error(ErrorKind::InvalidAudience));\n        }\n        (TryParse::Parsed(_), None) => {\n            return Err(new_error(ErrorKind::InvalidAudience));\n        }"
- The `Validation` defaults, in full. Note `validate_nbf` is **false** and the algorithm list
  defaults to HS256 — three explicit overrides are mandatory for an OAuth resource server.
  (source: same `validation.rs`, 2026-09-16)
  - "        Validation {\n            required_spec_claims: required_claims,\n            algorithms,\n            leeway: 60,\n            reject_tokens_expiring_in_less_than: 0,\n\n            validate_exp: true,\n            validate_nbf: false,\n            validate_aud: true,\n\n            iss: None,\n            sub: None,\n            aud: None,\n        }"
- The algorithm list is a real allowlist and it stops `alg` confusion: the header algorithm is
  checked against it before a verifier is built, an empty list errors, and the verifier's key
  family must match every allowed algorithm's family.
  (source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/decoding.rs, 2026-09-16)
  - "    if !validation.algorithms.contains(&header.alg) {\n        return Err(new_error(ErrorKind::InvalidAlgorithm));\n    }\n...\n    if validation.algorithms.is_empty() {\n        return Err(new_error(ErrorKind::MissingAlgorithm));\n    }\n\n    for alg in &validation.algorithms {\n        if verifying_provider.algorithm().family() != alg.family() {\n            return Err(new_error(ErrorKind::InvalidAlgorithm));\n        }\n    }"
- It parses a JWKS and selects by `kid` natively, so no extra crate is needed for that half. Note
  `find` matches only keys that *have* a `kid`.
  (source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/jwk.rs, 2026-09-16)
  - "pub struct JwkSet {\n    pub keys: Vec<Jwk>,\n}\n\nimpl JwkSet {\n    /// Find the key in the set that matches the given key id, if any.\n    pub fn find(&self, kid: &str) -> Option<&Jwk> {\n        self.keys\n            .iter()\n            .find(|jwk| jwk.common.key_id.is_some() && jwk.common.key_id.as_ref().unwrap() == kid)\n    }\n}"
- It has **no HTTP client at all**, so fetching the JWKS, caching it and re-fetching on an unknown
  `kid` — with a rate limit, so an attacker cannot force one fetch per request — is application
  code the plan must own.
  (source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/Cargo.toml, 2026-09-16)
  - "[dependencies]\nbase64 = \"0.22\"\nserde = { version = \"1.0.228\", features = [\"derive\"] }\nserde_json = \"1.0\"\nsignature = { version = \"2.2.0\", features = [\"std\"] }\n\n# For PEM decoding\npem = { version = \"3\", optional = true }\nsimple_asn1 = { version = \"0.6\", optional = true }"
- Maintenance: the crate's own badge says `passively-maintained`, which reads worse than the
  activity — 11.0.0 shipped 2026-07-24 and the newest commit at fetch time was 2026-09-15. Both
  10.0.0 and 11.0.0 were breaking, so the pin should be reviewed each milestone, not floated.
  (source: https://api.github.com/repos/Keats/jsonwebtoken/commits?per_page=5, 2026-09-16)
  - "2026-09-15T21:23:15Z Add insecure_decode_claims (#539)\n2026-07-24T10:58:56Z Bump v11"

### 4.3 The one unresolved decision is a Dockerfile question, not a crate question

Since 10.0.0 a crypto backend **must** be selected; with neither, the process has no provider.

- (source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/README.md, 2026-09-16)
  - "Two crypto backends are available via features, `aws_lc_rs` and `rust_crypto`, at most one of which must be enabled. If you select neither feature, you need to provide your own `CryptoProvider`."
- (source: https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/crypto/mod.rs, 2026-09-16)
  - "            const NOT_INSTALLED_ERROR: &str = r\"\nCould not automatically determine the process-level CryptoProvider from jsonwebtoken crate features.\nCall CryptoProvider::install_default() before this point to select a provider manually, or make sure exactly one of the 'rust_crypto' and 'aws_lc_rs' features is enabled."

Neither choice is free.

- `rust_crypto` is pure Rust and builds in the current image unchanged, but pulls `rsa` 0.9, which
  carries a permanently unpatched advisory, plus duplicate `sha2` 0.10, `base64` 0.22 and a third
  `rand` major.
  (source: https://crates.io/api/v1/crates/jsonwebtoken/11.0.0/dependencies, 2026-09-16)
  - "  aws-lc-rs              ^1.15.0      opt=True\n  ed25519-dalek          ^2.1.1       opt=True\n  hmac                   ^0.12.1      opt=True\n  p256                   ^0.13.2      opt=True\n  p384                   ^0.13.0      opt=True\n  pem                    ^3           opt=True\n  rand                   ^0.8.5       opt=True\n  rsa                    ^0.9.6       opt=True\n  sha2                   ^0.10.7      opt=True"
- The advisory has **no fixed version**, deliberately. Scope matters: the leak is of the *private*
  key through decryption or signing timing, and a resource server only verifies with public keys
  from a JWKS, so it does not apply to this use. The cost is process, not exploitability — any
  `cargo-audit` or `cargo-deny` gate flags it forever and needs a documented ignore.
  (source: https://raw.githubusercontent.com/rustsec/advisory-db/main/crates/rsa/RUSTSEC-2023-0071.md, 2026-09-16)
  - "[versions]\npatched = []\n...\n### Impact\nDue to a non-constant-time implementation, information about the private key is leaked through timing information which is observable over the network.\n\n### Patches\nNo patch is yet available.\n...\nStill affected as of 2026-09-12: rsa 0.9.10 (latest stable) and rsa 0.10.0-rc.18 (latest). `patched = []` is intentional."
- `aws_lc_rs` avoids the advisory and most duplicates but wants a C/C++ compiler the image
  deliberately does not install. CMake is *not* required for a non-FIPS build; a C++ compiler may
  be, and the Dockerfile installs only `gcc` and `libc6-dev`, explicitly not `build-essential`.
  Choosing this backend means editing the Dockerfile and invalidating its own comment.
  (sources: https://raw.githubusercontent.com/aws/aws-lc-rs/main/aws-lc-rs/README.md and the
  repository `Dockerfile`, 2026-09-16)
  - "Consuming projects will need a C/C++ compiler to build.\n\n**Non-FIPS builds (default):**\n* CMake is **never** required\n* Bindgen is **never** required (pre-generated bindings are provided)\n* Go is **never** required"
  - (Dockerfile) "# The workspace's only native dependency is `ring` (confirmed with\n# `cargo tree -i ring`, which shows exactly one path down through\n# `rustls` -> `ureq` -> `willikins-providers-http`; no `aws-lc-sys`,\n# `openssl-sys`, or `cmake` anywhere in the tree)"

RUSTSEC has no advisory directory at all for `jsonwebtoken`, `oauth2`, `openidconnect`,
`tower-sessions`, `axum-extra`, `josekit`, `jwt-simple` or `aws-lc-rs` (all 404 under
`rustsec/advisory-db/crates`); `cookie` has only RUSTSEC-2017-0005, long predating 0.18.

### 4.4 The approvals-page session cookie

- `axum-extra` 0.12.6 requires `axum ^0.8.9` and `axum-core ^0.5.2`, and the lock holds 0.8.9 and
  0.5.6 exactly — no version bump needed. It offers `CookieJar` (plaintext), `SignedCookieJar`
  (signed and verified, value still readable by the client) and `PrivateCookieJar` (encrypted and
  authenticated). For a cookie carrying only an opaque session id, signed is enough; private is
  the right pick if the cookie itself carries subject and expiry.
  (source: https://raw.githubusercontent.com/tokio-rs/axum/axum-extra-v0.12.6/axum-extra/src/extract/cookie/signed.rs, 2026-09-16)
  - "/// Extractor that grabs signed cookies from the request and manages the jar.\n///\n/// All cookies will be signed and verified with a [`Key`]. Do not use this to store private data\n/// as the values are still transmitted in plaintext.\n///\n/// Note that methods like [`SignedCookieJar::add`], [`SignedCookieJar::remove`], etc updates the\n/// [`SignedCookieJar`] and returns it. This value _must_ be returned from the handler as part of\n/// the response for the changes to be propagated."

  That last sentence is a documented footgun — a jar not returned never sets the cookie — and is
  worth an acceptance test.
- `cookie` 0.18.2 is already in the lock (pulled by `ureq`'s `cookie_store`), so the crate itself
  is free and only the crypto features are new. `Secure`, `HttpOnly` and `SameSite` are plain
  builder methods.
  (source: https://static.crates.io/crates/cookie/cookie-0.18.2.crate, 2026-09-16)
  - "    pub fn secure(mut self, value: bool) -> Self {\n        self.cookie.set_secure(value);\n        self\n    }\n...\n    pub fn http_only(mut self, value: bool) -> Self {\n        self.cookie.set_http_only(value);\n        self\n    }\n...\n    pub fn same_site(mut self, value: SameSite) -> Self {\n        self.cookie.set_same_site(value);\n        self\n    }"
- `tower-sessions` is not recommended, but its defaults are and are quoted so they can be copied:
  `http_only: true`, `same_site: Strict`, `secure: true`, path `/`, name `id` with a citation to
  OWASP on session-id name fingerprinting. Its cost is a session store — the only bundled one is
  in-memory, which loses every session on a redeploy.
  (source: https://static.crates.io/crates/tower-sessions/tower-sessions-0.15.0.crate, 2026-09-16)
  - "impl Default for SessionConfig<'_> {\n    fn default() -> Self {\n        Self {\n            name: \"id\".into(), /* See: https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-name-fingerprinting */\n            http_only: true,\n            same_site: SameSite::Strict,\n            expiry: None, // TODO: Is `Max-Age: \"Session\"` the right default?\n            secure: true,\n            path: \"/\".into(),\n            domain: None,\n            always_save: false,\n        }\n    }\n}"

### 4.5 Why the OAuth client crates are deferred

- `oauth2` 5.0.0's three built-in HTTP clients all mismatch this tree — `reqwest ^0.12` (crates.io
  stable is 0.13.5), `ureq ^2` (the tree has 3.4.2, a different major with a different API),
  `curl ^0.4` (native libcurl, breaks the image) — and its `base64 >=0.21, <0.23` constraint
  **cannot** unify with the tree's `base64` 0.23.1, guaranteeing a duplicate. It also pins
  `thiserror ^1.0` against the workspace's 2. The maintainer is moving the reqwest integration to
  a companion crate that has no stable release.
  (source: https://crates.io/api/v1/crates/oauth2/5.0.0/dependencies, 2026-09-16)
  - "  base64                 >=0.21, <0.23 opt=False\n  http                   ^1.0         opt=False\n  rand                   ^0.8         opt=False\n  sha2                   ^0.10        opt=False\n  thiserror              ^1.0         opt=False\n  url                    ^2.1         opt=False\n  curl                   ^0.4.0       opt=True\n  reqwest                ^0.12        opt=True\n  ureq                   ^2           opt=True"
- `openidconnect` 4.0.1 is a 23-crate non-optional dependency list duplicating four majors the
  workspace already pins higher, and pulls `rsa` — the advisory crate again — for two HTTP round
  trips of discovery.
  (source: https://crates.io/api/v1/crates/openidconnect/4.0.1/dependencies, 2026-09-16)
  - "  base64                 ^0.21        opt=False\n  ed25519-dalek          ^2.0.0       opt=False\n  itertools              ^0.10        opt=False\n  oauth2                 ^5.0.0       opt=False\n  p256                   ^0.13.2      opt=False\n  p384                   ^0.13.0      opt=False\n  rand                   ^0.8.5       opt=False\n  rsa                    ^0.9.2       opt=False\n...\n  thiserror              ^1.0         opt=False"
- `josekit` depends non-optionally on `openssl ^0.10.68`, contradicting the rustls-only,
  distroless, no-CA-bundle image directly.
  (source: https://crates.io/api/v1/crates/josekit/0.10.3/dependencies, 2026-09-16)
  - "  openssl                ^0.10.68     opt=False"
- `jwt-simple` is rejected on two independent grounds: BoringSSL is in its default feature set,
  and its default verification options leave the audience unchecked — the opposite of
  `jsonwebtoken`'s fail-closed behaviour.
  (source: https://static.crates.io/crates/jwt-simple/jwt-simple-0.13.1.crate, 2026-09-16)
  - "[features]\ncwt = [\"ciborium\"]\ndefault = [\n    \"optimal\",\n    \"jwe\",\n]\njwe = []\noptimal = [\"boring\"]\npure-rust = []"
  - "    /// Require the audience to be present in the set\n    pub allowed_audiences: Option<HashSet<String>>,\n...\n            allowed_issuers: None,\n            allowed_audiences: None,"
- `jwks-client`'s only recent version is 0.2.0 from 2020-01-13.
  (source: https://crates.io/api/v1/crates/jwks-client, 2026-09-16)
  - "{\"num\": \"0.2.0\", \"created_at\": \"2020-01-13T11:59:58.199582Z\", \"rust_version\": null, \"edition\": \"2018\", \"yanked\": false}"

**Unresolved**

- Which `jsonwebtoken` backend actually builds in the repository's Dockerfile. No cargo command
  was permitted, so neither was test-built. Verify by building both in the image before freezing
  the feature; if `aws_lc_rs` wins, the Dockerfile's "no `aws-lc-sys`, `openssl-sys`, or `cmake`
  anywhere in the tree" comment stops being true and must be amended.
- Whether `oauth2` 5.0.0 exposes a bring-your-own-HTTP-client trait an adapter over `ureq` 3 could
  implement. It depends on `http ^1.0`, which the tree satisfies, but the trait signature was not
  fetched — so "adapter over ureq 3" is speculation, not a finding.
- Whether `cargo-audit` or `cargo-deny` is or will be part of the gate. If so, `rust_crypto`
  brings a permanent RUSTSEC-2023-0071 finding needing a documented ignore.
- Compiled size and build time for each option, on an 11 GB host building with `-j 2`. Every
  weight statement above is a dependency count, not a measurement.
- The `SameSite` value for the approvals session cookie. `Strict` is safest and is
  `tower-sessions`' default, but it breaks a redirect that lands on a page needing the cookie
  immediately; `Lax` is the usual answer. A plan decision informed by the exact redirect flow.


## 5. Railway exposure

Method note. `docs.railway.com` publishes a markdown twin for every page at the same path plus
`.md`, indexed by `https://docs.railway.com/llms.txt`. The `docs.railway.com/reference/*` and
`/guides/*` paths cited in `docs/research/2026-09-12-m2-dependencies.md` now 404 or 308: the site
was reorganised under `/networking/...`, `/deployments/...`, `/infrastructure-as-code/...`.
Seventeen pages were fetched on 2026-09-16 and every quote below is from those twins.

### 5.1 The prior decision, and where the hostname is not written down

- The service deliberately has no public domain and no `domains` key at all until 2c lands. The
  header comment also states why omitting the key is safe.
  (source: `.railway/railway.ts:30-33`, read 2026-09-16)
  ```
  // Domains. None, and no `domains` key at all: the operator deleted the service's
  // public domain on 2026-09-15 and the service gets none until milestone 2c's
  // OAuth lands (docs/HANDOFF.md, "RESUME HERE"). Generated Railway domains are
  // never part of this file, so omitting the key removes nothing.
  ```

This note deliberately records **no generated hostname**. The service has none at present, and any
hostname recalled from an earlier session is memory, not a fetched fact; it belongs in section 7.

### 5.2 What the edge does to a request

- TLS is terminated at the anycast-nearest edge POP, not in the deployment region, and the edge
  adds headers on the way through.
  (source: https://docs.railway.com/networking/edge-networking.md, 2026-09-16)
  - "2. **Edge Processing**: The edge proxy (tcp-proxy) terminates TLS, adds headers, and looks up routing information\n3. **Internal Routing**: Traffic is forwarded over Railway's internal network to your deployment"
- Six headers are documented. `X-Forwarded-For` does **not** appear on any of the seventeen pages
  fetched (`grep -ric` returned zero) — `X-Real-IP` is the documented client-IP header. That is an
  absence in the documentation, not proof the header is absent on a real request.
  (source: https://docs.railway.com/networking/public-networking/specs-and-limits.md, 2026-09-16)
  - "- `X-Real-IP` for identifying client's remote IP.<br/>- `X-Forwarded-Proto` always indicates `https`.<br/>- `X-Forwarded-Host` for identifying the original host header.<br/>- `X-Railway-Edge` for identifying the edge [POP](https://status.railway.com/locations) that handled the request.<br/>- `X-Request-Start` for identifying the time the request was received (Unix milliseconds timestamp).<br/>- `X-Railway-Request-Id` for correlating requests against network logs.<br/>- `X-Railway-Debug` can be sent by clients with any value to receive extra debug response headers"
- **What `Host` reads at the service is not stated by any Railway page.** The only evidence is
  indirect and it points both ways: `X-Forwarded-Host` exists "for identifying the original host
  header", which implies a rewrite, but nothing says what `Host` is. This is the single most
  consequential unknown for `WILLIKINS_ALLOWED_HOSTS`, for rmcp's `allowed_hosts`, and for the
  approvals Origin check once a public domain exists. Measure it; do not freeze it.
  (source: same specs-and-limits page, 2026-09-16)
  - "- `X-Forwarded-Host` for identifying the original host header."
- Timeouts and caps. The service's own 30 s timeout is far tighter than any of these, and its 1 MiB
  body cap is the only body-size limit in the path: no fetched page names a byte cap on request
  bodies (`grep -niE "body size|request body|max body|payload"` over the specs page returned
  nothing).
  (source: same specs-and-limits page, 2026-09-16)
  - "- Support for HTTP/1.1 and HTTP/2.<br/>- Support for websockets over HTTP/1.1.<br/>- Idle HTTP/1.1 connections are closed after 60 seconds between requests. This does not apply to HTTP/2 or websocket connections.<br/>- Max 32 KB combined header size.<br/>- HTTP requests can run for up to 15 minutes if data keeps transferring ... and are otherwise closed after 5 minutes with no data transferred.<br/>- Request bodies must finish uploading within 5 minutes."
- **There is no per-IP rate limit at the edge.** All three limits are per-domain or per-connection
  aggregates in the ten-thousands, so per-IP limiting stays the service's job — and would have to
  key off `X-Real-IP`, whose trustworthiness is unsettled.
  (source: same specs-and-limits page, 2026-09-16)
  - "| **Maximum Connections**     | 10,000 concurrent connections | The number of concurrent connections.                     |\n| **HTTP Requests/Sec**       | 11,000~ RPS                   | The number of HTTP requests to a given domain per second. |\n| **Requests Per Connection** | 10,000 requests               | The number of requests each connection can make.          |"
- **A plain-HTTP POST to port 80 is silently converted to a GET.** For an MCP POST to `/mcp` or a
  form POST to `/approvals` sent over `http://`, the request changes method rather than failing
  loudly. All traffic must be HTTPS with TLS 1.2 or above, and SNI is mandatory.
  (source: same specs-and-limits page, 2026-09-16)
  - "- Plain HTTP GET requests will be redirected to HTTPS with a `301` response.\n- Plain HTTP POST requests will be converted to GET requests."
  - "All traffic must be HTTPS and use TLS 1.2 or above, and TLS SNI is mandatory for requests."

### 5.3 Getting a domain, and what that costs

- A generated domain is not automatic: the operator clicks Generate Domain or runs `railway domain`
  with no argument. One Railway-provided domain per service; custom domains are limited per plan.
  (source: https://docs.railway.com/cli/domain.md, 2026-09-16)
  - "Creates a free `*.up.railway.app` domain for your service."
  - "- One Railway-provided domain per service\n- Multiple custom domains can be added per service"
- A custom domain needs **both** a CNAME and a TXT ownership record. With only the CNAME, requests
  return 404 even after it resolves.
  (source: https://docs.railway.com/networking/domains/working-with-domains.md, 2026-09-16)
  - "**Important:** If the `TXT` record is missing, requests to your custom domain will return a `404` error even after the `CNAME` resolves. Railway uses the `TXT` record to confirm domain ownership before routing traffic, so your service will not be reachable on the custom domain until both records are in place and verified."
- Railway provisions Let's Encrypt certificates automatically, 90-day validity renewed at 30 days,
  and gives up after 72 hours. External certificates are not supported.
  (source: https://docs.railway.com/networking/public-networking/specs-and-limits.md, 2026-09-16)
  - "| **Certificate Issuance** | - Railway attempts to issue a certificate for **up to 72 hours** after domain creation before failing.<br/>- Certificates are expected to be issued within an hour. |\n| **TLS** | - Support for TLS 1.2 and TLS 1.3 with specific cipher sets.<br/>- Certificates are valid for 90 days and renewed when 30 days of validity remain."
- Cloudflare in front requires SSL/TLS mode **Full**, not Full (Strict), and Railway may be unable
  to issue a certificate at all while proxying is on.
  (source: https://docs.railway.com/networking/domains/working-with-domains.md, 2026-09-16)
  - "For proxied domains (Cloudflare orange cloud), we may not always be able to issue a certificate for the domain, but Cloudflare to Railway traffic will be encrypted with TLS using the default Railway `*.up.railway.app` certificate."
- A browser cannot reach the private network. This is the structural reason the approvals browser
  login needs public exposure at all.
  (source: same working-with-domains page, 2026-09-16)
  - "- Client-side requests from browsers **cannot** reach the private network - they must go through a public domain."

### 5.4 Declaring the domain in the IaC file

- The `domains` field takes either a bare hostname or `{ domain, port }`, and an empty array is a
  legal declared value.
  (source: https://docs.railway.com/infrastructure-as-code/reference.md, 2026-09-16)
  - "Attach custom domains to a service:\n\n```ts\nconst web = service(\"web\", {\n  domains: [\"app.example.com\"],\n});\n```\n\nSpecify a target port:\n\n```ts\nconst api = service(\"api\", {\n  domains: [{ domain: \"api.example.com\", port: 3000 }],\n});\n```"
- **A generated domain cannot be declared in the file at all**, which is the primary source behind
  the repository header's claim.
  (sources: https://docs.railway.com/infrastructure-as-code/reference.md and
  https://docs.railway.com/infrastructure-as-code.md, 2026-09-16)
  - "Generated Railway service domains are not included in `.railway/railway.ts`."
  - "The importer ... omits platform defaults, leaves out generated Railway domains, avoids internal IDs, and renders existing variable values as `preserve()` so they stay on Railway instead of being written into source."
- **Whether an omitted `domains` key deletes an already-attached *custom* domain is not stated.**
  The general contract is "omit means delete"; the documented exemption covers only *generated*
  domains. So the repository header's "omitting the key removes nothing" is established for a
  generated domain and is an untested inference for a custom one. Declare the custom domain
  explicitly and read the plan before applying.
  (source: https://docs.railway.com/infrastructure-as-code.md, 2026-09-16)
  - "Keep every service for a Railway environment in a single `.railway/railway.ts` (or `.py` / `.go`) file. That is the supported shape: one project definition, one apply, omit means delete."
- The apply that adds the domain is guarded: `plan` is read-only and redacts values, `apply`
  re-plans and is rejected on drift, and destructive changes need `--confirm-destructive`
  non-interactively.
  (source: same infrastructure-as-code page, 2026-09-16)
  - "Destructive changes, such as deleting a service or variable, are marked before confirmation. Review those lines carefully before continuing. Non-interactively (with `--yes`, `--json`, or in an agent session), destructive changes additionally require `--confirm-destructive`, so a stray `--yes` cannot remove resources on its own"

### 5.5 Two edge features that do not fit, and one that nearly does

- **Under Attack Mode cannot protect a public `/mcp`.** The browser check is shown only to browser
  navigations; every other request is turned away, so a domain serving an API has all its traffic
  blocked. It could protect a browser-only approvals host, but `/mcp` and `/approvals` share a
  service today.
  (source: https://docs.railway.com/networking/waf.md, 2026-09-16)
  - "- The check is only shown to browser navigations (a `GET` request whose `Accept` header includes `text/html`). API calls and other non-navigation requests are turned away instead, so protecting a domain that serves only an API blocks all of its traffic."
- **CDN caching is off by default and an `Authorization` header always bypasses it** — but note the
  gap for a cookie-session approvals page: a bare `Cookie` *request* header does not disable
  caching and cookies are not part of the cache key, so a personalized `GET /approvals` would need
  `Set-Cookie` or `Cache-Control: private` to stay out of a shared cache if caching were ever
  enabled.
  (source: https://docs.railway.com/networking/cdn.md, 2026-09-16)
  - "- The request has no `Authorization` header. Authenticated requests bypass the cache and go to your service.\n\nA request `Cookie` header doesn't disable caching. This keeps pages cacheable for sites that set analytics cookies (for example, PostHog or Google Analytics). Personalized responses are handled on the response side instead"
- **Edge rules can fence `/approvals` by IP before the request reaches the service**, but Client IP
  matching is IPv4-only, and they need a public domain plus a plan allowance.
  (source: https://docs.railway.com/networking/edge-rules.md, 2026-09-16)
  - "| Client IP | is, is not, is in, is not in | Matches an IPv4 address or CIDR range, such as `203.0.113.7` or `203.0.113.0/24`. Requests without an IPv4 source don't match. |\n| Host | is, is not, is in, matches | Matches the lowercase, canonical hostname without a port. |"
  - "Edge rules require a public domain and a plan with an edge-rule allowance."

### 5.6 What already holds regardless of exposure

- The healthcheck's hostname is `healthcheck.railway.app`, Railway queries until any 2xx and does
  not monitor afterwards, and a service with an attached volume still has brief downtime on
  redeploy — which applies here.
  (source: https://docs.railway.com/deployments/healthchecks.md, 2026-09-16)
  - "Railway uses the hostname `healthcheck.railway.app` when performing healthchecks on your service. This is the domain from which the healthcheck requests will originate.\n\nFor applications that restrict incoming traffic based on the hostname, you'll need to add `healthcheck.railway.app` to your list of allowed hosts."
- Internal traffic is `<service-name>.railway.internal` over encrypted Wireguard tunnels, scoped to
  one project and environment, runtime-only.
  (source: https://docs.railway.com/networking/private-networking/how-it-works.md, 2026-09-16)
  - "Note: When communicating internally, use `http://` rather than `https://` since traffic is already encrypted via Wireguard."
- The app must bind `0.0.0.0` on the injected `PORT`, or the edge answers 502.
  (source: https://docs.railway.com/networking/troubleshooting/application-failed-to-respond.md, 2026-09-16)
  - "Your web server should bind to the host `0.0.0.0` and listen on the port specified by the `PORT` environment variable, which Railway automatically injects into your application."

For orientation, `crates/willikins-server/src/` reads **none** of Railway's edge headers today —
no reference to `X-Forwarded-*`, `X-Real-IP` or `Forwarded` anywhere in the crate — so every
per-IP or forwarded-host idea in 2c starts from zero.

**Unresolved**

- What `Host` the service receives on public traffic. Not documented. Measure against a live
  deployment by echoing the inbound `Host`, `X-Forwarded-Host` and `X-Railway-Edge` before writing
  any allowed-hosts value into frozen code.
- Whether the edge overwrites a client-supplied `X-Real-IP`, `X-Forwarded-Host` or
  `X-Forwarded-Proto`, or appends to it. Stated for none of the three; `X-Forwarded-Proto` "always
  indicates `https`" reads as a guarantee, but there is no equivalent sentence for the other two.
  Until measured, none is spoof-proof.
- Whether the edge-to-deployment hop is plain HTTP or re-encrypted. Three facts point at plaintext,
  one at Wireguard; no sentence settles it. Decides whether `Secure`-cookie and HSTS decisions can
  rely on the socket rather than on `X-Forwarded-Proto`.
- Whether an omitted `domains` key deletes an attached custom domain. Settle with a read-only
  `railway config plan` after the operator attaches one; never by applying.
- Whether the live environment's private DNS is dual-stack or IPv6-only (it depends on whether the
  environment predates 2025-10-16), which matters because edge rules cannot match a request with
  no IPv4 source.
- What edge-rule allowance the current plan carries; the limit is shown only in the rule editor.


## 6. Identity providers, one row per provider

No provider is recommended or ranked here; that is the operator's decision. This section states
what each one does, with the caveats attached.

**The two must-haves, defined before anything is judged against them**, because "audience binding"
and "a registration path" both have a loose reading that would let everything pass.

- **Audience binding, as the MCP specification requires it**, means the authorization server
  honours the RFC 8707 `resource` parameter that the client MUST send on both the authorization
  and the token request, and reflects it into the token's audience (section 1.2, section 2.3). A
  provider that binds an audience only through a *different*, proprietary parameter still produces
  an audience-restricted token, but the client has to be told to send that parameter instead — so
  a stock MCP client's `resource` does nothing.
- **A registration path**, for the case the specification calls most common — "when client and
  server have no prior relationship" — means Client ID Metadata Documents or an open (no prior
  credential) Dynamic Client Registration endpoint. Pre-registration is a documented mechanism too,
  but it requires an admin to act before a new client can ever connect.

| Provider | Audience binding | Registration path | Discovery + JWKS | Access-token format | Hosting / licence / cost |
| --- | --- | --- | --- | --- | --- |
| **Auth0** | `audience` parameter carrying the registered API Identifier → `aud`. `resource` is **not** in its documented `/authorize` parameter list (absence, not a documented rejection) [6.1] | DCR at `/oidc/register`, off by default per tenant; when on, **open** registration with no access token. CIMD: not found either way [6.2] | OIDC discovery + `/.well-known/jwks.json`, plus an RFC 8414 alias [6.3] | JWT when issued for a registered custom API; opaque otherwise [6.4] | SaaS only. Free $0/mo, 25,000 MAU, 1 custom domain [6.5] |
| **Zitadel** | Reserved scope `urn:zitadel:iam:org:project:id:{id}:aud`. **RFC 8707 `resource` is accepted and ignored**; does not narrow `aud` [6.6] | DCR (RFC 7591 + 7592), off by default; token mode default, **open** mode available and documented as what MCP needs [6.7] | OIDC discovery + `/oauth/v2/keys`. **No RFC 8414 endpoint** [6.8] | JWT **or** opaque, a per-application "Auth Token Type" setting; default not established [6.9] | Self-host or cloud. AGPL-3.0-only with carve-outs. Free $0/mo at 100 DAU; Pro $100/mo from 25,000 DAU [6.10] |
| **Keycloak** | **RFC 8707 not supported** — "cannot recognize `resource` parameter". Documented workaround binds `aud` through an Audience mapper driven by `scope` [6.11] | DCR supported; CIMD supported but **experimental**, behind `--features=cimd`. Initial access tokens are the recommended route; anonymous registration only via Client Registration Policies [6.12] | OIDC discovery + `/protocol/openid-connect/certs`; RFC 8414 listed as supported [6.13] | JWKS described as verifying "any JSON Web Token"; the MCP guide's example token is a JSON claim set with `aud` and `scope`. No sentence states it outright [6.14] | Self-host only. Apache-2.0, no licence cost [6.15] |
| **authentik** | `resource` **rejected** with `invalid_target` at the token-exchange endpoint; `audience` used instead. `aud` = the provider's `client_id` (from source) [6.16] | DCR added in 2026.8 (OIDC DCR 1.0 + RFC 7591); **requires a Bearer token** and policy pass, so not open by default. No RFC 7592 [6.17] | OIDC discovery + JWKS, both **per application** under `/application/o/<slug>/` [6.18] | Signed JWT — **but only if a Signing Key is selected**; with none, HS256 with the client secret and no public key in JWKS [6.19] | Self-host. MIT with carve-outs. Free tier; Enterprise $5/user/mo [6.20] |
| **Logto** | **RFC 8707, by name.** API resources registered with a resource indicator; the `resource` parameter must exactly match it [6.21] | **CIMD** ("Dynamic app"): the `client_id` is a public HTTPS URL, nothing pre-created, aimed explicitly at MCP. No RFC 7591 endpoint found [6.22] | OIDC discovery at `/oidc/.well-known/openid-configuration`; JWKS URI and issuer not customisable [6.23] | JWT for tokens issued against a registered API resource; the no-`resource` case not established [6.24] | Self-host or cloud. MPL-2.0. Free $0/mo to 50,000 MAU; Pro from $24/mo with RBAC/Orgs/MFA as add-ons [6.25] |
| **Ory Hydra** | `audience` parameter validated against a per-client allow-list; not RFC 8707 `resource` [6.26] | DCR at `/oauth2/register` plus `/oauth2/register/{id}` management; a configuration toggle [6.27] | `/.well-known/openid-configuration` + `/.well-known/jwks.json` [6.28] | **Opaque by default**; JWT opt-in globally or per client, and Ory's own schema calls the JWT choice "a bad idea" [6.29] | Self-host, Apache-2.0. **Not an identity provider**: no user management, no login UI — a login and consent app is the operator's to build [6.30] |
| **GitHub OAuth apps** | **None.** Neither `resource` nor `audience` appears in the documented `/login/oauth/authorize` parameters; a token is valid against the whole API, not a named resource [6.31] | **Pre-registration only** — a human registers an app in account settings. No `/register` endpoint, no CIMD documented [6.32] | **None published for user OAuth.** Live probes: 404 on both well-known paths at `github.com` and `api.github.com` [6.33] | **Opaque** `gho_`-prefixed string with `token_type=bearer`; cannot be validated offline [6.34] | SaaS, GitHub-operated; registering an app costs nothing [6.35] |

Read across the two must-have columns: **Logto is the only provider surveyed that both honours
RFC 8707 `resource` and offers a no-prior-relationship registration path (CIMD).** Auth0 and
Zitadel offer open DCR but bind the audience through a non-`resource` mechanism. Keycloak and
authentik neither honour `resource` (Keycloak cannot recognise it; authentik rejects it at the
endpoint examined) nor offer open registration by default. Ory Hydra has DCR but uses `audience`
and leaves the entire login UI to the operator. GitHub OAuth apps fail both must-haves and publish
no discovery document, so an MCP client relying on metadata discovery has nothing to discover.
That is a statement of fetched facts, not a recommendation: an operator who controls the client
can send whatever parameter a provider wants, which changes the weight of the first column
entirely.

### The facts behind the table

**6.1** (source: https://auth0.com/docs/oas/authentication/authentication-api-oas.json, 2026-09-16)
- "{\"name\": \"audience\", \"in\": \"query\", \"required\": false, \"description\": \"The unique identifier of the target API you want to access. This is the **API Identifier** found in your API settings.\\n\\n**When to use:** Include this when requesting an Access Token to call a specific API.\", \"schema\": {\"type\": \"string\"}, \"example\": \"https://api.example.com\"}"
- The full documented `/authorize` parameter set is `response_type, client_id, redirect_uri, scope, state, audience, code_challenge, code_challenge_method, nonce, connection, prompt, organization, invitation, login_hint, acr_values, max_age, ui_locales, response_mode, dpop_jkt`. `resource` is not among them.
- The identifier itself is frozen at creation. (source: https://auth0.com/docs/get-started/auth0-overview/set-up-apis.md, 2026-09-16)
  - "A unique identifier for the API. Auth0 recommends using a URL. ... The URL does not have to be a publicly available URL. Auth0 will not call your API. This value cannot be modified afterwards."

**6.2** (source: https://auth0.com/docs/get-started/applications/dynamic-client-registration.md, 2026-09-16)
- "By default, Dynamic Client Registration is disabled for all tenants. To enable Dynamic Client Registration, use the Auth0 Dashboard or Management API.\n...\nTo dynamically register an application, make a `POST` request to the `/oidc/register` endpoint. Because Auth0 supports Open Dynamic Registration, the `/oidc/register` endpoint accepts registration requests without an access token."

**6.3** (source: https://auth0.com/docs/get-started/applications/configure-applications-with-oidc-discovery.md, 2026-09-16)
- "You can configure applications with the [OpenID Connect (OIDC)](https://openid.net/specs/openid-connect-discovery-1_0.html) discovery documents found at: `https://{yourDomain}/.well-known/openid-configuration`.\n...\n  \"jwks_uri\": \"https://{yourDomain}.us.auth0.com/.well-known/jwks.json\",\n...\nIf your application or SDK references the [OAuth RFC-8414](https://www.rfc-editor.org/rfc/rfc8414) Authorization Server Metadata specification, you can use the OAuth alias to fetch metadata about the IdP: `/.well-known/oauth-authorization-server`."

**6.4** (source: https://auth0.com/docs/secure/tokens/access-tokens.md, 2026-09-16)
- "Access tokens issued for the Management API and access tokens issued for any custom API that you have registered with Auth0 follow the JWT standard ... They are self-contained therefore it is not necessary for the recipient to call a server to validate the token.\n...\nIf validation of your custom API access token fails, make sure it was issued with your custom API as the `audience`."

**6.5** (source: https://auth0.com/pricing, 2026-09-16; extracted from the static HTML, not a rendered value)
- "Free $ 0 / month No credit card needed to sign up. ... Up to 25,000 monthly active users Includes 1 Custom Domain* ... 5 Organizations ... Community Support 1 Enterprise Connection"

**6.6** (sources: https://raw.githubusercontent.com/zitadel/zitadel/v4.17.3/apps/docs/content/apis/openidoauth/scopes.mdx and .../guides/integrate/dynamic-client-registration.mdx, 2026-09-16)
- "| `urn:zitadel:iam:org:project:id:{projectid}:aud`  | `urn:zitadel:iam:org:project:id:69234237810729019:aud` | By adding this scope, the requested project id will be added to the audience of the access token |"
- "- Dynamically registered clients share the audience of the `ZITADEL DCR` project. A JWT access token issued to one of\n  them carries every client ID registered in that project, plus the project ID, in its `aud` claim. The `resource`\n  parameter ([RFC 8707](https://datatracker.ietf.org/doc/html/rfc8707)) is accepted on the authorization code flow but\n  ignored, so it does not narrow `aud`. Do not rely on `aud` alone to tell one dynamically registered client from\n  another; validate the `client_id` or `azp` claim instead."
- Issue zitadel/zitadel#12710, "Support OAuth 2.0 Resource Indicators (RFC 8707)", was open at fetch time (`gh api`, 2026-09-16).

**6.7** (sources: .../apis/openidoauth/endpoints.mdx and .../guides/integrate/dynamic-client-registration.mdx at v4.17.3, 2026-09-16)
- "The registration_endpoint implements [OAuth 2.0 Dynamic Client Registration (RFC 7591)](https://datatracker.ietf.org/doc/html/rfc7591).\nIt lets clients register themselves as OIDC applications at runtime, which is required for example by [Model Context Protocol (MCP)](/guides/integrate/dynamic-client-registration) clients.\n...\nThe endpoint is disabled by default."
- "| Open registration | `dynamicClientRegistration.allowUnauthenticated = true` | None. Required for the MCP flow. | The instance's default organization. |"

**6.8** (source: .../apis/openidoauth/endpoints.mdx at v4.17.3, 2026-09-16)
- "The OpenID Connect Discovery Endpoint is located within the issuer domain.\nThis would give us `${CUSTOM_DOMAIN}/.well-known/openid-configuration`.\n...\n`${CUSTOM_DOMAIN}/oauth/v2/keys`\n\nThe endpoint returns a JSON Web Key Set (JWKS) containing the public keys that can be used to locally validate JWTs you received from ZITADEL.\n...\n**ZITADEL** does not yet provide a OAuth 2.0 Metadata endpoint but instead provides a [OpenID Connect Discovery Endpoint]"

**6.9** (sources: .../apis/openidoauth/endpoints.mdx, .../apis/openidoauth/claims.mdx and .../guides/integrate/services/google-cloud.mdx at v4.17.3, 2026-09-16)
- "| access_token  | An `access_token` as JWT or opaque token |"
- "- **Auth Token Type**: JWT"
- "| aud | `69234237810729019` | The audience of the token, by default all client id's and the project id are included |"

**6.10** (sources: https://raw.githubusercontent.com/zitadel/zitadel/v4.17.3/LICENSING.md and https://zitadel.com/pricing, 2026-09-16)
- "This repository is licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0-only). ... The following files and directories ... are licensed under the [Apache License 2.0]:\n\n```\nproto/\napps/docs/\n```"
- "FREE ... US$ 0 /Month ... 100 Daily Active Users ... PRO Our cloud plan that scales with your needs. US$ 100 /Month Everything in Free Start with 25'000 Daily Active Users per month included"

**6.11** (source: https://raw.githubusercontent.com/keycloak/keycloak/26.7.3/docs/guides/securing-apps/mcp-authz-server.adoc, 2026-09-16)
- "| https://datatracker.ietf.org/doc/html/rfc8707[Resource Indicators for OAuth 2.0 (RFC 8707)]\n| MUST\n| MUST\n| MUST\n| -\n| Not supported"
- "The MCP specification does not describe how to do this binding. One method for the binding is to set a value of `resource` parameter to an `aud` claim in an access token. However, {project_name} cannot recognize `resource` parameter.\n\nThe Keycloak community is planning to support Resource Indicators for OAuth 2.0 (RFC 8707) ... Until this support is completed, you can use OAuth 2.0's `scope` parameter instead of the `resource` parameter."
- Its own conformance table, which is Keycloak's claim about MCP and not a claim fetched from the MCP specification: "| https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization[2025-11-25]\n| Partially Supported without Resource Indicators for OAuth 2.0"

**6.12** (sources: .../mcp-authz-server.adoc and .../client-registration.adoc at 26.7.3, 2026-09-16)
- "{project_name} supports OAuth Client ID Metadata Document. ... WARNING: The OAuth Client ID Metadata Document support is an experimental feature in {project_name}. As such, it may introduce breaking changes in future versions of {project_name}. To enable it, start {project_name} with `--features=cimd`."
- "The Client Registration Service endpoint is `/realms/<realm>/clients-registrations/<provider>`.\n...\nTo invoke the Client Registration Services you usually need a token. ... There is an alternative to register new client without any token as well, but then you need to configure Client Registration Policies"

**6.13** (source: .../partials/oidc/available-endpoints.adoc and .../con-server-oidc-uri-endpoints.adoc at 26.7.3, 2026-09-16)
- "The endpoint is:\n\n....\n/realms/{realm-name}/.well-known/openid-configuration\n...."
- "/realms/{realm-name}/protocol/openid-connect/certs::\n  Used for the JSON Web Key Set (JWKS) containing the public keys used to verify any JSON Web Token (jwks_uri)"
- (the RFC 8414 row of the same MCP guide's standards table) "| https://datatracker.ietf.org/doc/html/rfc8414[OAuth 2.0 Authorization Server Metadata (RFC 8414)]\n| MUST (or OpenID Connect Discovery 1.0)\n| MUST\n| MUST\n| MUST\n| Supported"

**6.14** (source: .../mcp-authz-server.adoc at 26.7.3, 2026-09-16)
- "```json\n{\n  ...\n  \"aud\": \"https://example.com/mcp\",\n  \"scope\": \"mcp:resources mcp:tools mcp:prompts\"\n  ...\n}\n```"

**6.15** (source: https://raw.githubusercontent.com/keycloak/keycloak/26.7.3/LICENSE.txt, 2026-09-16)
- "                                 Apache License\n                           Version 2.0, January 2004"

**6.16** (sources: https://raw.githubusercontent.com/goauthentik/authentik/version/2026.8.2/website/docs/add-secure-apps/providers/oauth2/token_exchange.mdx and .../authentik/providers/oauth2/id_token.py, 2026-09-16)
- "authentik rejects requests containing `resource` with `invalid_target` rather than ignoring the parameter. This prevents clients from incorrectly assuming that authentik applied the requested restriction. Use `audience` to identify the target provider instead.\n...\nThe token uses the target provider's issuer as `iss` and its `client_id` as `aud`."
- (from source, not prose docs) "        id_token.aud = provider.client_id"
- Scope caveat: that rejection is documented for the RFC 8693 token-exchange endpoint. The fetched docs say nothing about `resource` at `/authorize`.

**6.17** (source: .../providers/oauth2/dynamic-client-registration.mdx at 2026.8.2, 2026-09-16)
- "authentik implements dynamic client registration as defined by:\n\n- [OpenID Connect Dynamic Client Registration 1.0](https://openid.net/specs/openid-connect-registration-1_0.html)\n- [RFC 7591: OAuth 2.0 Dynamic Client Registration Protocol](https://datatracker.ietf.org/doc/html/rfc7591)\n...\n- authentik's DCR implementation does not provide RFC 7592 client-management endpoints"
- "If the user is not authorized to register a client, authentik returns `403 Forbidden` with the error code `access_denied`.\n\nA successful registration is therefore not an anonymous or open registration unless the configured policies explicitly allow that behavior."

**6.18** (source: .../providers/oauth2/index.mdx at 2026.8.2, 2026-09-16)
- "| JWKS                 | `/application/o/<application_slug>/jwks/`                            |\n| OpenID Configuration | `/application/o/<application_slug>/.well-known/openid-configuration` |\n...\nGlobal issuer mode still serves the discovery document at `https://authentik.company/application/o/<application_slug>/.well-known/openid-configuration`, not at the root issuer URL."

**6.19** (sources: .../providers/oauth2/machine_to_machine.mdx and .../providers/oauth2/index.mdx at 2026.8.2, 2026-09-16)
- "This will return a JSON response with an `access_token`, which is a signed JWT token. This token can be sent along with requests to other hosts, which can then validate the JWT based on the signing key configured in authentik."
- "When no **Signing Key** is selected, authentik uses `HS256` and the provider's **Client secret** to sign JWTs. This does not publish a public signing key through JWKS; select a certificate key pair to use asymmetric signing, such as `RS256`."

**6.20** (sources: https://raw.githubusercontent.com/goauthentik/authentik/version/2026.8.2/LICENSE and https://goauthentik.io/pricing/, 2026-09-16)
- "* All content that resides under the \"authentik/enterprise/\" directory of this repository, if that directory exists, is licensed under the license defined in \"authentik/enterprise/LICENSE\".\n...\n* Content outside of the above mentioned directories or restrictions above is available under the \"MIT\" license as defined below."
- "Open Source For homelab users and simple use cases ... Community Discord No support Free ... Enterprise ... $5 / user / month $0.02 / external user* / month Billed annually. ... Enterprise Plus ... Starting at $20k annually"

**6.21** (source: https://raw.githubusercontent.com/logto-io/docs/master/docs/authorization/global-api-resources.mdx, 2026-09-16)
- "Logto models API resources according to [RFC 8707: Resource Indicators for OAuth 2.0](https://www.rfc-editor.org/rfc/rfc8707.html). A **resource indicator** is a URI that uniquely identifies the target API or service being requested.\n...\n- Resource indicators enable audience-restricted tokens and support for multi-API architectures.\n...\nThe `resource` parameter must exactly match the API identifier (resource indicator) you registered in Logto."

**6.22** (source: https://raw.githubusercontent.com/logto-io/docs/master/docs/integrate-logto/third-party-applications/dynamic-apps.mdx, 2026-09-16)
- "Dynamic app allows OAuth clients to connect to your tenant without pre-registration. Instead of a client ID issued by Logto, the client uses a public HTTPS URL as its `client_id`. The URL serves a JSON document describing the client, called the [client ID metadata document (CIMD)]. ...\n\nDynamic app implements the IETF draft [OAuth Client ID Metadata Document](https://www.ietf.org/archive/id/draft-ietf-oauth-client-id-metadata-document-02.html)."
- "Dynamic app requires the [OIDC provider SSRF protection] ... Self-hosted instances that disable it cannot enable dynamic app."
- A `gh api search/code` for `registration_endpoint` in `logto-io/logto` returned 0 results on 2026-09-16 — evidence of absence, not a documented statement.

**6.23** (source: https://raw.githubusercontent.com/logto-io/docs/master/docs/authorization/validate-access-tokens/fragments/_retrieve-info-about-logto-tenant.md, 2026-09-16)
- "These values can be retrieved from Logto's OpenID Connect discovery endpoint:\n\n```\nhttps://<your-logto-endpoint>/oidc/.well-known/openid-configuration\n```\n...\n```json\n{\n  \"jwks_uri\": \"https://your-tenant.logto.app/oidc/jwks\",\n  \"issuer\": \"https://your-tenant.logto.app/oidc\"\n}\n```"

**6.24** (source: https://raw.githubusercontent.com/logto-io/docs/master/docs/authorization/validate-access-tokens/README.mdx, 2026-09-16)
- "Validating access tokens is a critical part of enforcing [role-based access control (RBAC)](/authorization/role-based-access-control) in Logto. This guide walks you through verifying Logto-issued JWTs in your backend/API, checking for signature, issuer, audience, expiration, permissions (scopes), and organization context."

**6.25** (sources: https://raw.githubusercontent.com/logto-io/logto/v1.43.0/LICENSE and https://logto.io/pricing, 2026-09-16)
- "Mozilla Public License Version 2.0"
- "Free $0/mo For starting out and trying Logto, no credit card required. Up to 50,000 MAU 50K tokens ... Pro Best Value From $24/mo For production and teams. ... Add-ons, billed separately RBAC Organizations (Multi-tenancy) MFA Enterprise SSO SAML & third-party apps"

**6.26** (source: https://raw.githubusercontent.com/ory/docs/master/docs/hydra/guides/audiences.mdx, 2026-09-16)
- "To specify the intended audiences for an OAuth 2.0 access token, the OAuth 2.0 client needs to proactively define the audiences it\nneeds access to when creating or updating the client. ...\nWhen performing an OAuth 2.0 Authorization Code Grant ... developers can request audiences at the\n`/oauth2/auth` endpoint using the `audience` query parameter\n...\nThe values are validated against the allowed audiences defined in the OAuth 2.0 client."

**6.27** (sources: https://raw.githubusercontent.com/ory/hydra/v26.2.0/spec/api.json and https://raw.githubusercontent.com/ory/docs/master/docs/hydra/guides/oauth2-clients.mdx, 2026-09-16)
- "/oauth2/register\n/oauth2/register/{id}"
- "OpenID Dynamic Client Registration enables automatic registration of OAuth2 clients with the authorization server. ... To enable OpenID\nDynamic Client Registration, use the Ory CLI:\n\n```shell\nory patch oauth2-config --project <project-id> --workspace <workspace-id>\n  --replace \"/oidc/dynamic_client_registration/enabled=true\"\n```"

**6.28** (source: https://raw.githubusercontent.com/ory/hydra/v26.2.0/spec/api.json, 2026-09-16)
- "/.well-known/jwks.json\n/.well-known/openid-configuration"

**6.29** (source: https://raw.githubusercontent.com/ory/hydra/v26.2.0/spec/config.json, 2026-09-16)
- "{\"type\": \"string\", \"description\": \"Defines access token type. jwt is a bad idea, see https://www.ory.sh/docs/oauth2-oidc/jwt-access-token\", \"enum\": [\"opaque\", \"jwt\"], \"default\": \"opaque\"}"

**6.30** (source: https://raw.githubusercontent.com/ory/hydra/v26.2.0/README.md, 2026-09-16)
- "Ory Hydra is a hardened, OpenID Certified OAuth 2.0 Server and OpenID Connect\nProvider ... It connects to your existing identity provider through a login and\nconsent app, giving you absolute control over the user interface and experience.\n...\n- Be a standalone OAuth 2.0 and OpenID Connect server without user management\n- Connect to any existing identity provider through a login and consent app"
- (source: https://raw.githubusercontent.com/ory/hydra/v26.2.0/LICENSE, 2026-09-16) "                                 Apache License\n                           Version 2.0, January 2004\n                        http://www.apache.org/licenses/"

**6.31** (source: https://raw.githubusercontent.com/github/docs/main/content/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps.md, 2026-09-16; the quote keeps GitHub's Liquid placeholders, which are in the raw source)
- "| Query parameter | Type | Required? | Description |\n| --------------- | ---- | --------- | ----------- |\n| `client_id`|`string` | Required | The client ID you received from GitHub when you {% ifversion fpt or ghec %}[registered](https://github.com/settings/applications/new){% else %}registered{% endif %}. |\n| `redirect_uri`|`string` |Strongly recommended| The URL in your application where users will be sent after authorization. ... |\n| `login` | `string` | Optional| Suggests a specific account to use for signing in and authorizing the app. |\n| `scope`|`string` |Context dependent| A space-delimited list of [scopes](/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps)."

**6.32** (source: https://raw.githubusercontent.com/github/docs/main/content/apps/oauth-apps/building-oauth-apps/creating-an-oauth-app.md, 2026-09-16)
- "{% data reusables.user-settings.developer_settings %}\n{% data reusables.user-settings.oauth_apps %}\n1. Click **New OAuth App**."

**6.33** Live probes, re-run in this session (`curl -sS -o /dev/null -w "%{http_code}" -L`, 2026-09-16). These are observations, not quotes from a document; see section 7.
- `https://github.com/.well-known/openid-configuration` → 404
- `https://github.com/.well-known/oauth-authorization-server` → 404
- `https://api.github.com/.well-known/openid-configuration` → 404
- `https://api.github.com/.well-known/oauth-protected-resource` → 404
- `https://token.actions.githubusercontent.com/.well-known/openid-configuration` → 200 — the GitHub Actions workload-identity issuer, which is all the probe shows; it is not the issuer for user login.

**6.34** (sources: https://raw.githubusercontent.com/github/docs/main/content/authentication/keeping-your-account-and-data-secure/about-authentication-to-github.md and .../authorizing-oauth-apps.md, 2026-09-16)
- "| OAuth access token | `gho_` | [AUTOTITLE](/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) |"
- "access_token=gho_16C7e42F292c…178B4a (masked) \n&scope=repo%2Cgist\n&token_type=bearer"

**2026-09-16 note:** the worked `gho_` token above is quoted from GitHub's own OAuth
documentation with its random run elided by an ellipsis — a deliberate departure from this
document's "fetched verbatim" rule, the same one `docs/research/2026-09-12-m2-dependencies.md`
took for Doppler's three worked examples. GitHub's published secret-scanning pattern list marks
every one of its six token prefixes (`ghp_`, `github_pat_`, `gho_`, `ghu_`, `ghs_`, `ghr_`)
push-protected, so the unmasked value is exactly what a scanner looks for, and this file is
prose: it cannot `concat!`-split a literal apart the way the Rust siblings of such examples do.
Whether GitHub's own detector would clear this particular string on a checksum is not known and
is not the test — the rule this repository enforces (`crates/willikins-core/tests/secret_literal_guard.rs`)
is about shape, because a push blocked on shape is not something to clear with a bypass.

**6.35** No quotable sentence was fetched stating GitHub's hosting model or that registering an
OAuth app is free; see section 7.

**Unresolved**

- Auth0 and RFC 8707: `resource` is absent from the fetched OpenAPI parameter list, which is not
  the same as a documented rejection, and no Auth0 page stating a position on RFC 8707 or on MCP
  resource indicators was found. Auth0's `ai-agents-mcp` documentation section was not read in
  full and may state one.
- Auth0 and CIMD: no page found either way.
- Zitadel's default access-token format. The docs say "JWT or opaque" and expose a per-application
  setting; no fetched page states the default for a new application.
- Zitadel and CIMD: issue zitadel/zitadel#12316 was open at fetch time, suggesting it is
  unimplemented, but no documentation page confirms it.
- Keycloak: no sentence states outright that its access tokens are JWTs; the conclusion rests on
  the JWKS endpoint description and the MCP guide's example claim set.
- Keycloak: whether RFC 8707 support has landed since 26.7.3. The guide says "planning"; no issue
  or pull request number was located.
- authentik: whether `resource` is rejected, ignored or honoured at `/authorize`. The verbatim
  rejection is from the token-exchange guide only. The claims an access-token JWT carries are not
  enumerated in the docs; `aud = provider.client_id` is read from source.
- authentik: no hosted offering was confirmed either way.
- Logto: whether a token requested without a `resource` parameter is a JWT. No page found.
- Logto: the CIMD pages were fetched from the docs repository's `master`, not pinned to v1.43.0,
  so the feature may be newer than the current release.
- Ory: no pricing number could be quoted — `https://www.ory.sh/pricing` renders plan text with
  zero-width-obfuscated markup. And the quoted DCR toggle targets Ory Network; whether self-hosted
  Hydra uses the same configuration key was not established.
- GitHub Apps' user-to-server tokens (`ghu_`) share the same `/login/oauth` endpoints and token
  format, but no page was fetched on whether GitHub Apps change any of the six answers. GitHub
  Enterprise Server was not investigated.


## 7. Verify with a browser before relying on them

Everything here is either a claim that could not be fetched verbatim, or a point where two readers
disagreed. Where they disagreed, the source was re-fetched in this session and the entry says
which reading the fetched text supports. Nothing in this section may enter frozen code.

### 7.1 Disagreements, re-fetched and settled

- **Where the resource-server audience MUST lives.** One reader quoted the MCP specification
  ("MCP servers **MUST** validate that access tokens were issued specifically for them as the
  intended audience, according to RFC 8707 Section 2"); another read RFC 8707 itself and reported
  that neither it nor RFC 9728 states a resource-server MUST. **Both are supported by the fetched
  text, at different layers.** Re-fetched: the MCP 2025-11-25 `.md` carries that sentence at line
  474, and RFC 8707's own §2 says only "The authorization server SHOULD audience-restrict issued
  access tokens". The RS-side MUST is therefore the MCP profile's, plus RFC 9068 §4 for JWT-format
  tokens. Written out in section 2.4 so the plan does not re-open it.
- **Which OAuth 2.1 draft the specification's section numbers resolve to.** One reader noted the
  2026-07-28 page cites draft-13 throughout and draft-14 once; another quoted draft-16 section
  numbers throughout. Re-fetched and counted: 2025-11-25 cites draft-13 eighteen times and nothing
  else; 2026-07-28 cites draft-13 nine times and draft-14 once; datatracker reports the current
  revision as **16** (2026-09-03, expires 2027-03-07). The numbering is stable for the section
  that matters most — draft-13 §5.2 is "Access Token Validation" and names RFC 7662 and RFC 9068,
  the same content as draft-16 §5.2 — but it is not guaranteed stable elsewhere. **Treat every
  OAuth 2.1 section number as a verify item**, and pin the draft revision in any citation.
- **Which protocol revision willikins is anchored to.** Re-verified directly: rmcp 3.3.0's
  `ProtocolVersion::LATEST` is `V_2025_11_25`, and `crates/willikins-server/src/mcp.rs:722`
  advertises `V_2026_07_28`. Both readings were right; they describe different things. The
  divergence itself is real and is an open plan decision (section 1.3).
- **`reqwest` in the tree and the MSRV.** The task brief asserted both; a reader contradicted both.
  Verified locally in this session: `grep -c 'name = "reqwest"' Cargo.lock` → `0`; `ureq` is at
  3.4.2; `Cargo.toml:21` declares `rust-version = "1.88"`. The reader is right and the brief was
  wrong on both counts.
- **rmcp 3.4.0's existence and MSRV.** Re-confirmed against the crates.io sparse index: 3.4.0,
  not yanked, `rust_version` 1.88, with 3.3.0 immediately before it.

### 7.2 Claims that could not be fetched verbatim

- **GitHub publishes no OIDC discovery or authorization-server metadata for user OAuth.** This is
  a live-probe observation, not a quotable document: 404 on `/.well-known/openid-configuration`
  and `/.well-known/oauth-authorization-server` at `github.com`, and on
  `/.well-known/openid-configuration` and `/.well-known/oauth-protected-resource` at
  `api.github.com`; 200 on the GitHub Actions workload-identity issuer, which is a different
  issuer. Re-run in this session; still an absence, and an absence is weaker evidence than a
  statement. Confirm in a browser before writing "GitHub publishes no metadata" into a plan.
- **GitHub's hosting model and the cost of registering an OAuth app.** No sentence was fetched.
  The claim "SaaS, GitHub-operated, free to register" is inference from the surrounding pages.

### 7.3 Facts nobody could settle, which the plan must carry as verify items

- **What `Host` header the service receives on public Railway traffic.** Not documented anywhere
  on `docs.railway.com`. It decides what `WILLIKINS_ALLOWED_HOSTS` and rmcp's `allowed_hosts` must
  contain. Measure against a live deployment.
- **Whether Railway's edge overwrites a client-supplied `X-Real-IP`, `X-Forwarded-Host` or
  `X-Forwarded-Proto`.** Not stated for any of the three. Until measured, no per-IP limiter or
  forwarded-host check can be trusted. `X-Forwarded-For` appears on none of the seventeen fetched
  pages, which is not proof it is absent on a real request.
- **The service's generated Railway hostname.** It has none at present — the operator deleted the
  public domain on 2026-09-15. Any hostname recalled from an earlier session is memory, not a
  fetched fact, and is deliberately absent from this note. Read it from the dashboard or the CLI
  when a domain is generated; it also cannot be declared in `.railway/railway.ts`.
- **Whether an omitted `domains` key deletes an attached custom domain.** Settle with a read-only
  `railway config plan`, never by applying.
- **Which `jsonwebtoken` crypto backend builds in the repository's Dockerfile.** No cargo command
  was permitted, so neither `rust_crypto` nor `aws_lc_rs` was test-built. The choice changes the
  Dockerfile and the audit posture (section 4.3).
- **Every claim taken from draft-ietf-oauth-v2-1-16.** It is an Internet-Draft whose own Status of
  This Memo says it is inappropriate to cite other than as work in progress, and whose split
  between the non-normative §10 list and the normative body already contains one unreconciled
  tension (refresh tokens: "must" versus "SHOULD").
- **Whether Keycloak's access tokens are JWTs**, stated as such. Only the JWKS endpoint
  description and an example claim set were fetched. Likewise Keycloak's MCP-conformance table is
  **Keycloak's claim about MCP**, fetched from Keycloak's repository — confirm the requirement
  levels against `modelcontextprotocol.io` before repeating them.
- **Zitadel's default access-token format** (JWT or opaque) for a new application.
- **Whether authentik rejects `resource` at `/authorize`**, as opposed to at the token-exchange
  endpoint where the rejection is documented.
- **Logto's CIMD pages** were fetched from the docs repository's `master` branch, not from the
  v1.43.0 tag, so the feature may postdate the current release. Same caveat for the Ory Hydra
  guides (`ory/docs` `master`) and GitHub's docs (`github/docs` `main`); the Hydra OpenAPI and
  config schema quotes are pinned to v26.2.0.
- **Ory Network's free tier.** No number is quotable; the pricing page obfuscates plan text with
  zero-width characters.
- **RFC 9700 and RFC 7662 were not fetched at all.** RFC 9700 is the authority behind every OAuth
  2.1 removal; RFC 7662 governs validation entirely if the chosen provider issues opaque tokens,
  in which case section 2.5's JWT checks do not apply.

## 8. What this settles for the plan

Fifteen bullets, each backed by a quote above. Everything not here is either in a per-section
"Unresolved" list or in section 7.

1. **Authorization is optional, so today's scheme is not a violation and `serve --stdio` needs
   none.** The milestone 2 plan's out-of-scope bullet resolves as: the specification never says a
   bearer token is unacceptable. The trigger for 2c is the public-domain decision plus
   interoperability with OAuth-capable clients (§1.2).
2. **At `/mcp` willikins is an OAuth 2.1 resource server and nothing else**, unless the plan
   chooses to host an authorization server. The specification puts the AS out of scope and allows
   it co-hosted or separate, so the identity-provider question is a product decision (§1.2).
3. **What it must publish:** an RFC 9728 document carrying `resource` (the RFC's only REQUIRED
   field) *and* `authorization_servers` with at least one entry (the specification's MUST), plus
   `scopes_supported` (RECOMMENDED); served by GET as `application/json` with 200, omitting
   zero-valued parameters (§1.2, §2.2).
4. **Its URL is built by inserting the well-known string between host and path, not by
   appending** — `https://<host>/mcp` publishes at `/.well-known/oauth-protected-resource/mcp`,
   and the `resource` value returned must be byte-identical to the identifier the client used. A
   bare-origin identifier is equally legal; picking one freezes the other (§2.2).
5. **The 401 gains two things and nothing else changes:** `WWW-Authenticate: Bearer` with
   `resource_metadata="<PRM URL>"` and, SHOULD-level, `scope="..."`. Strictly, the MUST is to
   implement *one of* the two discovery mechanisms — the header or the well-known URI — and
   clients prefer the header, so do both. 401 for missing or invalid, 403 with
   `error="insufficient_scope"` for insufficient, 400 for malformed (§1.2, §2.2).
6. **Audience binding is a MUST for the server, and the MUST is the MCP profile's, not RFC
   8707's.** The client MUST send `resource` on both requests; the server MUST reject a token not
   issued for it. For a JWT-format token, RFC 9068 §4 spells out the check list — `typ`, exact
   `iss`, `aud` containing this resource, signature with `alg` never `none`, `exp` — and every
   failure is `invalid_token` (§1.2, §2.4, §2.5).
7. **The token transport does not change.** `Authorization: Bearer` on every request, no session
   state, never in a query string — which is exactly what ships. Only the provenance and
   validation of the token change (§1.2, §2.6).
8. **The passthrough prohibition is already satisfied structurally and must stay that way.** An
   access token minted for `/mcp` may never be forwarded to GitHub or Doppler, and no 2c design
   may exchange it for one. `Credential::authorize` being the single outgoing-header site is what
   makes this checkable (§1.2).
9. **PKCE, `state`, `resource`, client registration and issuer pinning are client or
   authorization-server obligations.** They become willikins' only on the branch where it hosts an
   AS, or through the approvals login acting as an OAuth client (§1.2, §2.6).
10. **rmcp does not help server-side, in 3.3.0 or 3.4.0**, and the seam is already correct:
    willikins' axum middleware runs strictly before rmcp sees the request, so token validation and
    the 401 need no rmcp API. The metadata route sits outside rmcp's host check exactly as
    `/healthz` does. An rmcp bump is a code change, not a lock bump — `ServerInfo` is deprecated at
    3.4.0 and used at three sites under `-D warnings` (§3).
11. **rmcp passes the complete request headers into every tool handler**, so the `Authorization`
    header is in handler scope unless the middleware strips it. Whether to strip is a deliberate
    plan decision against the redaction invariant (§3.4).
12. **Crates to pin:** `jsonwebtoken = "11"` with `default-features = false` and exactly one
    backend, and `axum-extra = "0.12"` with `cookie-signed`; the JWKS fetch and cache hand-rolled
    over the tree's `ureq` 3.4.2 inside `spawn_blocking`, with a rate-limited refresh on an unknown
    `kid`. No `reqwest`, no `oauth2`, no `openidconnect`, no `josekit`, no `jwt-simple`. Measure
    against MSRV **1.88** (§4).
13. **`jsonwebtoken`'s defaults are not enough on their own.** Audience fails closed only when the
    token carries `aud`; `required_spec_claims` defaults to `{"exp"}` and `validate_nbf` to false,
    and the algorithm list defaults to HS256. Issuer, audience, required claims and an explicit
    algorithm allowlist must all be set (§4.2).
14. **What Railway's edge does:** terminates TLS at the POP and adds `X-Real-IP`,
    `X-Forwarded-Proto` (always `https`), `X-Forwarded-Host`, `X-Railway-Edge`, `X-Request-Start`,
    `X-Railway-Request-Id`; imposes **no per-IP rate limit** and **no documented request-body size
    cap**; silently converts a plain-HTTP POST into a GET; blocks browsers from the private
    network entirely; and never lets a generated domain be declared in `.railway/railway.ts`
    (§5).
15. **Against the two must-haves, Logto is the only provider surveyed that passes both** — it
    honours RFC 8707 `resource` by name and offers CIMD for a client with no prior relationship.
    Auth0 and Zitadel offer open DCR but bind audience through a non-`resource` parameter (Zitadel
    accepts `resource` and ignores it); Keycloak "cannot recognize `resource`" and its CIMD is
    experimental; authentik rejects `resource` and its DCR needs a bearer token; Ory Hydra has DCR
    and `audience` but no login UI at all; GitHub OAuth apps fail both and publish no discovery
    document. This is a fact column, not a ranking — an operator who controls the client can send
    whichever parameter a provider wants (§6).

**The one fork the plan must decide before task 1.** If 2c stays at "MCP clients get OAuth tokens
for `/mcp`, operator-provisioned credentials still reach GitHub and Doppler", willikins is not an
"MCP Proxy Server" in the specification's defined sense and the confused-deputy MUSTs do not
apply. If 2c instead introduces a per-user upstream OAuth flow, willikins becomes one and a hard
set of MUSTs lands at once: a per-user registry of approved `client_id`s checked *before*
forwarding upstream, an MCP-owned consent page, `__Host-` signed cookies, single-use short-lived
`state`, exact-string `redirect_uri` matching. Credential routing across several GitHub
organizations and Doppler workplaces is milestone 3's work, which argues for the first branch
(§1.2).

**And one thing the specification does not reach.** The browser login on `/approvals` is outside
the authorization specification's stated scope. The confused-deputy consent-UI and consent-cookie
lists are the closest analogue and an adoptable checklist — `__Host-` prefix, `Secure`,
`HttpOnly`, `SameSite`, signed or server-side sessions, `frame-ancestors` or
`X-Frame-Options: DENY` — on top of the shipped single-use nonce and Origin/Referer defences, but
they are MUSTs only for an MCP Proxy Server's consent page. As an OAuth client the page is
draft-16's "web application", a *confidential* client; §9 "Browser-Based Apps" is still a TODO
placeholder and does not apply (§1.2, §2.6).
