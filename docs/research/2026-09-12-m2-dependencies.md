# Milestone 2 dependency and provider research

**Created:** 2026-09-12
**Plan:** `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`
**Previous:** `docs/research/2026-09-11-m1-dependencies.md`

Five research passes run in parallel on 2026-09-12, each written by an agent with
WebFetch, WebSearch, context7, `curl`, and `gh api`, in the same format as the milestone 1
note: a recommendation, then facts each carrying a source URL and a verbatim quote, then an
unresolved list. A fact taken from a search snippet or a rendered page's paraphrase rather
than a fetched primary source is marked **(unverified)**. Anything that could not be settled
verbatim is repeated in the plan's "Verify before relying on them" list.

Two method notes worth keeping. `docs.doppler.com` publishes an `llms.txt` index and every
reference page has a raw markdown twin at the same path plus `.md`, which returns the
page's embedded OpenAPI JSON verbatim and avoids the 429s a previous session hit.
`docs.swift.org` is a JavaScript shell to every fetcher; the DocC source it is built from is
in the `swiftlang/swift-book` repository.

Sections:

1. `rmcp` 3 and the MCP authorization specification
2. GitHub REST: repositories, topics, Actions secrets, sealed boxes, tokens
3. Doppler API v3: tokens, projects, environments, configs, service tokens, secrets
4. Supporting crates, Railway deployment, and the container image
5. Reserved-word verification (Swift, Kotlin, Rust, Java, Windows)


## 1. `rmcp` 3 and the MCP specification



All version/feature facts below are cross-checked against the `main` branch of
github.com/modelcontextprotocol/rust-sdk as of this session; current published version is 3.3.0
(confirmed in `docs/research/2026-09-11-m1-dependencies.md`, section "Rust MCP SDK"). Two facts
below were first retrieved via WebFetch's summarizing model, then caught and corrected against the
verbatim source: a config-defaults summary claimed `max_request_body_bytes: 8 MB` and
`legacy_session_mode: false`, but the actual source (`crates/rmcp/src/transport/streamable_http_server/tower.rs`)
says 4 MiB and `true`. Only the re-verified values are recorded as facts here; where a fact still
rests on a WebFetch paraphrase rather than a verbatim quote, it is marked as such inline.

### 1. Server definition: tools, errors, structured output, get_info

**Recommendation:** Use `#[tool_router(server_handler)]` for a single-capability tools-only server, or plain `#[tool_router]` + `#[tool_handler]` on a separate `ServerHandler` impl when you also need `get_info` overrides (instructions, capabilities, protocol version) or non-tool capabilities (prompts/resources). Parameters go through the `Parameters<T>` extractor over a `serde::Deserialize + schemars::JsonSchema` struct. For structured JSON output wrap the return type in `rmcp::Json<T>` (requires the `server` feature) — this places the value in the result's `structured_content` field, generating the output schema from `T` automatically. Return `Ok(CallToolResult::error(...))` for a tool-level failure the caller should see rendered normally, and reserve `Err(ErrorData)` (aka `McpError`) for protocol-level failures (unroutable request, invalid params) that surface as a JSON-RPC `-32602`-style error instead of a tool result.

- `ServerHandler::get_info` returns a `ServerInfo` built via `ServerCapabilities::builder()`, with `.with_server_info(...)`, `.with_protocol_version(...)`, and `.with_instructions("...")` to set the metadata a client sees at initialize time. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/common/counter.rs)
  - "fn get_info(&self) -> ServerInfo {\n        ServerInfo::new(\n            ServerCapabilities::builder()\n                .enable_prompts()\n                .enable_resources()\n                .enable_tools()\n                .build(),\n        )\n        .with_server_info(Implementation::from_build_env())\n        .with_protocol_version(ProtocolVersion::V_2024_11_05)\n        .with_instructions(\"This server provides counter tools and prompts. Tools: increment, decrement, get_value, say_hello, echo, sum. Prompts: example_prompt (takes a message), counter_analysis (analyzes counter state with a goal).\".to_string())\n    }"
- Tool handlers should distinguish `Ok(CallToolResult::error(...))` — the tool ran and produced a failure the caller should see — from `Err(ErrorData)`, a JSON-RPC protocol error to use "only when the request itself is unroutable". (source: https://docs.rs/rmcp/latest/rmcp/model/struct.CallToolResult.html, via context7 `/websites/rs_rmcp_rmcp`)
  - "if query.is_empty() {\n        return Err(ErrorData::invalid_params(\"query must be non-empty\", None));\n    }\n    // Tool ran, no result. Caller should see the explanation:\n    let rows = run_query(query).await;\n    if rows.is_empty() {\n        return Ok(CallToolResult::error(vec![ContentBlock::text(\n            format!(\"no rows matched '{query}'\"),\n        )]));\n    }"
- `ErrorData` (the type aliased/re-exported as the tool error type, referred to informally as `McpError`) is a plain struct of `code: ErrorCode`, `message: Cow<'static, str>`, `data: Option<Value>`. (source: https://docs.rs/rmcp/latest/rmcp/model/struct.ErrorData.html, via context7)
  - "pub struct ErrorData {\n    pub code: ErrorCode,\n    pub message: Cow<'static, str>,\n    pub data: Option<Value>,\n}"
- `CallToolResult` also supports a structured-error constructor for returning typed error detail as JSON rather than plain text content. (source: https://docs.rs/rmcp/latest/rmcp/model/struct.CallToolResult.html, via context7)
  - "let result = CallToolResult::structured_error(json!({\n    \"error_code\": \"INVALID_INPUT\",\n    \"message\": \"Temperature value out of range\",\n    \"details\": {\n        \"min\": -50,\n        \"max\": 50,\n        \"provided\": 100\n    }\n}));"
- `Json<T>` (in `rmcp::handler::server::wrapper`) marks a tool return value for structured JSON content with an associated schema; when used, the framework places the value in the result's `structured_content` field instead of the plain `content` field. Requires the `server` feature. (source: https://docs.rs/rmcp/latest/rmcp/handler/server/wrapper/struct.Json.html, via context7)
  - "The Json wrapper is used to indicate that a value should be serialized as structured JSON content with an associated schema when interacting with tools ... the framework places the resulting JSON into the structured_content field of the tool result instead of the standard content field. This functionality is available only when the server crate feature is enabled."
- A working structured-output tool returns `Result<Json<WeatherResponse>, String>` from an `#[tool(...)]`-annotated async method; the `Err(String)` arm becomes a tool-level error (not a protocol error), consistent with `CallToolHandler`'s `IntoCallToolResult` bound accepting a plain `Result<T, E: ToString>`. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/structured_output.rs)
  - "pub async fn get_weather(\n        &self,\n        params: Parameters<WeatherRequest>,\n    ) -> Result<Json<WeatherResponse>, String> {\n        ...\n        Ok(Json(weather))\n    }"
- `Parameters<T>` is a newtype extractor: the handler receives `Parameters(T)` and pulls the inner value out; `T` must derive `Deserialize` (+ `JsonSchema` for schema generation). (source: https://docs.rs/rmcp/latest/rmcp/handler/server/wrapper/struct.Parameters.html, via context7)
  - "async fn calculate(params: Parameters<CalculationRequest>) -> Result<String, String> {\n    let request = params.0; // Extract the inner value\n    ..."
- `#[tool_router(server_handler)]` on an impl block auto-emits `#[tool_handler]` on `impl ServerHandler for Self`, skipping a separate handler impl; plain `#[tool_router]` requires a manual `#[tool_handler]` block that calls `Self::tool_router()`. (source: https://docs.rs/rmcp/latest/rmcp/attr.tool_router.html, via context7)
  - "server_handler (flag) - Optional - When present, automatically emits `#[::rmcp::tool_handler]` on `impl ServerHandler for Self`." / "#[tool_router]\nimpl MyToolHandler {\n    #[tool]\n    pub fn my_tool() {}\n}\n\n// #[tool_handler] calls Self::tool_router() automatically\n#[tool_handler]\nimpl ServerHandler for MyToolHandler {}"
- `ServiceExt::serve` runs a `ServerHandler` over a transport and returns a `RunningService`; the idiomatic stdio entry point is `Counter::new().serve(stdio()).await?` followed by `service.waiting().await?`. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/counter_stdio.rs)
  - "let service = Counter::new().serve(stdio()).await.inspect_err(|e| {\n        tracing::error!(\"serving error: {:?}\", e);\n    })?;\n\n    service.waiting().await?;"

**Unresolved**
- Did not find an official docs.rs page enumerating every `ServerCapabilities::builder()` method (only `enable_prompts`/`enable_resources`/`enable_tools` seen in the example above); check docs.rs directly if a capability beyond tools/prompts/resources/logging is needed.

### 2. Streamable HTTP server: config, session manager, mounting, dependency versions

**Recommendation:** Build `StreamableHttpService::new(factory_fn, session_manager, config)` and mount it with `Router::nest_service("/mcp", service)` — it is a Tower service, not axum-specific. Use `LocalSessionManager` (in-process) unless cross-instance session recovery is needed, in which case set `StreamableHttpServerConfig.session_store` to a custom `SessionStore` impl. Leave `legacy_session_mode` at its default of `true` only if you must support pre-`2026-07-28` clients; per SEP-2567 the `2026-07-28` revision removes sessions entirely and such requests are always served statelessly regardless of this flag. Do not disable `keep_alive`/`init_timeout` on `SessionConfig` — they are the safety net against exactly the zombie-session class of bug fixed by GHSA-9pj6-vhgr-3mwh (section 3). For the axum/tokio pairing, mirror the rust-sdk's own example crate: axum 0.8, tokio 1, hyper 1, tower-http 0.7 — rmcp's own `[dependencies]` do not pin axum at all (only `tower-service` is a real dependency; `axum` appears only under `[dev-dependencies]` in the `rmcp` crate itself), so any axum version implementing `tower::Service` compatibly will work, but 0.8 is the version actually exercised in-repo.

- `StreamableHttpServerConfig`'s eleven fields, taken directly from the struct definition (not a paraphrase): `sse_keep_alive`, `sse_retry`, `legacy_session_mode`, `json_response`, `cancellation_token`, `allowed_hosts`, `allowed_origins`, a private `validate_empty_origin_allowlist`, `session_store`, `max_request_body_bytes`, `stateless_protocol_metadata_required`. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/transport/streamable_http_server/tower.rs)
  - "pub struct StreamableHttpServerConfig {\n    pub sse_keep_alive: Option<Duration>,\n ... pub legacy_session_mode: bool,\n    pub json_response: bool,\n ... pub allowed_hosts: Vec<String>,\n pub allowed_origins: Vec<String>,\n    validate_empty_origin_allowlist: bool,\n    pub session_store: Option<Arc<dyn SessionStore>>,\n    pub max_request_body_bytes: usize,\n    pub stateless_protocol_metadata_required: bool,\n}"
- The verbatim `Default` impl: `legacy_session_mode: true` (not `false` — corrects an earlier summarized-tool answer), `allowed_hosts: vec!["localhost", "127.0.0.1", "::1"]`, `max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES` = `4 * 1024 * 1024` (4 MiB, not the 8 MiB an earlier paraphrase claimed), `sse_keep_alive: Some(15s)`, `sse_retry: Some(3s)`. (source: same tower.rs file as above)
  - "sse_keep_alive: Some(Duration::from_secs(15)),\n sse_retry: Some(Duration::from_secs(3)),\n legacy_session_mode: true,\n json_response: false,\n ... allowed_hosts: vec![\"localhost\".into(), \"127.0.0.1\".into(), \"::1\".into()],\n ... max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES,\n" / "pub(crate) const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;"
- `legacy_session_mode`'s doc comment states it only applies to legacy protocol versions: per SEP-2567 sessions are removed from `2026-07-28`, so requests negotiating that version are always served statelessly regardless of the flag. (source: same tower.rs file)
  - "/// Only applies to legacy protocol versions (`< 2026-07-28`). Per SEP-2567,\n    /// sessions are removed from the `2026-07-28` version, so requests\n    /// negotiating that version are always served statelessly regardless of\n    /// this setting.\n    pub legacy_session_mode: bool,"
- `allowed_hosts` defaults to loopback-only specifically to prevent DNS rebinding attacks; public deployments must override it via `disable_allowed_hosts()` / `with_allowed_hosts(...)` / `with_allowed_origins(...)` / `enforce_origin_validation()`. (source: same tower.rs file)
  - "/// By default, Streamable HTTP servers only accept loopback hosts to\n    /// prevent DNS rebinding attacks against locally running servers. Public\n    /// deployments should override this list with their own hostnames."
- `max_request_body_bytes` is "enforced while streaming the body, independent of `Content-Length`, chunked transfer encoding, or HTTP version"; oversized payloads get HTTP `413 Payload Too Large`. (source: same tower.rs file)
  - "/// Enforced while streaming the body, independent of `Content-Length`,\n    /// chunked transfer encoding, or HTTP version. Oversized payloads receive\n    /// a `413 Payload Too Large` response."
- `json_response` (stateless mode's `with_json_response`) governs whether simple request/response tools get a plain `application/json` reply instead of an SSE stream, with an automatic fallback if the handler turns out to need to stream: "When true and `legacy_session_mode` is false, the server prefers `Content-Type: application/json` for simple request-response tools. If the handler emits a notification or request before the final response, the server falls back to `text/event-stream` so no message is lost." Note this only takes effect when `legacy_session_mode` is `false` — under the crate's own default (`legacy_session_mode: true`), `json_response` has no effect until you also disable legacy mode. (source: same tower.rs file)
- `LocalSessionManager`'s per-session `SessionConfig` has real timeout/eviction knobs: `channel_capacity` (default 16), `keep_alive: Option<Duration>` (default `Some(300s)` — closes a session after 5 minutes of inactivity as "a safety net for cleaning up sessions whose HTTP connections have silently dropped"), `completed_cache_ttl: Duration` (default 60s, for late SSE resume requests), `init_timeout: Option<Duration>` (default `Some(60s)`, terminates a session that never sends `initialize`). (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/transport/streamable_http_server/session/local.rs)
  - "/// The session will be closed after this duration of inactivity.\n    ///\n    /// This serves as a safety net for cleaning up sessions whose HTTP\n    /// connections have silently dropped (e.g., due to an HTTP/2\n    /// `RST_STREAM`). Without a timeout, such sessions become zombies:\n    ...\n    /// Defaults to 5 minutes. ...\n    pub keep_alive: Option<Duration>," / "pub const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(300);\n    pub const DEFAULT_SSE_RETRY: Duration = Duration::from_secs(3);\n    pub const DEFAULT_COMPLETED_CACHE_TTL: Duration = Duration::from_secs(60);\n    pub const DEFAULT_INIT_TIMEOUT: Duration = Duration::from_secs(60);"
- Mounting pattern confirmed in-repo: build the service, then `Router::new().nest_service("/mcp", mcp_service)`, optionally wrapped in `.layer(...)` for middleware (full example in section 4). (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/simple_auth_streamhttp.rs)
  - "let protected_mcp_router =\n        Router::new()\n            .nest_service(\"/mcp\", mcp_service)\n            .layer(middleware::from_fn_with_state(\n                token_store.clone(),\n                auth_middleware,\n            ));"
- The rust-sdk's own example crate (`examples/servers`, in the same workspace/CI as `rmcp` 3.3.0) pins `axum = { version = "0.8", features = ["macros"] }`, `tokio = { version = "1", features = [...] }`, `hyper = { version = "1" }`, `tower-http = { version = "0.7", features = ["cors"] }`. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/Cargo.toml)
  - "axum = { version = \"0.8\", features = [\"macros\"] }\n...\ntokio = { version = \"1\", features = [...] }\n...\nhyper = { version = \"1\" }\n...\ntower-http = { version = \"0.7\", features = [\"cors\"] }"
- Verbatim from `crates/rmcp/Cargo.toml`: the real `[dependencies]` pin on tokio is `tokio = { version = "1", features = ["sync", "macros", "rt", "time"] }` — no `io-std`/`net`/`full` there (those come in only via feature-gated combinations like `transport-io`/`transport-child-process`). `axum` does not appear under `[dependencies]` at all; it appears only under `[dev-dependencies]`: `tokio = { version = "1", features = ["full", "test-util"] }`, `schemars = { version = "1.1.0", features = ["chrono04"] }`, `axum = { version = "0.8", default-features = false, features = ["http1", "tokio"] }`. This confirms `StreamableHttpService` is transport/router-agnostic and axum is a consumer choice, not an rmcp dependency. (source: `gh api /repos/modelcontextprotocol/rust-sdk/contents/crates/rmcp/Cargo.toml`, i.e. https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/Cargo.toml)
  - "[dependencies]\nasync-trait = { version = \"0.1.89\", optional = true }\nserde = { version = \"1.0\", features = [\"derive\", \"rc\"] }\n...\ntokio = { version = \"1\", features = [\"sync\", \"macros\", \"rt\", \"time\"] }\n...\n[dev-dependencies]\ntokio = { version = \"1\", features = [\"full\", \"test-util\"] }\nschemars = { version = \"1.1.0\", features = [\"chrono04\"] }\naxum = { version = \"0.8\", default-features = false, features = [\"http1\", \"tokio\"] }"
- Required cargo features, verbatim `[features]` section of `crates/rmcp/Cargo.toml`: `server = ["transport-async-rw", "schemars", "dep:pastey", "uuid"]`, `macros = ["dep:rmcp-macros", "dep:pastey"]`, `schemars = ["dep:schemars"]`, `transport-io = ["transport-async-rw", "tokio/io-std"]`, `transport-streamable-http-server = ["transport-streamable-http-server-session", "server-side-http", "transport-worker"]`, `elicitation = ["dep:url"]`, `auth = ["dep:async-trait", "dep:oauth2", "__reqwest", "dep:url"]`, `auth-enterprise-managed = ["auth", "base64"]`. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/Cargo.toml)
  - "server = [\"transport-async-rw\", \"schemars\", \"dep:pastey\", \"uuid\"]" / "transport-streamable-http-server = [\n  \"transport-streamable-http-server-session\",\n  \"server-side-http\",\n \"transport-worker\",\n]" / "auth = [\"dep:async-trait\", \"dep:oauth2\", \"__reqwest\", \"dep:url\"]" / "auth-enterprise-managed = [\"auth\", \"base64\"]"

**Unresolved**
- Did not independently fetch the `SessionManager` trait's `restore_session`/cross-instance-recovery code path in `local.rs` beyond the config defaults above; if branching on `SessionStore` recovery semantics matters for the design, read `local.rs` lines around `restore_session` directly.

### 3. Security advisories affecting rmcp

**Recommendation:** rmcp 3.3.0 (the version this project depends on) postdates every known fix by at least one major version — all five advisories below are patched at 2.0.0 or 2.1.0. No advisory currently affects 3.x. Still worth codifying two defaults explicitly in the design (rather than relying on the crate default silently doing the right thing): keep `allowed_hosts` restricted to real hostnames in any non-loopback deployment (GHSA-89vp), and keep `SessionConfig.keep_alive` / `init_timeout` enabled. Note the latter is this session's own inference, not a confirmed fact: section 2 verifies those timeouts exist and describe themselves as guarding against "zombie" sessions from silently-dropped connections, which is adjacent to (but not verified here as literally the same fix as) GHSA-9pj6's unbounded-session-accumulation root cause.

- **GHSA-9pj6-vhgr-3mwh** — "Unauthenticated permanent session-table leak in rmcp Streamable HTTP server transport leads to remote denial-of-service" (CVE-2026-63128). Affected `rmcp <= 1.7.0`, patched in `2.0.0`. Severity high, CVSS 3.1 `7.5` (`AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H`). Root cause: `StreamableHttpService::handle_post` called `session_manager.create_session()` before validating that the JSON-RPC body was an `InitializeRequest`, and early-returned without closing the session on validation failure, permanently leaking a `LocalSessionHandle` (~400-550 bytes) per malformed request; a single client sustained >2,000 req/s, ~75-84 GB/day of leaked memory. (source: https://github.com/modelcontextprotocol/rust-sdk/security/advisories/GHSA-9pj6-vhgr-3mwh)
  - "An unauthenticated remote attacker can leak one entry per HTTP request out of the in-memory session table of `LocalSessionManager` by sending a well-formed JSON-RPC `POST` that is *not* an `InitializeRequest`. The Streamable HTTP server's `handle_post` allocates the session **before** it validates the body, then early-returns on the validation failure without calling `close_session`." / `"vulnerable_version_range": "<= 1.7.0", "patched_versions": "2.0.0"` / `"severity": "high"`, CVSS `"score": 7.5`
- **GHSA-89vp-x53w-74fx** — "DNS rebinding vulnerability in rmcp Streamable HTTP server transport" (CVE-2026-42559). Affected `rmcp < 1.4.0`, patched in `1.4.0`. Severity high, CVSS `8.8` (`AV:N/AC:L/PR:N/UI:R/S:U/C:H/I:H/A:H`). Prior to the fix, the `Host` header was not validated, letting a malicious webpage DNS-rebind to reach a loopback-bound MCP server and invoke any tool. The advisory's own text says the fix exposes `StreamableHttpService::with_allowed_hosts(...)`, but the current source (section 2, fetched fresh this session) has that builder method on `StreamableHttpServerConfig`, not on the service directly — likely the advisory used older/looser wording, or the API moved after the advisory was written; use the `StreamableHttpServerConfig` builder confirmed in section 2. (source: https://github.com/advisories/GHSA-89vp-x53w-74fx)
  - "Prior to version 1.4.0, the `rmcp` crate's Streamable HTTP server transport (`crates/rmcp/src/transport/streamable_http_server/`) did not validate the incoming `Host` header. This allowed a malicious public website, via a DNS rebinding attack, to send authenticated requests to an MCP server running on the victim's loopback or private-network interface" / `"vulnerable_version_range": "< 1.4.0", "first_patched_version": "1.4.0"` / "`StreamableHttpServerConfig::allowed_hosts` now defaults to a loopback-only allowlist: `[\"localhost\", \"127.0.0.1\", \"::1\"]`."
- Three further advisories, all client-side auth-flow issues (relevant only if willikins ever embeds an rmcp *client* doing OAuth discovery, not for a server-only deployment), all patched well below the current 3.3.0: **GHSA-c9xm-49cp-xcr9** "rmcp OAuth client fetches server-controlled resource_metadata URLs" (`<= 1.8.0`, patched `2.0.0`); **GHSA-9g45-5xwm-f3wc** "Custom HTTP headers leak to cross-origin redirect targets" (`<= 1.7.0`, patched `2.1.0`); **GHSA-33f5-2c5q-wgwj** "Missing Resource Field Validation in OAuth Protected Resource Metadata Discovery" (`<= 1.8.0`, patched `2.0.0`). (source: https://github.com/modelcontextprotocol/rust-sdk/security/advisories, listing endpoint `gh api /repos/modelcontextprotocol/rust-sdk/security-advisories`)
  - `{"ghsa_id":"GHSA-c9xm-49cp-xcr9", ... "vulnerable_version_range":"<= 1.8.0", "patched_versions":"2.0.0"}` / `{"ghsa_id":"GHSA-9g45-5xwm-f3wc", ... "vulnerable_version_range":"<= 1.7.0","patched_versions":"2.1.0"}` / `{"ghsa_id":"GHSA-33f5-2c5q-wgwj", ... "vulnerable_version_range":"<= 1.8.0", "patched_versions":"2.0.0"}`

**Unresolved**
- Only summary + version-range JSON was pulled for the three advisories above (not their full bodies); if willikins ever adds an rmcp *client* role, pull each full advisory via `gh api /repos/modelcontextprotocol/rust-sdk/security-advisories/<id>` before relying on the summary.

### 4. Authentication on the HTTP transport

**Recommendation:** rmcp's `auth` / `auth-enterprise-managed` features are entirely client-side (an `AuthorizationManager`/`AuthorizationSession`/`OAuthState` for an rmcp *client* to run an OAuth2 authorization-code or client-credentials flow against a remote server) — there is no server-side auth hook in rmcp itself. Requiring a bearer token in front of `StreamableHttpService` is done with ordinary axum/tower middleware wrapped around the nested service, exactly as the rust-sdk's own `simple_auth_streamhttp.rs` example does: an `axum::middleware::from_fn_with_state` layer that extracts `Authorization: Bearer <token>`, checks it against a store, and returns `401` on failure, applied only to the `/mcp` sub-router (not to public routes like a token-issuance endpoint).

- Every method on `rmcp::transport::auth::AuthorizationManager` is about *acting as* an OAuth2 client: `configure_client_id`, `get_authorization_url`, `exchange_code_for_token`, `get_access_token`, `request_scope_upgrade`, `prepare_request` (adds an auth header to an outgoing `RequestBuilder`), `exchange_client_credentials`. There is no analogous "validate_incoming_token" or "require_bearer" method anywhere in the `auth` feature's surface, and `OAuthState::authenticate_client_credentials` likewise only transitions the *client's own* state "from Unauthorized to Authorized". (source: https://docs.rs/rmcp/latest/rmcp/transport/auth/struct.AuthorizationManager.html and .../enum.OAuthState.html, via context7)
  - "prepare_request(request: RequestBuilder) -> Result<RequestBuilder, AuthError>\nAdds the necessary authorization headers to a given request builder." / "The authenticate_client_credentials method ... transitions the state directly from Unauthorized to Authorized, bypassing the interactive Session state by discovering metadata and exchanging credentials for an access token."
- The `auth` feature's Cargo gating (`auth = ["dep:async-trait", "dep:oauth2", "__reqwest", "dep:url"]`, `auth-enterprise-managed = ["auth", "base64"]`) pulls in `oauth2` and `reqwest`, both client-side HTTP-calling dependencies — reinforcing these features exist for an rmcp *client* talking outward, not for protecting an rmcp server's own endpoints. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/Cargo.toml)
  - "auth = [\"dep:async-trait\", \"dep:oauth2\", \"__reqwest\", \"dep:url\"]\nauth-enterprise-managed = [\"auth\", \"base64\"]"
- Full working reference for the axum-middleware pattern, from the rust-sdk's own examples (`examples/servers/src/simple_auth_streamhttp.rs`), reproduced because it directly answers "how do I require Bearer auth in front of StreamableHttpService": (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/simple_auth_streamhttp.rs)
  ```rust
  async fn auth_middleware(
      State(token_store): State<Arc<TokenStore>>,
      headers: HeaderMap,
      request: Request<axum::body::Body>,
      next: Next,
  ) -> Result<Response, StatusCode> {
      match extract_token(&headers) {
          Some(token) if token_store.is_valid(&token) => Ok(next.run(request).await),
          _ => Err(StatusCode::UNAUTHORIZED),
      }
  }
  // ...
  let mcp_service: StreamableHttpService<Counter, LocalSessionManager> =
      StreamableHttpService::new(
          || Ok(Counter::new()),
          LocalSessionManager::default().into(),
          StreamableHttpServerConfig::default(),
      );
  let protected_mcp_router = Router::new()
      .nest_service("/mcp", mcp_service)
      .layer(middleware::from_fn_with_state(token_store.clone(), auth_middleware));
  let app = Router::new()
      .route("/", get(index))
      .nest("/api", api_routes)          // public: token issuance, health check
      .merge(protected_mcp_router);       // protected: /mcp
  ```
- `StreamableHttpService` injects the raw `http::request::Parts` (and any axum/tower extension-layer state) into the request context extensions, so a tool handler can read the caller's identity that the auth middleware attached (e.g. via `request.extensions_mut().insert(...)` upstream) — `ctx.extensions.get::<http::request::Parts>()` then `parts.extensions.get::<AppState>()`. (source: https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/tower/struct.StreamableHttpService.html, via context7)
  - "Retrieve custom state injected via axum layers from the request context extensions." / "let parts = ctx.extensions.get::<http::request::Parts>().unwrap();\n    let state = parts.extensions.get::<AppState>().unwrap();"

**Unresolved**
- Did not read `complex_auth_streamhttp.rs` or `cimd_auth_streamhttp.rs` (two other auth examples in the same directory, likely demonstrating full OAuth2/CIMD *client* flows against a resource server) — only `simple_auth_streamhttp.rs` (the static-bearer-token case, most relevant to a provisioning-butler MCP server) was read in full.

### 5. Blocking work inside a tool handler

**Recommendation:** rmcp's own repository contains no example of `tokio::task::spawn_blocking` inside a tool handler — this is a plain tokio concern, not an rmcp-specific pattern, and the crate does not special-case it. The standard advice still applies and is *not* contradicted by anything found: wrap the project's synchronous core's calls in `tokio::task::spawn_blocking(move || { ... })` inside the `async fn` tool method, `.await` the `JoinHandle`, and map its `Result<T, JoinError>` into the tool's own error type. `CallToolHandler`'s blanket impls are generic over `F: FnOnce(&S, ...) -> R + MaybeSendFuture` and `R: IntoCallToolResult + MaybeSendFuture` — the `MaybeSend`/ `MaybeSendFuture` bounds (rather than a hard `Send`) suggest rmcp conditionally relaxes the `Send` requirement (likely for a non-Send/single-threaded or wasm target), but on a normal multi-threaded tokio server the practical constraint is the same as any other spawned async work: the closure passed to `spawn_blocking` must be `'static` (own or `Arc`-clone its captures) since it runs on a separate blocking-pool thread outliving the calling stack frame.

- No usage of `spawn_blocking` appears anywhere under `examples/` in the rust-sdk repository (a full-repo GitHub code search for `spawn_blocking` returns exactly one hit, in an unrelated low-level transport file, not a tool handler). (source: https://github.com/search?q=repo%3Amodelcontextprotocol%2Frust-sdk+spawn_blocking&type=code)
  - `{"total_count": 1, "items": [{"path": "crates/rmcp/src/transport/common/unix_socket.rs"}]}`
- `CallToolHandler`'s generic bounds for a zero-to-eleven-argument synchronous/async method are `F: FnOnce(&S, T0, ..) -> R + MaybeSendFuture`, `R: IntoCallToolResult + MaybeSendFuture`, `S: MaybeSend` — i.e. the framework's own trait bounds use a `MaybeSend`/`MaybeSendFuture` marker rather than a hard `Send`, implying the `Send` requirement is feature/target-conditional inside rmcp itself. (source: https://docs.rs/rmcp/latest/rmcp/handler/server/tool/trait.CallToolHandler.html, via context7)
  - "impl<T0, T1, S, F, R> CallToolHandler<S, SyncMethodAdapter<(T0, T1), R>> for F\nwhere T0: for<'a> FromContextPart<ToolCallContext<'a, S>>, T1: for<'a> FromContextPart<ToolCallContext<'a, S>>, F: FnOnce(&S, T0, T1) -> R + MaybeSendFuture, R: IntoCallToolResult + MaybeSendFuture, S: MaybeSend,"

**Unresolved**
- Did not find rmcp documentation or an issue/discussion explicitly recommending `spawn_blocking` for tool handlers — this section's recommendation is inferred from general tokio practice plus the absence of any rmcp-specific alternative, not from a source that states it directly. If a stronger citation is needed, this would require a non-rmcp source (the tokio docs themselves) rather than anything in the rust-sdk repo.
- Did not resolve what feature flag or target actually flips `MaybeSend`/`MaybeSendFuture` between `Send` and non-`Send` (a plausible guess is a `wasm`/single-threaded-executor feature, but this was not confirmed by reading the `MaybeSend` trait definition itself).

### 6. The MCP specification (2026-07-28 revision)

**Recommendation:** Treat MCP's Authorization framework as **optional overall**: a server that skips HTTP-transport OAuth entirely and uses a pre-shared bearer token instead is spec-compliant, because the spec explicitly permits "clients and servers ... to negotiate their own custom authentication and authorization strategies" and gates the whole OAuth 2.1 apparatus behind "when supported." *If* you do implement the Authorization framework, however, its internal requirements are mostly hard MUSTs (protected resource metadata, audience-bound token validation, 401 on invalid token, `WWW-Authenticate` with `resource_metadata` and — SHOULD-level — a `scope` parameter). For willikins' own bearer-token design (section 4), the honest framing is: this is the "custom authentication strategy" escape hatch the spec allows, not an implementation of MCP's OAuth 2.1 Authorization framework.

- Authorization is explicitly optional, and STDIO is explicitly told not to use it: "Authorization is **OPTIONAL** for MCP implementations. When supported: Implementations using an HTTP-based transport **SHOULD** conform to this specification. Implementations using an STDIO transport **SHOULD NOT** follow this specification, and instead retrieve credentials from the environment." (source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- Clients and servers may bypass the whole framework with a custom scheme: "clients and servers **MAY** negotiate their own custom authentication and authorization strategies." (source: https://modelcontextprotocol.io/specification/2026-07-28/basic)
- *If* the Authorization framework is adopted, several sub-requirements are hard MUSTs: "MCP servers **MUST** implement OAuth 2.0 Protected Resource Metadata ([RFC9728])" and "Authorization servers **MUST** implement OAuth 2.1 with appropriate security measures for both confidential and public clients." (source: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- Token validation and 401 handling are MUST-level: "MCP servers ... **MUST** validate access tokens as described in [OAuth 2.1 Section 5.2] ... **MUST** validate that access tokens were issued specifically for them as the intended audience ... Invalid or expired tokens **MUST** receive a HTTP 401 response." (source: same authorization page)
- `WWW-Authenticate` on 401 should carry a `resource_metadata` URI and, per RFC 6750, a scope hint: "`WWW-Authenticate: Bearer resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource\", scope=\"files:read\"`"; error table: "401 | Unauthorized | Authorization required or token invalid" / "403 | Forbidden | Invalid scopes or insufficient permissions". (source: same authorization page)
- Elicitation (server asking the user for input) is a stable, spec-defined feature in 2026-07-28 with two modes; form mode requires the client to declare the `elicitation` capability. URL mode is newer and explicitly marked unstable: "**New feature:** URL mode elicitation is introduced in the `2025-11-25` version of the MCP specification. Its design and implementation may change in future protocol revisions." Servers "**MUST NOT** use form mode elicitation to request sensitive information such as passwords, API keys, access tokens, or payment credentials" — must use URL mode instead, directly relevant to willikins' own secret-handling invariants. (source: https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation)
- rmcp itself supports server-side form-mode elicitation today via an `elicit_safe!(Type)` macro marking a request/response struct as elicitable, used from inside a tool handler. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/elicitation_stdio.rs)
  - "use rmcp::{\n    ErrorData as McpError, ServerHandler, ServiceExt, elicit_safe, ...\n};\n...\n pub struct UserInfo {\n    pub name: String,\n}\n\n// Mark as safe for elicitation\nelicit_safe!(UserInfo);"
- MCP is now explicitly a **stateless** protocol at the base-protocol level in this revision: "The Model Context Protocol (MCP) is a **stateless protocol**: all the information needed to process a request is contained in the request itself ... an open connection, such as a STDIO process, is not a conversation or session." This is the spec-level change (SEP-2567, cited in section 2) that `legacy_session_mode` exists to bridge for older clients. (source: https://modelcontextprotocol.io/specification/2026-07-28/basic)
- Correction to an assumption used throughout this document: rmcp 3.3.0 *knows about* `2026-07-28` but does not negotiate it by default. `ProtocolVersion::LATEST` is defined as `Self::V_2025_11_25`, one revision behind; `2026-07-28` is only reachable by naming it explicitly (`ProtocolVersion::V_2026_07_28`) or via `KNOWN_VERSIONS`, which lists it as the newest of five. The `get_info` example quoted in section 1 in fact pins `ProtocolVersion::V_2024_11_05` — two revisions behind — so copying it verbatim would advertise a stale protocol version; explicitly set `.with_protocol_version(ProtocolVersion::V_2026_07_28)` if the SEP-2567 stateless behavior is wanted. (source: https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/model.rs)
  - "pub const V_2026_07_28: Self = Self(Cow::Borrowed(\"2026-07-28\"));\n    pub const V_2025_11_25: Self = Self(Cow::Borrowed(\"2025-11-25\"));\n    ...\n    pub const LATEST: Self = Self::V_2025_11_25;\n\n    /// First protocol version that requires SEP-2243 standard HTTP headers.\n    pub const STANDARD_HEADERS: Self = Self::V_2026_07_28;\n\n    /// All protocol versions known to this SDK, oldest first.\n    pub const KNOWN_VERSIONS: &[Self] = &[\n        Self::V_2024_11_05,\n        Self::V_2025_03_26,\n        Self::V_2025_06_18,\n        Self::V_2025_11_25,\n        Self::V_2026_07_28,\n    ];"
- Design-relevant observation (not a research gap): the section-4 `simple_auth_streamhttp.rs` example returns a bare `StatusCode::UNAUTHORIZED` on an invalid/missing token, with no `WWW-Authenticate` header at all. Per the spec text quoted above, a 401 from a server that follows the Authorization framework should carry `WWW-Authenticate: Bearer resource_metadata="..."`; the example (and any willikins server modeled directly on it) does not do this. That is consistent with treating a static bearer token as the spec's "custom authentication strategy" escape hatch rather than an implementation of the Authorization framework — but it does mean a spec-aware OAuth client hitting a 401 gets no discovery challenge to follow. Worth a deliberate decision, not an oversight to silently inherit.

**Unresolved**
- Did not check whether rmcp's client side implements URL-mode elicitation (only form-mode confirmed); verify separately if willikins ever needs to *request* URL-mode elicitation.
- Did not find rmcp's own README/lib.rs prose sentence asserting "we implement 2026-07-28" (only the `ProtocolVersion` constants above, which is stronger primary evidence than prose would be, but is a different kind of source than the m1 doc's original claim).

### 7. Body/result size limits and rate limiting

**Recommendation:** Use rmcp's built-in `StreamableHttpServerConfig.max_request_body_bytes` (default 4 MiB, returns 413 when exceeded) rather than reaching for axum's `DefaultBodyLimit` — it's already wired into the streamed-body path. rmcp has no server-side rate limiting of any kind and no limit on *outbound* tool-result size; for both of those, add ordinary tower/axum middleware in front of the service (e.g. `tower_governor` for rate limiting) exactly as you would for any other axum app, since `StreamableHttpService` is a plain Tower service nested into the router.

- `max_request_body_bytes` (covered fully in section 2) is the one built-in size limit rmcp ships, and it is a *request*-body limit only, enforced during streaming, independent of framing (`Content-Length`/chunked/HTTP version), returning 413. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/transport/streamable_http_server/tower.rs, same file as section 2)
  - "/// Enforced while streaming the body, independent of `Content-Length`,\n    /// chunked transfer encoding, or HTTP version. Oversized payloads receive\n    /// a `413 Payload Too Large` response."
- The only other size-limiting knobs found in rmcp are all on the *client* side, not the server: `get_stream_with_max_sse_event_size` / `post_message_with_max_sse_event_size` (max raw SSE event size for an `auth_token`-bearing HTTP client transport) and `JsonRpcMessageCodec::new_with_max_length` (max frame length for the stdio/async-rw codec used by local transports). None of these bound a *server's* outbound `CallToolResult` size. (source: https://docs.rs/rmcp/latest/rmcp/transport/common/unix_socket/struct.UnixSocketHttpClient.html and https://docs.rs/rmcp/latest/rmcp/transport/async_rw/struct.JsonRpcMessageCodec.html, via context7)
  - "pub fn new_with_max_length(max_length: usize) -> Self\nCreates a new instance of the codec with a custom maximum frame length limit." / "get_stream_with_max_sse_event_size ... Opens an SSE stream while enforcing a maximum raw event size limit."
- No rate-limiting type, trait, or feature (nothing named `rate_limit`, `governor`, `throttle`, or similar) was found anywhere in the `[features]` list of `crates/rmcp/Cargo.toml` (section 2) or in the `StreamableHttpServerConfig`/`SessionConfig` field lists (section 2) — confirming this is left entirely to the hosting router. (inferred, from the absence of any such symbol across all `Cargo.toml`/config-struct fetches performed this session) **(unverified)**

**Unresolved**
- Did not run an exhaustive repo-wide search for "rate" or "governor" or "throttle" strings across the whole `rust-sdk` repository (only checked the Cargo feature list and the two config structs already fetched for other topics) — the "no rate limiting" conclusion is an absence-of-evidence inference, not a confirmed "we deliberately do not support this" statement from the maintainers.
- Did not check whether `tower-http`'s `RequestBodyLimitLayer`/`DefaultBodyLimit` would double up awkwardly with rmcp's own `max_request_body_bytes` if both were applied (e.g. which one wins, or whether stacking them is redundant/harmless) — untested combination.

### Facts that still need a browser

- Full bodies of GHSA-c9xm-49cp-xcr9, GHSA-9g45-5xwm-f3wc, and GHSA-33f5-2c5q-wgwj (only summary + affected/patched version was pulled for these three; full descriptions were skipped since none affect the current 3.3.0 dependency and the project only plans a server role, not an rmcp OAuth *client* role).
- `complex_auth_streamhttp.rs` and `cimd_auth_streamhttp.rs` (the fuller OAuth2/Client-ID-Metadata-Document server-side examples) were not read — only the static-bearer-token example was, since it most directly matches "pre-shared bearer token in front of the transport."
- Whether `MaybeSend`/`MaybeSendFuture` resolve to a hard `Send` bound under rmcp's default feature set (the `MaybeSend` trait's own definition/feature-gating was not opened; this session only saw it used in `CallToolHandler` bounds via a documentation search tool, not the source file).

## 2. GitHub REST API and sealed boxes



Primary source for all GitHub REST facts below (unless noted otherwise) is the published OpenAPI
description for the REST API, fetched directly: https://raw.githubusercontent.com/github/rest-api-description/main/descriptions/api.github.com/api.github.com.json
(13MB, pretty-printed JSON; queried locally with `jq` rather than loaded whole). This is the same
machine-readable spec that generates docs.github.com's REST pages, so its `description` strings are
the exact prose GitHub publishes, and its schemas are authoritative for field names/requiredness —
more reliable than the rendered docs.github.com HTML, which for a few things (fine-grained PAT
permission tables) is populated by client-side JS from a non-public data source and could only be
read via WebFetch's rendering. Rust crate facts are sourced from crates.io's API and from the crates'
own GitHub source (raw file fetch + `jq`/`grep`), not from docs.rs summaries, wherever the two
disagreed — docs.rs / WebSearch snippets for `crypto_box` described a `SealedBox` struct that does
not exist in the actual crate; the real API (verified against source) is `PublicKey::seal` /
`SecretKey::unseal`.

### GitHub REST API — repositories

**Recommendation:** Use `GET /repos/{owner}/{repo}` to check existence: 200 means it exists, 404
("Resource not found", `basic-error` schema — `message`/`documentation_url`/`url`/`status`, none
marked required) means it doesn't (or is private and inaccessible to your token — GitHub
deliberately conflates the two). Create with `POST /orgs/{org}/repos`, body `{name}` required,
`private`/`visibility`/`auto_init`/`has_issues`/`has_projects`/`has_wiki`/`has_downloads` optional
(all four `has_*` flags default `true`, `private`/`auto_init` default `false`). On name collision
expect `422` shaped as the `validation-error` schema (`message`+`documentation_url` required,
optional `errors[]` with `resource`/`field`/`message`/`code`/`index`/`value`, `code` one of
`missing`/`missing_field`/`invalid`/`already_exists`/`unprocessable`/`custom`) — a real captured
example uses `code: "already_exists"` on `field: "name"`. Use `PATCH /repos/{owner}/{repo}` with
`visibility`/`private` to change visibility later; note the API itself documents a `422` if an org
restricts visibility changes to owners and a non-owner tries. Treat repo names as case-insensitive
for uniqueness (do not rely on case to disambiguate two desired names) — GitHub's own docs used to
claim repo names are case-sensitive and that claim has since been removed from the current page.
The OpenAPI schema declares no `pattern`/`maxLength` on the `name` field at all — GitHub enforces
the 100-char / ASCII-only / no-trailing-`.git` rule server-side (surfaced as UI error strings), not
as a machine-readable constraint, so client-side validation of a candidate name is inherently a
best-effort mirror of undocumented server behavior, not something you can derive from the spec.

- `GET /repos/{owner}/{repo}` declares responses `200`, `301` (moved permanently — repo renamed), `403`, `404`. (source: https://raw.githubusercontent.com/github/rest-api-description/main/descriptions/api.github.com/api.github.com.json, `.paths."/repos/{owner}/{repo}".get.responses`)
  - `["200","301","403","404"]`
- The `404` response is `{"description": "Resource not found", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/basic-error"}}}}`, and `basic-error` has properties `message`, `documentation_url`, `url`, `status` with **no required array at all**. (source: same file, `.components.responses.not_found` and `.components.schemas."basic-error"`)
  - "Basic Error" / "type": "object" / properties message, documentation_url, url, status (no `required` key present)
- The `422` response used by repo creation is the shared `validation-error` schema: `required: ["message","documentation_url"]`, plus an optional `errors[]` array whose items require `code` and may carry `resource`, `field`, `message`, `index`, `value`. (source: same file, `.components.schemas."validation-error"`)
  - "required": ["message", "documentation_url"] ... "errors": {"type": "array", "items": {"required": ["code"], "properties": {"resource":..., "field":..., "message":..., "code":..., "index":..., "value":...}}}
- `troubleshooting-the-rest-api.md`'s own `code` table for `errors[].code`: `missing` = "A resource does not exist.", `already_exists` = "Another resource has the same value as one of your parameters. This can happen in resources that must have some unique key (such as label names)." (source: https://raw.githubusercontent.com/github/docs/main/content/rest/using-the-rest-api/troubleshooting-the-rest-api.md)
  - "`missing` | A resource does not exist." / "`already_exists` | Another resource has the same value as one of your parameters..."
- A real captured `422` body for a duplicate repo name (user-pasted, not from official docs — **(unverified)**): (source: https://github.com/orgs/community/discussions/53911 via WebSearch snippet)
  - `{"message":"Repository creation failed.","errors":[{"resource":"Repository","code":"custom","field":"name","message":"name already exists on this account"}],"documentation_url":"https://developer.github.com/v3/repos/#create"}`
- `POST /orgs/{org}/repos` request body: `name` is the only required field; `private` (default `false`), `visibility` (enum `public`/`private`), `auto_init` (default `false`, "Pass `true` to create an initial commit with empty README."), `has_issues`/`has_projects`/`has_wiki`/`has_downloads` all default `true`. (source: same OpenAPI file, `.paths."/orgs/{org}/repos".post.requestBody...schema`)
  - required: ["name"]; auto_init: "Pass `true` to create an initial commit with empty README."; has_projects: "...if you're creating a repository in an organization that has disabled repository projects, the default is `false`, and if you pass `true`, the API returns an error."
- `PATCH /repos/{owner}/{repo}` `private` field description includes the exact org-restriction caveat. (source: same file, `.paths."/repos/{owner}/{repo}".patch.requestBody...schema.properties.private`)
  - "Either `true` to make the repository private or `false` to make it public. Default: `false`.  \n**Note**: You will get a `422` error if the organization restricts changing repository visibility to organization owners and a non-owner tries to change the value of private."
- No `pattern`/`maxLength` is declared on the `name` property of either the create or patch request body schema. (source: same file, `jq '...properties.name.maxLength'` → `null`)
- A still-open github/docs community issue, whose body text I fetched and confirmed directly via the GitHub API, states repo names allow only ASCII letters/digits/`.`/`-`/`_`, max 100 chars, must not end in `.git` or `.wiki`, and non-ASCII chars are auto-converted to `-` with a UI hint. **(unverified — community report, not an official reference page)** (source: https://github.com/github/docs/issues/44518, fetched via `api.github.com/repos/github/docs/issues/44518`)
  - "Maximum length: 100 characters... Allowed characters: ASCII letters, digits, and the characters `.`, `-`, and `_`... Repository names must not end with `.git`... Repository names must not end with `.wiki`."
- `docs.github.com`'s live `troubleshooting-cloning-errors.md` no longer contains any claim that repo names are case-sensitive (I grepped the current raw file for "case" and got zero matches); a 2024 github/docs issue I fetched directly reports the old wording as false, backed by a `git clone` transcript showing a repo cloned successfully under two different cases. (source: https://raw.githubusercontent.com/github/docs/main/content/repositories/creating-and-managing-repositories/troubleshooting-cloning-errors.md and https://github.com/github/docs/issues/32838)
  - issue #32838: "This is false." followed by a `git clone git@github.com:cOmMuNiTy/CoMmUnItY.git` transcript succeeding

**Unresolved**

- No single official docs.github.com page states the repo-name character-set/length rule as a citable sentence (only a community issue report, confirmed to exist but not "official"); a browser session could check whether GitHub has since added this to a canonical reference page.
- Whether `.git`/`.wiki` suffix rejection happens client-side (web UI hint only) or is also enforced by the REST API itself (i.e. does `POST /orgs/{org}/repos` with `name: "foo.wiki"` 422, or silently strip/accept it?) was not tested and isn't in the OpenAPI schema.

### GitHub REST API — repository topics

**Recommendation:** `managed-by-willikins` is a legal topic: GitHub's own rule is lowercase
letters, numbers, and hyphens, ≤50 characters, ≤20 topics per repo — the candidate string satisfies
all three trivially. Use `PUT /repos/{owner}/{repo}/topics` with `{"names": [...]}` to replace the
full topic set (idempotent — sending the same array twice is a no-op); `GET` the same path to read
it back. GitHub lowercases whatever you send, so there's no need to pre-lowercase client-side beyond
matching what you expect to compare against later.

- The canonical rule, verbatim from the current docs source: "When creating a topic: * Use lowercase letters, numbers, and hyphens. * Use 50 characters or less. * Add no more than 20 topics." (source: https://raw.githubusercontent.com/github/docs/main/content/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics.md)
  - "When creating a topic:\n* Use lowercase letters, numbers, and hyphens.\n* Use 50 characters or less.\n* Add no more than 20 topics."
- `PUT /repos/{owner}/{repo}/topics` request body: `names` (array of string, required) — "Pass one or more topics to _replace_ the set of existing topics. Send an empty array (`[]`) to clear all topics from the repository. **Note:** Topic `names` will be saved as lowercase." (source: OpenAPI file above, `.paths."/repos/{owner}/{repo}/topics".put.requestBody...`)
  - required: ["names"]; description as quoted above verbatim
- `GET /repos/{owner}/{repo}/topics` returns the same shape, `{"names": [...]}`, 200. (source: WebFetch of https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#get-all-repository-topics)
  - `* names: required, array of string`

**Unresolved**

- Whether a topic string is validated server-side against the 50-char/lowercase rule at the API level (rejecting a bad value) versus silently mutating it (lowercasing, truncating) — the OpenAPI schema puts no `pattern`/`maxLength` on the `names` array items, so this is unconfirmed from the spec alone.

### GitHub REST API — Actions repository secrets

**Recommendation:** Flow is: `GET /repos/{owner}/{repo}/actions/secrets/public-key` → get `key_id`
+ base64 `key`; seal the plaintext secret with that public key (see Rust section below); `PUT
/repos/{owner}/{repo}/actions/secrets/{secret_name}` with `{encrypted_value, key_id}` where
`encrypted_value` is itself base64 (the schema even declares a base64-charset regex on that field).
201 on first creation, 204 on every subsequent update of the same name — treat both as success.
`GET .../actions/secrets/{secret_name}` returns only `{name, created_at, updated_at}` (never the
value) and is useful only to check existence/staleness, not content. Enforce secret-name rules
client-side before calling: alphanumeric or `_` only, must not start with a digit, must not start
with `GITHUB_`, case-insensitive (GitHub uppercases on store) — the API will presumably 422 on a bad
name via the shared `validation-error` shape, but I did not find a documented example of that
specific body.

- `public-key` response schema `actions-public-key`: `key_id` and `key` both required strings; `key` is explicitly "The Base64 encoded public key." (source: OpenAPI file, `.components.schemas."actions-public-key"`)
  - "key": {"description": "The Base64 encoded public key.", "type": "string", "example": "hBT5WZEj8ZoOv6TYJsfWq7MxTEQopZO5/IT3ZCVQPzs="}, required: ["key_id","key"]
- `PUT .../actions/secrets/{secret_name}` request body requires `encrypted_value` and `key_id`; `encrypted_value`'s schema carries a base64-alphabet `pattern`, and its description points at "Encrypt your secret using LibSodium". (source: OpenAPI file, `.paths."/repos/{owner}/{repo}/actions/secrets/{secret_name}".put.requestBody...`)
  - "encrypted_value": {"description": "Value for your secret, encrypted with [LibSodium](...) using the public key retrieved from the Get a repository public key endpoint.", "pattern": "^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=|[A-Za-z0-9+/]{4})$"}, required: ["encrypted_value","key_id"]
- `PUT` responses are `201` ("Response when creating a secret", body `{}`/null example) and `204` ("Response when updating a secret", no body). (source: same file, `.paths...put.responses`)
  - responses keys: ["201","204"]; 201.description: "Response when creating a secret"; 204.description: "Response when updating a secret"
- `GET .../actions/secrets/{secret_name}` schema `actions-secret` requires `name`, `created_at`, `updated_at` — no value field, ever. The OpenAPI `responses` object for this operation only declares `200` (no `404` entry present in the spec, though ordinary GitHub 404 semantics would presumably still apply for a missing name). (source: OpenAPI file, `.components.schemas."actions-secret"` and `.paths...get.responses`)
  - required: ["name", "created_at", "updated_at"]; responses keys: ["200"]
- Secret naming rules and size limit, from the rendered secrets reference page (raw markdown source for this page could not be located at its expected path; quoted via WebFetch of the rendered page): (source: https://docs.github.com/en/actions/reference/secrets-reference)
  - "Can only contain alphanumeric characters (`[a-z]`, `[A-Z]`, `[0-9]`) or underscores (`_`). Spaces are not allowed." / "Must not start with the `GITHUB_` prefix." / "Must not start with a number." / "Are case insensitive when referenced. GitHub stores secret names as uppercase regardless of how they are entered." / secrets "limited to 48 KB in size" / org limit 1,000 secrets, repo limit 100, environment limit 100
- The encryption guide's canonical description and Python (PyNaCl) example, matching two independent fetches (WebFetch of rendered page + WebFetch of raw markdown from github/docs): (source: https://docs.github.com/en/rest/guides/encrypting-secrets-for-the-rest-api and https://raw.githubusercontent.com/github/docs/main/content/rest/guides/encrypting-secrets-for-the-rest-api.md)
  - "To use these endpoints, you must encrypt the secret value using libsodium."
  - ```python
    from base64 import b64encode
    from nacl import encoding, public

    def encrypt(public_key: str, secret_value: str) -> str:
      """Encrypt a Unicode string using the public key."""
      public_key = public.PublicKey(public_key.encode("utf-8"), encoding.Base64Encoder())
      sealed_box = public.SealedBox(public_key)
      encrypted = sealed_box.encrypt(secret_value.encode("utf-8"))
      return b64encode(encrypted).decode("utf-8")
    ```

**Unresolved**

- Could not locate the raw markdown source file for the "Secrets reference" page in github/docs at any guessed path (tried `content/actions/reference/secrets-reference.md`, got 404); the naming-rule facts above come from WebFetch's rendering rather than a grep of primary markdown, and GitHub's code search requires auth I don't have in this session — a browser or authenticated `gh api` session could locate the exact file and re-quote it verbatim.
- No documented example of the `422` body produced by an invalid secret name (reserved `GITHUB_` prefix, leading digit, bad character) — only the generic `validation-error` schema shape is known to apply.
- Whether `PUT` with an unchanged `encrypted_value`/`key_id` for an existing secret still returns `204` (vs erroring) was not tested; the two documented outcomes are keyed only by "does this secret name already exist", not by whether the value differs.

### Rust implementations of libsodium's sealed box (`crypto_box_seal`)

**Recommendation:** Use `crypto_box = { version = "0.9", features = ["seal"] }` (pin `0.9`, not a
bare `*` or the unreleased `0.10.0-pre.0` — see below) and call `PublicKey::from_bytes(bytes)` (or
`PublicKey::from(bytes)`) then `public_key.seal(&mut rng, plaintext)` on the recipient side, or
`secret_key.unseal(ciphertext)` to open. **Correction to my own initial assumption**: several
docs.rs/WebSearch summaries describe a `SealedBox` struct with `seal()`/`open()` methods (mirroring
PyNaCl's `nacl.public.SealedBox` naming) — that type does not exist in `crypto_box`. I verified this
by fetching the actual tagged source (`crypto_box-v0.9.1` on GitHub): sealing is a method on
`PublicKey`, opening is a method on `SecretKey`, named `seal`/`unseal`, not a separate struct.
`crypto_box`'s `seal` feature is off by default and pulls in `blake2` (used to derive the sealed-box
nonce from the two public keys) plus the crate's own `alloc` feature. The default features already
give you `getrandom` + `rand_core` (transitively, via the `aead` crate's own feature flags) so
`&mut rng` can be `rand::thread_rng()` or any `CryptoRngCore`. `dryoc = "1.0.0"` is a solid
alternative if you'd rather not depend on the RustCrypto/`aead` ecosystem: `DryocBox::seal_to_vecbox`
/`unseal_to_vec` do the same job with no feature flag required (its seal API isn't behind a Cargo
feature at all), at the cost of a heavier, self-contained crypto stack (its own `chacha20`,
`salsa20`, `sha2`/`sha3` deps) rather than composing with the smaller RustCrypto crates already
likely in the dependency tree. `sodiumoxide` is dead — last release 0.2.7, years old, explicit
"reached the end of its development" notice — do not add it.

- `crypto_box` on crates.io: `max_stable_version: "0.9.1"`, `newest_version: "0.10.0-pre.0"` (a pre-release; `cargo add crypto_box` / a `"0.9"` requirement will not pick this up). (source: https://crates.io/api/v1/crates/crypto_box, fetched directly with `curl` + a `User-Agent` header, which crates.io's API requires)
  - `{"max_stable_version": "0.9.1", "newest_version": "0.10.0-pre.0", "repository": "https://github.com/RustCrypto/nacl-compat"}`
- The crate actually lives in the `RustCrypto/nacl-compat` monorepo (not its own repo), tagged per-crate as `crypto_box-v0.9.1`. (source: same crates.io API response, `repository` field, cross-checked against `https://api.github.com/repos/RustCrypto/nacl-compat/tags`)
- `crypto_box` 0.9.1's real `Cargo.toml` `[dependencies]` and `[features]` (fetched from the tagged source, not the unreleased `master` branch, which is already at `0.10.0-pre.0` with several breaking version bumps — `aead = "0.6.0-rc.2"`, `curve25519-dalek = "=5.0.0-pre.1"`, feature renamed `getrandom`→`os_rng`): (source: https://docs.rs/crate/crypto_box/0.9.1/source/Cargo.toml)
  - dependencies: `aead = { version = "0.5.2", default-features = false }`, `blake2 = { version = "0.10", optional = true, default-features = false }`, `crypto_secretbox = { version = "0.1.1", default-features = false }`, `curve25519-dalek = { version = "4", features = ["zeroize"], default-features = false }`, `subtle = { version = "2", default-features = false }`, `zeroize = { version = "1", default-features = false }`
  - features: `default = ["alloc", "getrandom", "salsa20"]`; `seal = ["dep:blake2", "alloc"]`; `getrandom = ["aead/getrandom", "rand_core"]`; `rand_core = ["aead/rand_core"]`; `std = ["aead/std"]` — note `getrandom`/`rand_core` are not direct `crypto_box` dependencies at all, only feature-flag pass-throughs to the `aead` crate
- The actual sealed-box API, from the tagged source (`crypto_box/src/public_key.rs` and `secret_key.rs`), is a method on `PublicKey`/`SecretKey`, not a `SealedBox` type: (source: https://raw.githubusercontent.com/RustCrypto/nacl-compat/crypto_box-v0.9.1/crypto_box/src/public_key.rs and .../secret_key.rs)
  - ```rust
    /// Implementation of `crypto_box_seal` function from libsodium "sealed boxes".
    #[cfg(feature = "seal")]
    pub fn seal(
        &self,
        csprng: &mut impl CryptoRngCore,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, aead::Error>
    ```
  - ```rust
    /// Implementation of `crypto_box_seal_open` function from libsodium "sealed boxes".
    #[cfg(feature = "seal")]
    pub fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>, aead::Error>
    ```
  - (`seal` is defined on `PublicKey`; `unseal` is defined on `SecretKey`.)
- `PublicKey` construction from raw 32 bytes: both `from_bytes` and a `From` impl exist. (source: same `public_key.rs` file)
  - `pub fn from_bytes(bytes: [u8; KEY_SIZE]) -> Self { PublicKey(MontgomeryPoint(bytes)) }` and `impl From<[u8; KEY_SIZE]> for PublicKey`
- `dryoc` on crates.io: `max_stable_version`/`newest_version` both `"1.0.0"`, repository `github.com/brndnmtthws/dryoc`. Its `Cargo.toml` declares `rust-version = "1.89"`, `edition = "2024"`, and a `[features]` block (`base64`, `u64_backend`, `protected`, `simd_backend`, `wincode`, `nightly`) that contains **no `seal` feature at all** — sealing is always compiled in. (source: https://crates.io/api/v1/crates/dryoc and https://raw.githubusercontent.com/brndnmtthws/dryoc/v1.0.0/Cargo.toml)
  - `{"max_stable_version": "1.0.0", "newest_version": "1.0.0"}`; `[features]\nbase64 = []\ndefault = ["base64", "u64_backend", "protected"]\n...` (no `seal` key)
- `dryoc`'s sealed-box API, from tagged source `src/dryocbox.rs`: `DryocBox::seal`/`seal_to_vecbox` (encrypt to a fresh recipient, generating an ephemeral keypair internally) and `unseal_to_vec` (decrypt with the recipient's keypair). (source: https://raw.githubusercontent.com/brndnmtthws/dryoc/v1.0.0/src/dryocbox.rs)
  - `pub fn seal<Message: Bytes + ?Sized, RecipientPublicKey: ByteArray<CRYPTO_BOX_PUBLICKEYBYTES>>(message: &Message, recipient_public_key: &RecipientPublicKey) -> Result<Self, Error>`
  - `pub fn seal_to_vecbox<Message: Bytes + ?Sized>(message: &Message, recipient_public_key: &PublicKey) -> Result<Self, Error> { Self::seal(message, recipient_public_key) }`
  - `pub fn unseal_to_vec<RecipientPublicKey: ..., RecipientSecretKey: ...>(&self, recipient_keypair: &crate::keypair::KeyPair<RecipientPublicKey, RecipientSecretKey>) -> Result<Vec<u8>, Error>`
  - doc comment: "`DryocBox::seal` instead sends an anonymous sealed box: it generates a one-time ephemeral keypair and stores the ephemeral public key with the ciphertext."
- `sodiumoxide` is explicitly end-of-life: `max_stable_version`/`newest_version` both `"0.2.7"` on crates.io (an old release), and its README says outright it will not be updated further. (source: https://crates.io/api/v1/crates/sodiumoxide and https://raw.githubusercontent.com/sodiumoxide/sodiumoxide/master/README.md via WebFetch — the raw README fetch itself 403'd for me, so this specific quote is via WebFetch's summary of the GitHub-hosted page, not a raw-file grep)
  - "This project has reached the end of its development as a cryptographic library for rust. Feel free to browse the code, and feel free to use it, but it will not see any more updates (unless a security issue arises, those will be fixed)."

**Unresolved**
- Whether `crypto_box`'s `0.10.0-pre.0` (unreleased on `master`) keeps the same `PublicKey::seal`/`SecretKey::unseal` method names, or restructures the sealed-box API further — I only diffed the two `Cargo.toml` files (dependency/feature churn is substantial: `aead` 0.5→0.6-rc, `curve25519-dalek` 4→5-pre, `getrandom` feature renamed `os_rng`), not the `0.10.0-pre.0` source itself. Re-check before ever bumping past `0.9.x`.
- `crypto_box`'s Cure53 security-audit claim (mentioned in a docs.rs-derived WebFetch summary) was not independently re-verified against a primary audit report or the RustCrypto repo's own README — treat as **(unverified)** if it matters for the design doc.
- Did not benchmark or otherwise compare `crypto_box` vs `dryoc` performance/binary-size; the recommendation above is about ecosystem fit (RustCrypto composability vs self-contained), not measured cost.

### GitHub authentication — token prefixes, permissions, required headers

**Recommendation:** Detect token type by prefix for logging/diagnostics only (never gate behavior
solely on prefix, since GitHub could add more): `ghp_` classic PAT, `github_pat_` fine-grained PAT,
`gho_` OAuth, `ghu_`/`ghs_`/`ghr_` GitHub App user/installation/refresh tokens. For a
fine-grained PAT used by willikins, request repository-level `Administration: write` (repo create
implies org-level admin, or a pre-existing repo's admin settings), `Secrets: write` (Actions
secrets), and `Metadata: read` (implied automatically, but needed for the existence-check `GET`).
If the PAT is org-owned, expect it to sit in a **pending** state needing an org owner's approval
before it can do anything beyond read public resources — this is the *default* org policy (owners
can disable the approval requirement, and an owner's own tokens skip approval). Always send
`Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`, and a real `User-Agent` —
GitHub's own docs say requests with no `User-Agent` are flatly rejected.

- Token-prefix table, straight from the current authentication overview doc's raw markdown: (source: https://raw.githubusercontent.com/github/docs/main/content/authentication/keeping-your-account-and-data-secure/about-authentication-to-github.md)
  - "| Personal access token (classic) | `ghp_` | ... |\n| Fine-grained personal access token | `github_pat_` | ... |\n| OAuth access token | `gho_` | ... |\n| User access token for a GitHub App | `ghu_` | ... |\n| Installation access token for a GitHub App | `ghs_` | ... |\n| Refresh token for a GitHub App | `ghr_` | ... |"
- This same page states no character-length or charset rule for the token body after the prefix — I grepped the raw file for "40 character"/"22 character"/etc. and found nothing; a WebSearch summary separately mentioned installation tokens "transitioning to a new stateless format that may exceed the previously standard 40-character length", which I could not verify against a primary page. **(unverified)**
- Required headers, from the raw REST API getting-started guide: (source: https://raw.githubusercontent.com/github/docs/main/content/rest/using-the-rest-api/getting-started-with-the-rest-api.md)
  - "Most GitHub REST API endpoints specify that you should pass an `Accept` header with a value of `application/vnd.github+json`."
  - "#### `X-GitHub-Api-Version`\n\nYou should use this header to specify a version of the REST API to use for your request."
  - "All API requests must include a valid `User-Agent` header. The `User-Agent` header identifies the user or application that is making the request." followed (per-client-tool sections) by: "By default, `curl` sends a valid `User-Agent` header. However GitHub recommends using your GitHub username, or the name of your application, for the `User-Agent` header value."
- The `troubleshooting-the-rest-api.md` page is blunter about consequences: "Requests without a valid `User-Agent` header will be rejected." (source: same file as the repos-section troubleshooting quote above)
- Fine-grained PAT permission levels for the relevant endpoints (from WebFetch's rendering of the permissions page — the raw markdown at this path is a stub whose table is populated client-side from data not present in the public github/docs or rest-api-description repos, so this is **not** re-verified against a primary text source): (source: https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens)
  - `POST /orgs/{org}/repos` needs "write" Repository/Administration; `GET /repos/{owner}/{repo}` needs "read" Repository/Metadata; Actions secrets endpoints need "read"/"write" Secrets permission
- Org-owned fine-grained PAT approval defaults to required, with owner-created tokens exempt: (source: https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/setting-a-personal-access-token-policy-for-your-organization, via WebFetch)
  - "Require administrator approval" is "the default value"; "tokens created by organization owners themselves don't need approval"; public resources remain accessible regardless of the setting

**Unresolved**
- No primary-source confirmation of exact token length/charset per prefix (e.g. the commonly-cited "`ghp_` + 36 alphanumeric = 40 chars total" convention) — not stated in the fetched docs; would need a browser/community source to pin down, and GitHub could change it without notice per the installation-token remark above.
- The fine-grained-PAT permission-table facts came only from WebFetch's rendering, not a grep of primary markdown or the OpenAPI spec (the permission matrix isn't in either public repo I could reach) — a browser session authenticated to GitHub could screenshot/copy the live table for a stronger citation.

### GitHub REST API — rate limits and retries

**Recommendation:** Watch `x-ratelimit-remaining` and `retry-after` on every response rather than
pre-computing budgets. Primary limit for an authenticated PAT is 5,000 requests/hour (15,000/hour
only for GitHub-Cloud-org-owned Apps/OAuth apps acting on a Cloud-org member's behalf). Secondary
limits matter more for willikins' write-heavy workload: no more than 900 "points"/minute on REST,
where a `GET`/`HEAD`/`OPTIONS` costs 1 point and `POST`/`PATCH`/`PUT`/`DELETE` cost 5 — so a
provisioning run doing repo-create + topics-put + public-key-get + secret-put per project is mostly
5-point writes and could hit the secondary limit well before the primary one on a big batch. On
`403`/`429`: obey `retry-after` verbatim if present; otherwise if `x-ratelimit-remaining` is `0`
wait until `x-ratelimit-reset`; otherwise wait ≥1 minute and back off exponentially on repeat
failures.

- Rate-limit response headers, verbatim from the raw doc: (source: https://raw.githubusercontent.com/github/docs/main/content/rest/using-the-rest-api/rate-limits-for-the-rest-api.md)
  - "`x-ratelimit-limit` | The maximum number of requests that you can make per hour\n`x-ratelimit-remaining` | The number of requests remaining in the current rate limit window\n`x-ratelimit-used` | The number of requests you have made in the current rate limit window\n`x-ratelimit-reset` | The time at which the current rate limit window resets, in UTC epoch seconds\n`x-ratelimit-resource` | The rate limit resource that the request counted against."
- Primary rate limit for authenticated users, from the reusable snippet the page includes: (source: https://raw.githubusercontent.com/github/docs/main/data/reusables/rest-api/primary-rate-limit-authenticated-users.md)
  - "All of these requests count towards your personal rate limit of 5,000 requests per hour. ...requests made on your behalf by a GitHub App that is owned by a GitHub Enterprise Cloud organization have a higher rate limit of 15,000 requests per hour."
- Secondary rate limits and their point costs, from the reusable snippet: (source: https://raw.githubusercontent.com/github/docs/main/data/reusables/rest-api/secondary-rate-limit-rest-graphql.md)
  - "No more than 100 concurrent requests are allowed. This limit is shared across the REST API and GraphQL API." / "No more than 900 points per minute are allowed for REST API endpoints..." / "No more than 90 seconds of CPU time per 60 seconds of real time is allowed." / table: "Most REST API `GET`, `HEAD`, and `OPTIONS` requests | 1" and "Most REST API `POST`, `PATCH`, `PUT`, or `DELETE` requests | 5"
- Retry guidance, verbatim: (source: https://raw.githubusercontent.com/github/docs/main/content/rest/using-the-rest-api/troubleshooting-the-rest-api.md)
  - "If you exceed your primary rate limit, you will receive a `403 Forbidden` or `429 Too Many Requests` response, and the `x-ratelimit-remaining` header will be `0`." / "If the `retry-after` response header is present, you should not retry your request until after that many seconds has elapsed." / "If the `x-ratelimit-remaining` header is `0`, you should not make another request until after the time specified by the `x-ratelimit-reset` header." / "Otherwise, wait for at least one minute before retrying... wait for an exponentially increasing amount of time between retries, and throw an error after a specific number of retries."

**Unresolved**
- Whether the Actions-secrets endpoints (`public-key` GET, secret PUT) have a documented non-standard point cost ("Some REST API endpoints have a different point cost that is not shared publicly" per the secondary-rate-limit reusable) — could not find a per-endpoint override for these specifically.

### Idempotency hazards

**Recommendation:** `POST /orgs/{org}/repos` is **not** safely retriable on ambiguous
failure (timeout, connection drop) without a existence-check first: a retried create after a
first attempt actually succeeded server-side will `422` with `errors[].code: "already_exists"`
on `field: "name"` (see repos section above) — treat that specific 422 shape as "probably already
created, go verify with `GET`" rather than a hard failure. `PUT
/repos/{owner}/{repo}/actions/secrets/{secret_name}` **is** safely retriable in the sense that
matters operationally: calling it twice with a freshly-sealed ciphertext of the same plaintext
converges to the same final secret value, and the response code alone (`201` vs `204`) tells you
whether you created or overwrote. One subtlety worth flagging to implementers: because
`PublicKey::seal`/`DryocBox::seal` both generate a fresh ephemeral keypair per call (confirmed in
the source excerpts above — `crypto_box`'s `seal` calls `SecretKey::generate(csprng)` internally;
`dryoc`'s doc comment says the same), the `encrypted_value` byte string is different on every call
even for identical `(plaintext, key_id)` — so a naive "retry only if the request body differs"
dedup strategy will always think it's a new write. Idempotency here has to be judged by intent
(same plaintext secret) not by request-body equality.

- Basis for the repo-creation-retry claim is the same `already_exists` validation-error code documented in the repos section (source: OpenAPI `validation-error` schema `errors[].code` enum values, and `troubleshooting-the-rest-api.md`'s table — both cited above; not a new fact, cross-referenced rather than re-quoted here to save space).
- Basis for the secret-PUT idempotency claim is the documented `201`-then-`204` behavior (source: OpenAPI file, `.paths."/repos/{owner}/{repo}/actions/secrets/{secret_name}".put.responses`, cited in full in the secrets section above).
- The ephemeral-keypair-per-seal behavior is directly visible in the source: `crypto_box`'s `PublicKey::seal` calls `let ephemeral_sk = SecretKey::generate(csprng);` on every invocation, and `dryoc`'s `DryocBox::seal` doc comment states it "generates a one-time ephemeral keypair" per call (both quoted in full above).

**Unresolved**
- No official GitHub doc states an idempotency policy explicitly for either endpoint — both conclusions above are derived by combining documented status/error codes with how the crypto libraries behave, not read directly off a single source that says "this endpoint is idempotent."
- Did not test empirically (no live GitHub org/token available in this research task) whether a retried `POST /orgs/{org}/repos` after a genuine mid-request timeout (as opposed to a deliberate duplicate call) actually surfaces the same `422`/`already_exists` shape, or something else (e.g. a `409`) — the evidence here is a schema capability plus one community-pasted example, not a controlled reproduction.

### Facts that still need a browser

- The exact character length/charset of the token body following each prefix (`ghp_`, `github_pat_`, `ghs_`, etc.) — not published in any page I could fetch; GitHub's docs explicitly note installation tokens are moving to a longer "stateless format", so this may be a moving target best confirmed by decoding a real token rather than trusting a doc.
- The fine-grained-PAT permission-table entries (Administration/Secrets/Metadata levels for the specific endpoints willikins needs) came only from WebFetch's rendering of a client-side-populated table; a signed-in browser session (or `gh api` against a real fine-grained PAT's `/rate_limit` / token-introspection) would let you confirm the exact required scopes against a live token instead.
- The "Secrets reference" page's raw markdown source file could not be located in github/docs at any path I guessed (404), so its naming-rule quotes are WebFetch-rendered text, not a primary-file grep — worth relocating the correct file path with a browser or `gh repo view github/docs` clone.
- `sodiumoxide`'s deprecation-notice quote came through WebFetch after a direct `raw.githubusercontent.com` fetch of its README 403'd for me — re-fetching that raw URL from a browser/different network path would upgrade this from "WebFetch-rendered" to "grepped primary source."
- Whether `crypto_box` 0.10.0-pre.0 (currently unreleased) changes the `seal`/`unseal` method names or structure was only checked at the `Cargo.toml` dependency-version level, not against its actual (pre-release) source — revisit before ever moving off the `0.9` line.

## 3. Doppler API v3



Extends the Doppler section of `docs/research/2026-09-11-m1-dependencies.md` (project
creation, default environments, branch-config naming convention). Does not repeat those
facts. Method note for future sessions: `docs.doppler.com` publishes an `llms.txt` index
(https://docs.doppler.com/llms.txt) and every `/reference/*` and `/docs/*` page has a raw,
un-summarized markdown twin at the same path plus `.md` (e.g.
`https://docs.doppler.com/reference/projects-create.md`). Fetching the `.md` twin via plain
`curl` (not WebFetch) returns the page's embedded OpenAPI JSON verbatim, avoids WebFetch's
125-character paraphrase/quote-length limit, and is far lighter than the JS-rendered HTML
page — this avoided the 429s a previous session hit.

### Doppler authentication and token types

**Recommendation:** Provision (create projects/environments/configs/service tokens) with a
Personal Token or a Service Account Token whose workplace role grants the needed
permissions; never attempt provisioning with a Service Token, which Doppler's own docs
describe as scoped only to secrets access within one config. Doppler's docs say "six types
of tokens" but list seven (Service Account Identity Token is the extra, newer one) — a
documented off-by-one, not a research gap.

- Auth is a bearer token in the `Authorization` header; all requests must be HTTPS. (source: https://docs.doppler.com/reference/api)
  - "Authentication to the API is performed by providing a bearer token to the [HTTP Authorization] header." / `Authorization: Bearer <TOKEN>`
- The docs claim six token types, then enumerate seven. (source: https://docs.doppler.com/reference/api)
  - "There are six types of tokens:" followed by sections for CLI, Personal, Service, Service Account, Service Account Identity, SCIM, and Audit tokens (seven headings).
- CLI Tokens and Personal Tokens both have full account access; Service Tokens are secrets-only. (source: https://docs.doppler.com/reference/api)
  - "**CLI Tokens** * Provide read/write access to all resources on your account * Generated via the `login` command in the Doppler CLI"
  - "**Personal Tokens** * Provide read/write access to all resources on your account"
  - "**Service Tokens** * Provide read or read/write access to secrets within a specific config * Generated from the Project > Config > Access page"
- Service Account Tokens carry a granular, configurable permission set at the workplace level. (source: https://docs.doppler.com/reference/api)
  - "**Service Account Tokens** * Provide access to a granular set of resources within your workplace * These tokens are attached to a Service Account"
- SCIM tokens are read/write over users/groups; Audit tokens are read-only over users/groups. (source: https://docs.doppler.com/reference/api)
  - "**SCIM Tokens**: * Provide read/write access to users and groups within your workplace." / "**Audit Tokens** * Provide read-only access to users and groups within your workplace"
- Every token kind has an exact prefix and a fixed-length random suffix regex, with a worked example per kind. (source: https://docs.doppler.com/reference/auth-token-formats)
  - CLI: `/dp\.ct\.[a-zA-Z0-9]{40,44}/` — `dp.ct.bAqhcVzrhy5cRHkOlNTc0Ve6w5NUDCpcutm8vGE9myi`
  - Personal: `/dp\.pt\.[a-zA-Z0-9]{40,44}/`
  - Service: `/dp\.st\.(?:[a-z0-9\-_]{2,35}\.)?[a-zA-Z0-9]{40,44}/` — `dp.st.dev.bAqhcVzrhy5cRHkOlNTc0Ve6w5NUDCpcutm8vGE9myi`
  - Service Account: `/dp\.sa\.[a-zA-Z0-9]{40,44}/`
  - Service Account Identity (short lived): `/dp\.said\.[a-zA-Z0-9]{40,44}/`
  - SCIM: `/dp\.scim\.[a-zA-Z0-9]{40,44}/`
  - Audit: `/dp\.audit\.[a-zA-Z0-9]{40,44}/`
- A service token's optional leading segment (before the random suffix) is lowercase alphanumeric/hyphen/underscore, 2-35 chars; the random suffix itself is exactly 40-44 plain alphanumeric characters, with no dots/hyphens inside it. (source: https://docs.doppler.com/reference/auth-token-formats)
  - `/dp\.st\.(?:[a-z0-9\-_]{2,35}\.)?[a-zA-Z0-9]{40,44}/`
- A Service Account's permissions come either from an existing role identifier or an explicit permissions array, never both. (source: https://docs.doppler.com/reference/service_accounts-create)
  - "You may provide an identifier OR permissions, but not both" ... example response: `"workplace_role": {"name": "Custom", "permissions": ["team"], "identifier": "custom", ...}`

**Unresolved**

- No page states which specific token kind(s) Doppler *recommends* for automation/provisioning tooling versus interactive human use beyond the capability descriptions above.

### Doppler projects

**Recommendation:** Query and mutate projects by `name` only — the API's project object has
no separate `slug` field distinct from `name`; the same string used to create a project is
what every other endpoint's `project` parameter expects. No fetched page documents the
duplicate-name-on-create error or any non-200 response at all (see the global "Facts that
still need a browser" list below).

- `POST /v3/projects` requires `name`; `description` is optional; the response is `{"project": {...}}` with no `slug` field. (source: https://docs.doppler.com/reference/projects-create)
  - `"required": ["name"]` ... response: `{"project": {"id": "ed0c2a68b6", "name": "Compression", "description": "Super rad middle-out compression algo.", "created_at": "2019-03-26T03:16:20.233Z"}}`
- `GET /v3/projects/project`'s `project` query parameter is documented the same way as the create body's `name` semantics — "Unique identifier for the project object" — and returns the identical four-field shape (`id`, `name`, `description`, `created_at`). (source: https://docs.doppler.com/reference/projects-get)
  - `"name": "project", "in": "query", "description": "Unique identifier for the project object.", "required": true`
- Project Name length: minimum 2, maximum 80 characters. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Project Name | 2 characters | 80 characters |"
- Developer/Team/Enterprise plans cap total projects at 10 / 250 / 1000 (\*case-by-case increase). (source: https://docs.doppler.com/docs/platform-limits)
  - "| Projects | 10 | 250 | -- | 1000 \* |"

### Doppler environments

**Recommendation:** `POST /v3/environments` needs **both** `name` and `slug` in the body —
it is not name-only with server-derived slug, contrary to what might be assumed. Neither
field carries a regex in the OpenAPI schema (both are bare `"type": "string"`); the only
documented constraint is length. The response's `project` field is the project's opaque
internal `id` (e.g. `ed0c2a68b6`), not the project name — a response-shape gotcha shared with
configs (see below). **Corrected 2026-09-14 by the live write cycle:** against a real
workplace, `project.id`, `project.name` and `project.slug` all hold the project name, and the
`project` field of every other object (config, environment, token) is that name too. The
fixtures were changed to match; the `ed0c2a68b6` example above is the OpenAPI example, not
live behaviour.

- `POST /v3/environments` request body requires both `name` and `slug`; query requires `project` (by name); `personal_configs` is an optional boolean, default `false`. (source: https://docs.doppler.com/reference/environments-create)
  - `"required": ["name", "slug"]` ... `"personal_configs": {"type": "boolean", "description": "Whether or not to enable personal configs for the environment", "default": false}`
- Neither `name` nor `slug` carries a `pattern`/character-set constraint in the schema — both are declared as plain `{"type": "string"}`. (source: https://docs.doppler.com/reference/environments-create)
  - property definitions: `"name": {"type": "string"}, "slug": {"type": "string"}`
- The environment object returned by create/get has `id`, `name`, `initial_fetch_at`, `created_at`, `project` — no separate `slug` key; `id` holds the slug-like value (e.g. `"dev"`), and `project` holds the project's opaque id, not its name. (source: https://docs.doppler.com/reference/environments-get)
  - `{"environment": {"id": "dev", "name": "Development", "initial_fetch_at": "2019-11-21T03:45:47.982Z", "created_at": "2019-11-19T07:19:00.476Z", "project": "ed0c2a68b6"}}`
- `GET /v3/environments/environment`'s `environment` query parameter is explicitly the environment's slug. (source: https://docs.doppler.com/reference/environments-get)
  - `"name": "environment", "in": "query", "description": "The environment's slug", "required": true`
- Environment Name and Environment Slug are each bounded 2-50 characters. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Environment Name | 2 characters | 50 characters |" / "| Environment Slug | 2 characters | 50 characters |"
- Environments per project cap: Developer 4, Team/Enterprise 15 (\*case-by-case). (source: https://docs.doppler.com/docs/platform-limits)
  - "| Environments per Project | 4 | 15 | -- | 15 \* |"

**Unresolved**

- Whether the environment `slug` field accepts underscores, only hyphens, or both — the OpenAPI schema gives no character class, only the 2-50 length bound from Platform Limits.

### Doppler configs (branch configs)

**Recommendation:** `POST /v3/configs` takes `project`, `environment` (the environment's
`id`/slug, e.g. `"prd"`, per its own description "Identifier for the environment object"),
and `name` for the new branch config. The open question that most affects milestone 2's
naming code: **whether the caller must already pass the fully environment-prefixed name
(`"prd_aws"`) or whether Doppler prepends the environment itself** is not resolvable from the
OpenAPI example alone (see Unresolved) — confirm empirically before writing a `v2` naming row
for branch configs.

- `POST /v3/configs` requires `project`, `environment`, `name`; `environment`'s description is "Identifier for the environment object", default placeholder `"ENVIRONMENT_ID"`. (source: https://docs.doppler.com/reference/configs-create)
  - `"required": ["project", "environment", "name"]` ... `"environment": {"type": "string", "description": "Identifier for the environment object.", "default": "ENVIRONMENT_ID"}`
- The create-response example is a branch config: `name: "prd_aws"`, `root: false`, `environment: "prd"`. No request-body example is shown alongside it, so it doesn't establish whether the caller supplied `"prd_aws"` or just `"aws"`. (source: https://docs.doppler.com/reference/configs-create)
  - `{"config": {"name": "prd_aws", "root": false, "locked": false, ..., "environment": "prd", "project": "ed0c2a68b6"}}`
- `GET /v3/configs/config` returns the same config-object shape: `name`, `root`, `locked`, `initial_fetch_at`, `last_fetch_at`, `created_at`, `environment`, `project`. (source: https://docs.doppler.com/reference/configs-get)
  - identical `{"config": {"name": "prd_aws", "root": false, "locked": false, ...}}` example.
- Config Slug length is 2-60 characters, **and that cap counts the environment-slug prefix**. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Config Slug | 2 characters | 60 characters \*\*\*\*\* |" ... "This limit includes the length of the Environment slug prefix, so a config named `dev_test` has a length of 8."
- Configs per environment cap: Developer 10, Team/Enterprise 100. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Configs per Environment | 10 | 100 | -- | 100 |"

**Unresolved**

- Whether `POST /v3/configs`'s `name` must already include the `<environment>_` prefix or whether Doppler applies it server-side. The conceptual branch-configs page (already cited in the milestone 1 doc) describes a dashboard modal that "will be prefixed with the environment name" for you, which suggests UI-side (or server-side) prefixing — but that page describes the web UI, not this JSON endpoint, and no request/response pair here demonstrates the transform. Needs a real API call (e.g. `name: "aws"` against environment `"prd"`) to see whether the stored config ends up named `"aws"` or `"prd_aws"`.
- Whether the config `name` field enforces `[a-z0-9_]+` (as `DopplerConfigName` assumes) or something looser — the OpenAPI schema gives only `{"type": "string"}`, no pattern.

### Doppler service tokens

**Recommendation:** Create with `POST /v3/configs/config/tokens`; `access` is the literal
string `"read"` or `"read/write"` (not `"read-write"`). The response's `key` field is the
only time the raw `dp.st.*` value is ever returned — list/get never show it again, and the
list endpoint's example doesn't even include the `access` field, so an existing token's
access level isn't recoverable from `GET /v3/configs/config/tokens` per the documented shape.
Revoke by `slug` (a server-assigned UUID) or by the raw `token` value.

- `POST /v3/configs/config/tokens` requires `project`, `config`, `name`; optional `expire_at` (date-time) and `access` (enum `["read", "read/write"]`, default `"read"`). (source: https://docs.doppler.com/reference/service_tokens-create)
  - `"required": ["project", "config", "name"]` ... `"access": {"type": "string", "description": "Token's capabilities.", "default": "read", "enum": ["read", "read/write"]}`
- Create response includes the one-time `key`, plus `name`, `slug` (a UUID), `created_at`, `config`, `environment`, `project`, `expires_at`, `access`. (source: https://docs.doppler.com/reference/service_tokens-create)
  - `{"token": {"name": "AWS Lambda", "slug": "56c69f96-3045-11ea-978f-2e728ce88125", "created_at": "2019-11-19T07:19:01.073Z", "key": "dp.st.gJ23agW5s09x4TKLMJMc4OPIr9fCm3bIs0QAC2L5", "config": "dev", "environment": "dev", "project": "ed0c2a68b6t", "expires_at": null, "access": "read"}}`
- `GET /v3/configs/config/tokens` (list) response's token objects omit `key` (expected — it's a secret) **and also omit `access`**, unlike the create response. (source: https://docs.doppler.com/reference/service_tokens-list)
  - `{"tokens": [{"name": "AWS Lambda", "slug": "56c69f96-3045-11ea-978f-2e728ce88125", "created_at": "...", "config": "dev", "environment": "dev", "project": "ed0c2a68b6t", "expires_at": null}]}`
- `DELETE /v3/configs/config/tokens/token` requires `project` and `config` in the body; `slug` and `token` are both present as optional properties (the schema's `required` array lists only `project`/`config`), so the token is identified by whichever of `slug`/`token` is supplied. Response is `{"success": true}`. (source: https://docs.doppler.com/reference/service_tokens-delete)
  - `"required": ["project", "config"]` ... properties include `"slug": {"description": "The slug of the service token."}` and `"token": {"description": "The token value."}` ... response `{"success": true}`
- Doppler's own example token `name` is a free-form display string ("AWS Lambda": mixed case, a space) and the `name` property's schema is a bare `{"type": "string"}` with no pattern — nothing in the API requires kebab-case. (source: https://docs.doppler.com/reference/service_tokens-create)
  - `"name": {"type": "string", "description": "Name of the service token.", "default": "TOKEN_NAME"}`
- Service Token Name length: 2-100 characters. Service Tokens per plan: Developer 50, Team 500, Enterprise 1000 (\*case-by-case). (source: https://docs.doppler.com/docs/platform-limits)
  - "| Service Token Name | 2 characters | 100 characters |" / "| Service Tokens | 50 | 500 | -- | 1000 \* |"

### Doppler secrets

**Recommendation:** `GET .../secret` returns `{"name": ..., "value": {"raw", "computed",
"note"}}` exactly as assumed. `POST .../secrets` (update) supports two mutually exclusive
request shapes: a flat `secrets: {NAME: value}` map (simple upsert), or a `change_requests`
array giving compare-and-swap semantics (`originalValue`/`originalVisibility` preconditions),
rename, delete, promote-to-root, converge-from-root, and typed validation (`valueType`).

- `GET /v3/configs/config/secret` requires `project`, `config`, `name`; response is exactly `{"name": ..., "value": {"raw": ..., "computed": ..., "note": ...}}`. (source: https://docs.doppler.com/reference/secrets-get)
  - `{"name": "DATABASE", "value": {"raw": "${USER}@aws.dynamodb.com:9876", "computed": "brian@aws.dynamodb.com:9876", "note": ""}}`
- `POST /v3/configs/config/secrets` accepts either `secrets` (an object of `NAME: value`) or `change_requests` (an array) — never both. (source: https://docs.doppler.com/reference/secrets-update)
  - "Either `secrets` or `change_requests` is required (can't use both)."
- Each `change_requests` entry supports optimistic-concurrency preconditions and side-effects: `originalName`, `originalValue` ("the request will only be processed if the provided value matches what's found in Doppler"), `originalVisibility`, `shouldPromote`, `shouldDelete`, `shouldConverge`. (source: https://docs.doppler.com/reference/secrets-update)
  - `"originalValue": {"description": "The value you expect the secret to have before `name` is applied. If specified, the request will only be processed if the provided value matches what's found in Doppler."}`
  - `"shouldPromote": {"description": "... Can only be set to `true` if the config being updated is a branch config. If set to `true`, the provided secret will be set in both the branch config as well as the root config in that environment."}`
- `valueType`/`originalValueType` accept one of 14 typed validators. (source: https://docs.doppler.com/reference/secrets-update)
  - `"enum": ["string", "json", "json5", "boolean", "integer", "decimal", "email", "url", "uuidv4", "cuid2", "ulid", "datetime8601", "date8601", "yaml"]`
- `visibility`/`originalVisibility` accept exactly `masked`, `unmasked`, or `restricted`. (source: https://docs.doppler.com/reference/secrets-update)
  - "Must be set to either `masked`, `unmasked`, or `restricted`."
- Secret Name length: minimum 1, maximum 200 characters. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Secret Name | 1 character | 200 characters |"
- Abuse limits (uniform across all plans): 1,200 secrets per config, 500 KiB config payload, 50 KiB per secret value. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Secrets per Config\* | 1,200 | 1,200 | 1,200 |" / "| Config Payload Size | 500 KiB | 500 KiB | 500 KiB |" / "| Secret Value Size | 50 KiB | 50 KiB | 50 KiB |"

### Doppler rate limits, errors, and platform limits

**Recommendation:** Rate limits are per-plan and dual-keyed (per IP address and per API
token); a hit returns HTTP 429 with a `retry-after` header in seconds. No fetched page
defines a JSON error-body schema for 4xx/5xx responses — see the global unresolved list.

- Two independent limits apply per request: per-IP and per-token. (source: https://docs.doppler.com/reference/api)
  - "The first rate limit is tied to an individual IP address and limits the number of requests that can be performed per minute. The second rate limit is tied to an individual API key."
- Rate limits by plan (requests/minute): Developer 240 reads / 120 secret-reads / 60 writes; Team 480 / 240 / 120; Enterprise 480 / 480 / 240. (source: https://docs.doppler.com/reference/api)
  - `| Developer | 240 | 120 | 60 | \n | Team | 480 | 240 | 120 | \n | Enterprise | 480 | 480 | 240 |`
- Every response carries `x-ratelimit-limit`, `x-ratelimit-remaining`, `x-ratelimit-reset`; a rate-limited response additionally carries `retry-after` (seconds) and status 429. (source: https://docs.doppler.com/reference/api)
  - "`retry-after` | Suggested wait period in seconds. This is only returned after a rate limit is hit." / "If you hit a rate limit, the API will respond with a 429 response code."
- General error-code semantics only: 2xx success, 4xx client error, 5xx server error — no error-body JSON schema is given on this page. (source: https://docs.doppler.com/reference/api)
  - "codes in the 2xx range indicate success, codes in the 4xx range indicate a client error (e.g. missing a required parameter), and codes in the 5xx range indicate an error with Doppler's servers."
- Full resource-quantity ceilings by plan (Developer/Team/Enterprise, \*=negotiable): Users 25/500/Custom; Config Syncs 5/100(500 add-on)/1000\*; Service Accounts --/250/5000\*; Service Account Tokens --/500/500\*; CLI Tokens per User 5/20/50; Logs retention 3 days/90 days/1095 days. (source: https://docs.doppler.com/docs/platform-limits)
  - "| Config Syncs \*\* | 5 | 100 | 500 | 1000 \* |" / "| Service Accounts | -- | 250 | -- | 5000 \* |" / "| CLI Tokens per User | 5 | 20 | -- | 50 |" / "| Logs | 3 days | 90 days | -- | 1095 days |"

### Rust client crate for Doppler (topic 8)

**Recommendation:** No official Doppler Rust SDK exists. An unofficial, self-described
"generated" client, `doppler-rs` (crates.io, version 0.0.2, MIT), exists but is explicitly
disclaimed by its own author as unofficial and is too early (0.0.2, unverified internals) to
depend on. Hand-writing a small client over a sync HTTP library, as already planned, remains
the right call — consistent with the milestone 1 doc finding no maintained Rust SDK for any
of GitHub/Buildkite/Railway/Apple either.

- `doppler-rs` (github.com/nikothomas/doppler-rs) is at version 0.0.2, MIT-licensed, and self-labels as unofficial and generated. (source: https://raw.githubusercontent.com/nikothomas/doppler-rs/main/README.md)
  - "This is a generated API client library." / "This is an unofficial client library. For official SDKs and support, visit Doppler's official documentation." / `doppler-rs = "0.0.2"`
- Its README claims full endpoint coverage (secrets, projects, environments, configs, users, groups, service accounts, activity logs, webhooks, syncs) built on reqwest + tokio + serde + chrono. (source: https://raw.githubusercontent.com/nikothomas/doppler-rs/main/README.md)
  - "Complete API coverage with full support for all Doppler API endpoints" ... "Async/await built on reqwest with full async support using tokio" ... "Native chrono integration for date/time fields"

**Unresolved**

- `doppler-rs`'s Cargo.toml/source were not inspected directly — crates.io's own page rendered only its JS-shell in WebFetch (same failure mode the milestone 1 doc hit for the crates.io API), so its "complete API coverage" claim is unverified marketing copy from its own README, not independently confirmed against its code.

### Facts that still need a browser

- The exact JSON shape of a Doppler error response (e.g. `{"success": false, "messages": [...]}`) for any 4xx/404 case — every `/reference/*` OpenAPI page fetched (projects-create/get, environments-create/get, configs-create/get, service_tokens-create/list/delete, secrets-get/update) defines only a `200` response; none defines a non-2xx schema or example. The `success`/`messages` shape referenced in the milestone 1 doc traces to a WebSearch synthesis, not a page this session could fetch and quote.
- What happens on `POST /v3/projects` with a name that collides with an existing project (status code, body) — undocumented in the fetched OpenAPI examples.
- Whether `POST /v3/configs`'s `name` must be pre-prefixed with `<environment>_` by the caller, or whether Doppler applies the prefix — see the Configs section above.
- Whether the environment `slug` and config `name` fields enforce any character class beyond the documented length bounds — every relevant OpenAPI property is a bare `{"type": "string"}`.

### Conflicts with the code's assumptions

- `DopplerServiceToken`'s pattern `dp\.st\.[A-Za-z0-9._-]{8,}` (`crates/willikins-types/src/doppler.rs:52`) is looser than Doppler's real format `dp\.st\.(?:[a-z0-9\-_]{2,35}\.)?[a-zA-Z0-9]{40,44}`: the real random suffix is exactly 40-44 characters from `[A-Za-z0-9]` only, with no dots/hyphens/underscores inside it, and the optional leading segment is lowercase-only. The code's pattern would accept strings (e.g. `dp.st.aaaaaaaa`, 8 chars, or a suffix containing a literal dot) that Doppler would never actually issue.
- `DopplerConfigName`'s `max_len = 64` (`doppler.rs:23`) exceeds Doppler's real "Config Slug" cap of 60 characters, which the docs state explicitly includes the environment-slug prefix. A locally-valid 61-64 character config name could still be rejected by the live API. This also matters for any milestone-2 branch-config naming row added to `naming.rs`: it must budget for the 60-char cap including the `<environment>_` prefix, not just the 64-char local bound.
- `SecretName`'s `max_len = 256` (`doppler.rs:47`) exceeds Doppler's documented "Secret Name" cap of 200 characters — same class of gap: local validation is more permissive than the live API for names between 201 and 256 characters.
- `DopplerProject`'s `max_len = 64` and `DopplerTokenName`'s `max_len = 64` are both *more* restrictive than Doppler's real caps (Project Name 80, Service Token Name 100) — not a bug, just headroom the code doesn't use.
- `DopplerTokenName`'s pattern `[a-z0-9]+(?:-[a-z0-9]+)*` (lowercase-kebab only) is a willikins-internal convention, not a Doppler requirement: Doppler's own example token name is `"AWS Lambda"` (mixed case, a space), and the `service_tokens-create` schema's `name` property has no pattern at all.
- `naming::v1::doppler_root_config` only produces root-config names (the environment's own snake join, e.g. `"prd"`). Doppler's config-creation endpoint (`POST /v3/configs`) is for *branch* configs exclusively and always takes a separate `environment` identifier plus `name` — milestone 2 needs a new naming row (a `v2` addition, since `v1` is frozen) for branch-config names once the auto-prefix question above is resolved.

### 3.x Config Inheritance (added 2026-09-14, for milestone 3)

Doppler's docs page (`https://docs.doppler.com/docs/config-inheritance.md`, fetched
2026-09-14): "Config Inheritance allows you to share the secrets stored in one config with
another config. Once this inheritance is setup, whenever secrets in the child config are
fetched, they will include the secrets from the inherited configs" and "This feature is
available with our Team and Enterprise plans." Two endpoints, request schemas quoted from
`https://docs.doppler.com/reference/configs-inheritable.md` and
`https://docs.doppler.com/reference/configs-inherits.md` (same fetch):

`POST /v3/configs/config/inheritable`, required `project`, `config`, `inheritable`:

```json
"inheritable": {
  "type": "boolean",
  "description": "Boolean determining if the config is inheritable or not.",
  "default": false
}
```

`POST /v3/configs/config/inherits`, required `project`, `config`, `inherits`:

```json
"inherits": {
  "type": "array",
  "description": "Array of objects indicating which configs are being inherited.",
  "items": {
    "properties": {
      "project": { "type": "string", "description": "Unique identifier for the project object of the config being inherited." },
      "config": { "type": "string", "description": "Name of the config object being inherited." }
    },
    "required": ["project", "config"],
    "type": "object"
  }
}
```

Both answer `200` with the config object, whose `inheritable`, `inheriting`, `inherits` and
`inheritedBy` fields the live write cycle observed on every config `GET` the same day (the
example in the reference shows `"name": "prd_gcp", "root": false, "inheritable": true`).
The `project` values in these bodies are project names, per the correction above.

## 4. Supporting crates, Railway, and the container image



### Synchronous HTTP client: `ureq` vs `reqwest::blocking`

**Recommendation:** Use `ureq` 3.x (current: 3.4.1) with its default `rustls` backend and the `json` feature enabled. ureq is a genuinely blocking client with no internal tokio runtime, so it is safe to call from inside a `tokio::task::spawn_blocking` closure. `reqwest::blocking` wraps its own background tokio runtime thread and its maintainers added an explicit panic guard that fires when any tokio `Handle` is already current on the calling thread — and tokio's own docs say a `Handle` is available on any thread "run by the runtime," which includes `spawn_blocking` pool threads. That makes `reqwest::blocking` a real hazard in exactly this project's calling pattern, whereas ureq has no such failure mode and pulls in a smaller, non-tokio dependency graph.

- ureq's current published version is 3.4.1. (source: `cargo info ureq`, crates.io registry data fetched locally)
  - "Downloaded ureq v3.4.1 ... version: 3.4.1"
- ureq's default features are rustls + gzip; `json`, `native-tls` are opt-in. (source: `cargo info ureq`, crates.io registry data fetched locally)
  - "+default = [rustls, gzip]" / "json = [dep:serde, dep:serde_json, cookie_store?/serde_json]" / "native-tls = [native-tls-no-default, dep:der, _tls, native-tls-webpki-roots]"
- ureq is deliberately blocking-only, to keep the API simple and dependencies minimal. (source: https://raw.githubusercontent.com/algesten/ureq/main/README.md)
  - "It uses blocking I/O instead of async I/O, because that keeps the API simple and keeps dependencies to a minimum."
- A 4xx/5xx response status is, by default, converted into an `Err` rather than returned as a normal response. (source: https://docs.rs/ureq/latest/ureq/enum.Error.html)
  - "When `http_status_as_error()` is true, 4xx and 5xx response status codes are translated to this error. This is the default behavior."
- The opt-out is `ConfigBuilder::http_status_as_error(bool)`, defaulting to `true`. (source: https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html)
  - "pub fn http_status_as_error(self, v: bool) -> Self ... Whether to treat 4xx and 5xx HTTP status codes as `Err(Error::StatusCode))`. Defaults to `true`."
- `reqwest::blocking` must not run inside an async runtime, or it panics. (source: https://raw.githubusercontent.com/seanmonstar/reqwest/master/src/blocking/mod.rs)
  - "the functionality in `reqwest::blocking` must *not* be executed within an async runtime, or it will panic when attempting to block."
- `reqwest::blocking::Client` spawns a dedicated OS thread that builds its own current-thread tokio runtime to drive requests. (source: https://raw.githubusercontent.com/seanmonstar/reqwest/master/src/blocking/client.rs)
  - "let handle = thread::Builder::new().name(\"reqwest-internal-sync-runtime\".into()).spawn(move || { use tokio::runtime; let rt = match runtime::Builder::new_current_thread().enable_all().build()"
- The `blocking` feature itself pulls in `tokio/sync` — reqwest's blocking client is not tokio-free even for purely synchronous calls. (source: `cargo info reqwest`, crates.io registry data fetched locally)
  - "blocking = [dep:futures-channel, futures-channel?/sink, dep:futures-util, futures-util?/io, futures-util?/sink, tokio/sync]"
- Real users hit this exact failure calling `reqwest::blocking::Client` from inside a tokio runtime. (source: https://github.com/seanmonstar/reqwest/issues/1017)
  - "Cannot drop a runtime in a context where blocking is not allowed. This happens when a runtime is dropped from within an asynchronous context."
- reqwest maintainers merged an early-panic guard checking for any current tokio `Handle` before building a blocking client. (source: https://patch-diff.githubusercontent.com/raw/seanmonstar/reqwest/pull/1263.diff)
  - "+ if Handle::try_current().is_ok() {" / "+ panic!(\"You should not run reqwest::blocking inside existing Tokio Runtime\");"
- tokio's `Handle::current()`/`try_current()` succeed on any thread "run by the runtime," not only worker threads spawned directly by it. (source: https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html)
  - "you must call this on one of the threads being run by the runtime, or from a thread with an active `EnterGuard`"

**Unresolved**

- No single primary source states verbatim "`reqwest::blocking` panics specifically inside `spawn_blocking`" — the conclusion chains reqwest's `Handle::try_current()` guard (PR #1263) with tokio's documented rule that `Handle::current()` succeeds on any runtime-managed thread. Worth a quick smoke test in willikins' own harness before relying on it.

### HTTP mocking for synchronous ureq-based provider tests: mockito vs httpmock vs wiremock

**Recommendation:** Use `mockito` (1.x, current 1.7.2) for the sync `ureq` provider tests. `Server::new()` is a purpose-built blocking constructor — no runtime, no `.await` — and its fluent builder (`server.mock(...).match_header(...).with_status(...).with_body(...).create()`) plus `Matcher::Json`/`Matcher::PartialJson` cover header and JSON-body assertions directly. `httpmock` (0.8.3) is a workable second choice — `MockServer::start()` is synchronous even though the server core is async internally — but it pulls in a heavier dependency tree (hyper, tokio, rustls/rcgen behind features) for a project where build time is a cost. `wiremock` (0.6.5) should be ruled out: it is async-only, so it cannot be driven from a purely synchronous test without pulling in and blocking on a runtime.

- `mockito` is at 1.7.2. (source: `cargo info mockito`, crates.io registry data fetched locally)
  - "version: 1.7.2"
- `mockito`'s mock-building API is a fluent chain ending in `.create()`. (source: https://raw.githubusercontent.com/lipanski/mockito/master/README.md)
  - "server.mock(\"GET\", \"/hello\")\n  .with_status(201)\n  .with_header(\"content-type\", \"text/plain\")\n  .with_body(\"world\")\n  .create();"
- `mockito`'s `Mock::assert()` asserts the expected request count (default 1). (source: https://docs.rs/mockito/latest/mockito/struct.Mock.html)
  - "Asserts that the expected amount of requests (defaults to 1 request) were performed."
- `mockito` supports JSON body matching via the `Matcher` enum (exact and partial variants). (source: https://docs.rs/mockito/latest/mockito/enum.Matcher.html)
  - "Matches a specified JSON body from a `serde_json::Value`" (Json); "Matches a partial JSON body from a `serde_json::Value`" (PartialJson)
- `httpmock` is at 0.8.3. (source: `cargo info httpmock`, crates.io registry data fetched locally)
  - "version: 0.8.3"
- `httpmock`'s `MockServer::start()` is called without `.await` in its own getting-started example. (source: https://raw.githubusercontent.com/httpmock/httpmock/master/README.md)
  - "let server = MockServer::start();"
- `httpmock` uses a `when(...)/then(...)` closure API. (source: https://raw.githubusercontent.com/httpmock/httpmock/master/README.md)
  - "let mock = server.mock(|when, then| {\n    when.method(GET)\n        .path(\"/translate\")\n        .query_param(\"word\", \"hello\");\n    then.status(200)\n        .header(\"content-type\", \"text/html; charset=UTF-8\")\n        .body(\"hola\");\n});"
- `wiremock` is at 0.6.5 and explicitly supports tokio and async-std as futures runtimes — i.e. it requires one. (source: https://docs.rs/wiremock/latest/wiremock/)
  - "wiremock can be used (and it is tested to work) with both async_std and tokio as futures runtimes."

**Unresolved**

- Could not get crates.io's own version pages to render via WebFetch (likely a JS-rendered SPA); versions were instead sourced from real `cargo info` output against the live index.
- mockito's current docs show `expect()`/`expect_at_least()`/`expect_at_most()` for call-count expectations rather than a separate `assert_hits(n)` method; if such a method existed in an older release and was renamed, that history was not verified.

### Rejecting YAML anchors and aliases before deserialization

**Recommendation:** Pre-scan untrusted YAML with `saphyr-parser`'s low-level event API and reject any document that emits an `Event::Alias` or an anchored `Scalar`/`SequenceStart`/`MappingStart` (anchor id present), before handing the same bytes to the existing `serde_yaml_ng` deserializer. This is a small, additive gate in front of an already-vetted deserializer: `saphyr-parser` (0.0.12) is the lightest option exposing exactly the needed primitives, and adding it costs only 7 new transitive crates (8 packages locked total, including itself). Do not swap `serde_yaml_ng` for `serde-saphyr` yet: its `Budget` mechanism is a genuinely more complete built-in defense (alias/anchor ratio and count limits, not just scan-and-reject), but it pulls in ~20 additional crates and only reached a stable 1.0 API two months ago — a deserializer swap is a far larger-blast-radius change than a narrow pre-scan gate.

- `saphyr-parser` is at 0.0.12 (released 18 August 2026). (source: https://docs.rs/saphyr-parser/latest/saphyr_parser/)
  - "0.0.12 (released 18 August 2026)"
- Its `Parser` type is constructed from a string via `new_from_str`. (source: https://docs.rs/saphyr-parser/latest/saphyr_parser/struct.Parser.html)
  - "pub fn new_from_str(value: &'input str) -> Self" — "Create a new instance of a parser from a &str."
- Its `Event` enum carries alias ids and anchor ids directly on the relevant variants. (source: https://docs.rs/saphyr-parser/latest/saphyr_parser/enum.Event.html)
  - "Alias(usize), Scalar(Cow<'input, str>, ScalarStyle, usize, Option<Cow<'input, Tag>>), SequenceStart(usize, Option<Cow<'input, Tag>>), ... MappingStart(usize, Option<Cow<'input, Tag>>)"
- Its `Marker` type exposes line/column accessors for error reporting. (source: https://docs.rs/saphyr-parser/latest/saphyr_parser/struct.Marker.html)
  - "pub fn line(&self) -> usize" — "Return the line of the marker in the source." / "pub fn col(&self) -> usize" — "Return the column of the marker in the source."
- Adding `saphyr-parser` alone to a fresh crate locks a small dependency set. (source: `cargo add saphyr-parser`, run locally in scratch crate)
  - "Locking 8 packages to latest Rust 1.97.0 compatible versions" (saphyr-parser + arraydeque + thiserror + thiserror-impl + proc-macro2 + quote + syn + unicode-ident)
- `yaml-rust2` (0.13.0) also exposes a low-level `Event` enum with an `Alias` variant, with a comparable dependency footprint. (source: https://docs.rs/yaml-rust2/latest/yaml_rust2/parser/enum.Event.html)
  - "Alias(usize)" — "The anchor ID the alias refers to."
- `serde-saphyr` (1.2.0) ships a `Budget` type that inspects the parser's event stream to enforce limits against pathological inputs. (source: https://docs.rs/serde-saphyr/latest/serde_saphyr/budget/index.html)
  - "Streaming YAML budget checker using granit-parser. This inspects the parser's event stream and enforces simple budgets to avoid pathological inputs"
- That `Budget` struct has explicit alias/anchor-ratio and count fields — an amplification-ratio check, not just a raw cap. (source: https://docs.rs/serde-saphyr/latest/serde_saphyr/budget/struct.Budget.html)
  - fields observed: `max_aliases`, `max_anchors`, `enforce_alias_anchor_ratio`, `alias_anchor_min_aliases`, `alias_anchor_ratio_multiplier`, `max_depth`, `max_recorded_anchor_bytes`
- `serde-saphyr` was created recently and only reached stable 1.0 two months ago. (source: https://crates.io/api/v1/crates/serde-saphyr)
  - "Created: September 27, 2025" ... "1.0.0 | July 31, 2026" ... "Max Stable Version: 1.2.0"
- Swapping to `serde-saphyr` pulls in substantially more crates than either event-parser-only option. (source: `cargo add serde-saphyr`, run locally in scratch crate)
  - added `granit-parser, memchr, multiversion, multiversion-macros, multiversion_no_op, nohash-hasher, num-traits, proc-macro2, quote, rustversion, scopeguard, serde-saphyr, serde_core, serde_derive, simdutf8, smallvec, syn, unicode-ident, unicode-width, zmij` (20 crates)

**Unresolved**

- Could not confirm from docs.rs whether `Event::Alias`'s `usize` uses a `0`-sentinel for "no anchor" on the paired Scalar/SequenceStart/MappingStart variants, or whether anchors are always non-zero — check against `saphyr-parser` source before writing the reject condition.
- Could not get a verbatim doc-comment quote (only a paraphrased field table) for `Budget`'s default numeric values.

### Append-only journal storage: rusqlite (bundled + WAL) vs. hand-rolled JSONL

**Recommendation:** For tens of runs/day from a single process on a Railway persistent volume, a hand-rolled JSONL append log — `OpenOptions::new().append(true).open(...)`, one `serde_json` line per run, `sync_data()` (or `sync_all()` when metadata durability matters too) after each write, an `fd-lock`/`fs2` exclusive lock guarding against an accidental second instance, and a linear read-and-replay on startup — is simpler and sufficient. `rusqlite`'s `bundled` feature is easy to justify technically (no system-SQLite dependency, crash-safe WAL for free), but at this volume it buys crash-consistency and queryability the workload doesn't need yet, at the fixed cost of compiling SQLite from source on every clean build on an already slow host. Keep SQLite+WAL in reserve for when multi-process access or ad-hoc SQL filtering over runs becomes an actual requirement.

- `rusqlite` is at 0.40.2; `libsqlite3-sys` is at 0.38.2. (source: https://crates.io/crates/rusqlite ; https://crates.io/crates/libsqlite3-sys)
  - "version: 0.40.2" / "version: 0.38.2"
- The `bundled` feature makes `libsqlite3-sys` compile SQLite from vendored source via the `cc` crate rather than link a system library, currently SQLite 3.53.2. (source: https://github.com/rusqlite/rusqlite/blob/master/README.md)
  - "If you use the `bundled`, ... features, `libsqlite3-sys` will use the [cc](https://crates.io/crates/cc) crate to compile SQLite or SQLCipher from source and link against that. This source is embedded in the `libsqlite3-sys` crate and is currently SQLite 3.53.2"
- rusqlite's docs frame `bundled` as avoiding host-SQLite-version issues at the cost of extra compile time. (source: https://github.com/rusqlite/rusqlite/blob/master/README.md)
  - "`bundled` causes us to automatically compile and link in an up to date version of SQLite for you. This avoids many common build issues, and avoids depending on the version of SQLite on the users system"
- WAL removes reader/writer blocking and is generally faster, with fewer required `fsync()` calls than rollback-journal mode, but checkpointing still requires sync operations for crash safety. (source: https://www.sqlite.org/wal.html)
  - "WAL provides more concurrency as readers do not block writers and a writer does not block readers." / "WAL is significantly faster in most scenarios." / "WAL uses many fewer fsync() operations" / "Checkpointing does require sync operations in order to avoid the possibility of database corruption following a power loss"
- `File::sync_all` flushes content and metadata; `File::sync_data` may skip metadata to reduce I/O. (source: https://doc.rust-lang.org/std/fs/struct.File.html)
  - "Attempts to sync all OS-internal file content and metadata to disk." / "This function is similar to sync_all, except that it might not synchronize file metadata ... The goal of this method is to reduce disk operations."
- `fs2::FileExt::lock_exclusive` provides a blocking OS-level exclusive advisory lock; `fd-lock` (4.0.4) provides an advisory `RwLock` wrapper for files. (source: https://docs.rs/fs2/latest/fs2/trait.FileExt.html ; https://docs.rs/fd-lock/latest/fd_lock/)
  - "Locks the file for exclusive usage, blocking if the file is currently locked." / "Advisory reader-writer locks for files."
- `fs4` (1.1.0) is a fork of `fs2` adding async support and replacing `libc` with `rustix`; `fs2`'s upstream repo is not formally archived but shows no recent pushes. (source: https://github.com/al8n/fs4/blob/main/README.md ; https://api.github.com/repos/danburkert/fs2-rs)
  - "This is a fork of the fs2-rs crate, the aim for this fork is to support `async` and replace `libc` by rustix." / `"archived": false, "pushed_at": "2024-02-16T20:16:28Z"`
- No canonical "journal" crate combining JSONL + fsync + locking + replay was found; the closest match (`jsonl` 4.0.1) is only a line-format parser/writer. (source: https://crates.io/crates/jsonl)
  - "An implementation of JSON Lines for Rust"

**Unresolved**

- No official statement that `fs2` is deprecated in favor of `fs4` was found (only inferred from GitHub staleness plus `fs4`'s self-description).
- `fs4` 1.1.0's exact lock-trait method signature was not confirmed against rendered rustdoc (docs.rs served only a crate-metadata page for the fetch attempted).

### Encoding, hashing, and constant-time comparison utilities

**Recommendation:** Use `base64` 0.23.1 via the `Engine` trait (`base64::engine::general_purpose::STANDARD` or `base64::prelude::BASE64_STANDARD`) — free functions have been deprecated since 0.21. Use `sha2` 0.11.0's `Sha256::new()/.update()/.finalize()` for hashing agent tokens at rest, and compare digests with `subtle` 2.6.1's `ConstantTimeEq::ct_eq`, never `==`.

- `base64` is at 0.23.1 with default features `std, simd-unsafe`; the free-function API has been deprecated since 0.21.0 in favor of `Engine`. (source: https://crates.io/crates/base64/0.23.1 ; https://docs.rs/base64/latest/base64/fn.encode.html)
  - "+default = [std, simd-unsafe]" / "👎Deprecated since 0.21.0: Use Engine::encode"
- The crate root shows the modern call shape. (source: https://docs.rs/base64/latest/base64/)
  - "use base64::prelude::*;\n\nassert_eq!(BASE64_STANDARD.decode(b\"+uwgVQA=\")?, b\"\\xFA\\xEC\\x20\\x55\\0\");\nassert_eq!(BASE64_STANDARD.encode(b\"\\xFF\\xEC\\x20\\x55\\0\"), \"/+wgVQA=\");"
- `sha2` is at 0.11.0; usage is the incremental new/update/finalize shape. (source: https://crates.io/crates/sha2/0.11.0 ; https://docs.rs/sha2/latest/sha2/)
  - "version: 0.11.0" / "let mut hasher = Sha256::new();\nhasher.update(b\"hello \");\nhasher.update(b\"world\");\nlet hash256 = hasher.finalize();"
- `subtle` is at 2.6.1, purpose-built for constant-time comparison via `ConstantTimeEq::ct_eq`, documented to run in constant time. (source: https://crates.io/crates/subtle/2.6.1 ; https://docs.rs/subtle/latest/subtle/trait.ConstantTimeEq.html)
  - "Pure-Rust traits and utilities for constant-time cryptographic implementations." / "fn ct_eq(&self, other: &Self) -> Choice" / "Determine if two items are equal. The `ct_eq` function should execute in constant time."

**Unresolved**

- Whether any other willikins dependency still pins `sha2` 0.10.x (which would duplicate the crate) was not checked — run `cargo tree -i sha2` in the real workspace.

### Hashing high-entropy agent tokens: is a fast hash enough?

**Recommendation:** Plain `sha2::Sha256` over the raw token bytes is sufficient at rest for a CSPRNG-generated agent token, provided the token carries at least 112 bits of entropy (e.g. >=16 random bytes before encoding) — NIST SP 800-63B draws exactly this line between low-entropy human-memorized secrets (which need a slow KDF) and high-entropy "look-up secrets" (which only need an approved one-way function). A slow KDF like `argon2` buys nothing extra here and only adds latency to every request.

- Memorized (human-chosen, low-entropy) secrets must be hashed with a suitable one-way key derivation function. (source: https://pages.nist.gov/800-63-3/sp800-63b.html)
  - "Verifiers SHALL store memorized secrets in a form that is resistant to offline attacks. Memorized secrets SHALL be salted and hashed using a suitable one-way key derivation function."
- Look-up secrets with >=112 bits of entropy only need an approved one-way function (a plain hash), no KDF required; below that threshold the KDF requirement re-applies. (source: https://pages.nist.gov/800-63-3/sp800-63b.html)
  - "Look-up secrets having at least 112 bits of entropy SHALL be hashed with an approved one-way function" / "Look-up secrets with fewer than 112 bits of entropy SHALL be salted and hashed using a suitable one-way key derivation function."

**Unresolved**

- Whether a later NIST SP 800-63B revision keeps the 112-bit threshold was not checked.
- This only holds if willikins' token generator actually produces >=112 bits of CSPRNG entropy per token — an implementation fact, not something this research checked.

### Identifiers: UUID v7

**Recommendation:** Use `uuid` 1.26.1 with the `v7` (and `std`) features and `Uuid::now_v7()` for time-ordered agent-run identifiers.

- `uuid` is at 1.26.1. (source: https://crates.io/crates/uuid/1.26.1)
  - "version: 1.26.1"
- `now_v7()` is gated by the `v7` (and `std`) feature and guarantees creation ordering. (source: https://docs.rs/uuid/latest/uuid/struct.Uuid.html)
  - "pub fn now_v7() -> Self" / "Available on **crate features `std` and `v7`** only." / "All UUIDs generated through this method by the same process are guaranteed to be ordered by their creation."

### Timestamps: jiff vs chrono vs time, and the schemars `chrono04` tail

**Recommendation:** Standardize on `chrono`, not `jiff` or `time`. `schemars`'s `chrono04` feature (enabled by rmcp 3.3, per the milestone 1 research doc) is an optional dependency renamed at the manifest level — the actual package pulled into the tree is `chrono` itself with its own default features off. Adding `chrono` directly with `default-features = false, features = ["now", "serde"]` unifies with that same registry package and adds zero new crates. `jiff` would add at least one more crate with no corresponding benefit here, and `time` has no existing foothold in the tree.

- `schemars`'s `chrono04` feature maps to an optional dependency renamed `chrono04` that is package `chrono`, with its own default features off. (source: https://crates.io/crates/schemars/1.2.2 ; https://docs.rs/crate/schemars/1.2.2/source/Cargo.toml)
  - "chrono04 = [dep:chrono04]" / "[dependencies.chrono04]\nversion = \"0.4.39\"\noptional = true\ndefault-features = false\npackage = \"chrono\""
- `chrono` is at 0.4.45; `now` (needed for `Utc::now()`) enables `std`, and `serde` is a separate opt-in feature. (source: https://crates.io/crates/chrono/0.4.45 ; https://docs.rs/chrono/latest/chrono/struct.Utc.html)
  - "now = [std]" / "std = [alloc]" / "serde = [dep:serde]" / "Available on **crate feature `now` and not (WebAssembly ...)** only."
- `chrono::DateTime` has direct RFC 3339 format/parse methods. (source: https://docs.rs/chrono/latest/chrono/struct.DateTime.html)
  - "Returns an RFC 3339 and ISO 8601 date and time string such as `1996-12-19T16:39:57-08:00`." / "Parses an RFC 3339 date-and-time string into a `DateTime<FixedOffset>` value."
- `jiff` (0.2.35) supports RFC 3339-shaped formatting but is built on a Temporal-derived hybrid format, and depends on a separate `jcore` crate. (source: https://docs.rs/jiff/latest/jiff/ ; https://crates.io/crates/jiff/0.2.35)
  - "Formatting and parsing datetimes via a Temporal-specified hybrid format that takes the best parts of RFC 3339, RFC 9557 and ISO 8601." / "alloc = [jcore/alloc, serde_core?/alloc, ...]"
- `time` (0.3.55) has a dedicated RFC 3339 format-description type but has no existing foothold in this project's tree. (source: https://docs.rs/time/latest/time/format_description/well_known/struct.Rfc3339.html)
  - "The format described in RFC 3339"

**Unresolved**

- Whether rmcp 3.3's own Cargo.toml enables `chrono04` was taken from the milestone 1 research doc, not independently re-verified here — run `cargo tree -e features -i chrono` in the real workspace to confirm the final unified feature set.

### Async runtime, HTTP framework, and middleware

**Recommendation:** `tokio` 1.53.1 and `axum` 0.8.9 are both current and compatible with `tower-http` 0.7.x and `tower_governor` 0.8.x. Use `axum::extract::DefaultBodyLimit::max(N)` for extractor-backed routes (axum's own default is only 2MB) and `tower_http::limit::RequestBodyLimitLayer` for any raw-body path bypassing axum's extractors, plus `tower_http::timeout::TimeoutLayer` for request timeouts — `tower-http` ships with an empty default feature set, so `limit`/`timeout` must be requested explicitly. Use `tower_governor` 0.8.0's `GovernorLayer`/`GovernorConfigBuilder` for rate limiting, with `default-features = false, features = ["axum"]` to skip the bundled `tonic` (gRPC) integration.

- `tokio` is at 1.53.1; `axum` is at 0.8.9. (source: https://crates.io/crates/tokio/1.53.1 ; https://crates.io/crates/axum/0.8.9)
  - "version: 1.53.1" / "version: 0.8.9"
- `tower-http` is at 0.7.1 with no default features; `limit` and `timeout` are separate opt-in flags. (source: https://crates.io/crates/tower-http/0.7.1 ; https://docs.rs/crate/tower-http/0.7.1/source/Cargo.toml)
  - "+default = []" / "limit = [\"dep:http-body\", \"dep:http-body-util\"]" / "timeout = [\"dep:http-body\", \"dep:tokio\", \"tokio?/time\"]"
- `RequestBodyLimitLayer` converts oversized bodies into 413 responses; axum's own `DefaultBodyLimit` caps bodies at 2MB by default. (source: https://docs.rs/tower-http/latest/tower_http/limit/index.html ; https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html)
  - "intercepts requests with body lengths greater than the configured limit and converts them into 413 Payload Too Large responses." / "For security reasons, `Bytes` will, by default, not accept bodies larger than 2MB."
- `tower_governor` is at 0.8.0 with default features `axum, tonic`; it's a Tower/axum rate-limiting layer configured via burst size and refill period. (source: https://crates.io/crates/tower_governor/0.8.0 ; https://docs.rs/tower_governor/latest/tower_governor/)
  - "+default = [axum, tonic]" / "allows bursts with up to five requests per IP address and replenishes one element every two seconds."
- Its default key extractor is peer IP, which the crate's own docs flag as possibly wrong for a given deployment; header-based and global extractors are also provided. (source: https://docs.rs/tower_governor/latest/tower_governor/key_extractor/index.html)
  - "A KeyExtractor that uses peer IP as key. This is the default key extractor and it may no do want you want." [sic]

**Unresolved**

- Whether `PeerIpKeyExtractor` needs axum served via `into_make_service_with_connect_info` to see the real peer address was not sourced here — get this right before relying on per-IP limiting, since a misconfigured extractor can silently collapse all callers into one bucket.
- No cross-check that `tower` doesn't resolve to duplicate major versions across `axum`/`tower-http`/`tower_governor` (the last pins `tower = "0.5.1"`) — run `cargo tree` in the real workspace.

### Railway deployment as code

**Recommendation:** Do not build on `railway.json`/`railway.toml` for a new service — Railway's own docs state Config as Code is deprecated, existing config files stop being read on 2026-12-01, and new services cannot opt into it. Use Infrastructure as Code (`.railway/railway.ts`) instead: declare the service with `source: github(...)` (Railway builds the repo's Dockerfile) or `source: image(...)`, set `healthcheck`, and declare `volume(...)` with `sizeMB` bound via `volumeMounts`. Layer Doppler on top via its native Railway integration for continuously synced variables, enable PR Environments from the dashboard, and rely on Railway's edge proxy for TLS termination — the app only needs to bind `0.0.0.0:$PORT` over plain HTTP.

- Config as Code is deprecated with a hard cutoff, and new services cannot adopt it; a service cannot be managed by both systems at once. (source: https://docs.railway.com/config-as-code ; https://docs.railway.com/infrastructure-as-code)
  - "Config as Code is deprecated. Prefer Infrastructure as Code (`.railway/railway.ts`) for project configuration. Existing `railway.json` / `railway.toml` files continue to work for services that already use them until **2026-12-01** (hard cutoff). New services cannot opt into Config as Code." / "A service cannot be managed by both systems at the same time."
- IaC declares build source via `source: github(...)` or `source: image(...)`, a `healthcheck` field, and per-service/per-region `replicas`. (source: https://docs.railway.com/infrastructure-as-code/reference ; https://docs.railway.com/infrastructure-as-code)
  - "Omit source when .railway/railway.ts should manage service settings but not declare a GitHub repository or Docker image." / `healthcheck: "/health"` / `const web = service("web", { replicas: 3, });`
- IaC declares a persistent volume with a region and size, bound to a service path via `volumeMounts`. (source: https://docs.railway.com/infrastructure-as-code/reference)
  - `const data = volume("backend-data", { region: "us-west2", sizeMB: 1024, }); ... volumeMounts: { "/data": data }`
- The legacy schema's equivalents: `build.builder` (RAILPACK default or DOCKERFILE) + `dockerfilePath`; `deploy.healthcheckPath`; `deploy.restartPolicyType` (ON_FAILURE/ALWAYS/NEVER); per-region `numReplicas`. (source: https://docs.railway.com/reference/config-as-code)
  - "Location of non-standard Dockerfile." / "Path to check after starting your deployment to ensure it is healthy." / "How to handle the deployment crashing." / `"us-west2": {"numReplicas": 2}`
- Volumes mount at deploy/runtime (not build time); a service is limited to one volume and cannot use multiple replicas. (source: https://docs.railway.com/guides/volumes ; https://docs.railway.com/reference/volumes)
  - "Volumes are mounted to your service's container when it is started, not during build time." / "Replicas cannot be used with volumes" / "Each service can only have a single volume"
- Volume capacity is plan-tiered, e.g. 5GB on Hobby, growing self-serve up to 1TB on Pro/Enterprise. (source: https://docs.railway.com/reference/volumes)
  - "Hobby plans: 5GB"
- Railway injects `PORT` and the app must bind `0.0.0.0` to receive traffic. (source: https://docs.railway.com/networking/troubleshooting/application-failed-to-respond)
  - "Your web server should bind to the host 0.0.0.0 and listen on the port specified by the PORT environment variable, which Railway automatically injects into your application."
- Doppler ships a native Railway integration, continuously syncing the selected config's secrets into the Railway project. (source: https://blog.railway.com/p/doppler-integration-secrets-sharing ; https://docs.doppler.com/docs/railway)
  - "Doppler has just released a new native integration with Railway!" / "The secrets from your selected config will be immediately and continuously synced with your Railway project."
- PR Environments auto-provision on PR open and de-provision on merge/close, replicating "services, networking, and variables" per PR. (source: https://docs.railway.com/guides/preview-deployments-with-pr-environments)
  - "Railway automatically provisions all relevant infrastructure whenever a new PR is opened" / "After the PR is merged or closed, Railway de-provisions all services in the PR Environment." / "The standard PR Environment replicates your entire base environment, including services, networking, and variables, into an isolated ephemeral environment for each PR."
- Railway's edge proxy terminates TLS before forwarding traffic over the internal network to the deployment. (source: https://docs.railway.com/networking/edge-networking)
  - "The edge proxy (tcp-proxy) terminates TLS, adds headers, and looks up routing information" ... "Traffic is forwarded over Railway's internal network to your deployment."

**Unresolved**

- No page states verbatim that the internal hop to the app is plain HTTP; it's inferred from edge TLS termination plus PORT-binding guidance (no app-side cert config anywhere), not directly quoted.
- No `prDeploys` IaC field name was found — PR Environments appear to be a dashboard toggle only in the docs surfaced.
- Whether a PR Environment gets its own separate (empty) volume, or omits volumes from the base environment, is not documented — a genuine gap.
- Doppler's sync mechanism (poll vs. push on secret change) is not stated explicitly beyond "continuous."

### Minimal multi-stage Dockerfile for a Rust binary

**Recommendation:** Pin the builder to `rust:1.97-slim-bookworm` (falling back to `rust:1.97-bookworm` only if apt build deps are needed), following the official image's `<version>-<codename>` tagging pattern. Use `cargo-chef` (the `lukemathwalker/cargo-chef:latest-rust-1` image, or a `cargo install cargo-chef` step) with the standard `prepare` -> `cook --recipe-path recipe.json` split so dependency compilation is cached in its own Docker layer. For runtime, prefer `gcr.io/distroless/cc-debian12` over `debian:bookworm-slim`: since the binary's only TLS backend is rustls, it needs neither `libssl` nor a shell/package manager — just glibc/libgcc plus a CA bundle. If the binary embeds `webpki-roots` rather than relying on `rustls-native-certs` + system `ca-certificates`, even `distroless/static-debian12` becomes viable.

- The official Rust image publishes version+codename tags like `1-bookworm`, `1.98-bookworm`, `1.98.1-bookworm`, plus parallel `slim` variants (`1-slim-bookworm`, etc.). (source: https://github.com/docker-library/docs/blob/master/rust/README.md)
  - "[`1-bookworm`, `1.98-bookworm`, `1.98.1-bookworm`, `bookworm`]" / "[`1-slim-bookworm`, `1.98-slim-bookworm`, `1.98.1-slim-bookworm`, `slim-bookworm`]"
- `cargo-chef`'s prebuilt image reuses that same Rust tag: `<cargo-chef version>-rust-<rust tag>`. (source: https://github.com/LukeMathWalker/cargo-chef/blob/main/README.md)
  - "The tagging scheme is `<cargo-chef version>-rust-<rust tag>`. For example, `0.1.74-rust-1.56.0`."
- `debian:bookworm-slim` only strips docs/man pages and is explicitly called experimental, not a hardening measure; it still ships `apt`. (source: https://github.com/docker-library/docs/blob/master/debian/README.md)
  - "an experiment in providing a slimmer base (removing some extra files that are normally not necessary within containers, such as man pages and documentation), and are definitely subject to change." / "RUN apt-get update && apt-get install -y locales && rm -rf /var/lib/apt/lists/*"
- Distroless images have no shell or package manager; `gcr.io/distroless/cc` adds libgcc for languages like Rust, while `gcr.io/distroless/base` adds glibc + libssl on top of `static` (which already bundles `ca-certificates`). (source: https://github.com/GoogleContainerTools/distroless/blob/main/README.md ; https://github.com/GoogleContainerTools/distroless/blob/main/cc/README.md ; https://github.com/GoogleContainerTools/distroless/blob/main/base/README.md)
  - "They do not contain package managers, shells or any other programs you would expect to find in a standard Linux distribution." / "This image contains a minimal Linux, glibc runtime for \"mostly-statically compiled\" languages like Rust and D... plus: * libgcc1 and its dependencies." / "which contains: * ca-certificates ... `gcr.io/distroless/base`, which contains all of the packages in `gcr.io/distroless/static`, and * glibc * libssl"
- `cargo-chef` exists to cache dependency compilation as a separate Docker layer, reporting up to 5x build-time wins. (source: https://github.com/LukeMathWalker/cargo-chef/blob/main/README.md)
  - "Cache the dependencies of your Rust project and speed up your Docker builds." / "massively speeding up your builds (up to 5x measured on some commercial projects)"
- `rustls-native-certs` reads OS-native trust roots at runtime (needs a CA package); `webpki-roots` bakes Mozilla's roots into the binary (needs none). (source: https://docs.rs/rustls-native-certs/latest/rustls_native_certs/ ; https://docs.rs/webpki-roots/latest/webpki_roots/)
  - "rustls-native-certs allows rustls to use the platform's native certificate store when operating as a TLS client." / "A compiled-in copy of the root certificates trusted by Mozilla."
- rustls does not use OpenSSL by default — OpenSSL is only available via a separate, opt-in provider crate. (source: https://github.com/rustls/rustls/blob/main/README.md)
  - "[`rustls-openssl`] - a provider that uses [OpenSSL] for cryptography."

**Unresolved**

- No Railway-specific source was checked for this topic, so whether Railway's build pipeline has any constraint on distroless final images (vs. its Nixpacks default) is unverified.
- No single official page states in one sentence "a rustls binary needs no libssl at runtime" — assembled from rustls's default crypto-provider docs plus the distroless package lists, not one direct quote.

### Facts that still need a browser

- serde-saphyr `Budget` default numeric values (only a paraphrased field table was retrievable, not a verbatim doc-comment).
- Whether `saphyr-parser`'s alias/anchor `usize` ids use 0 as a "no anchor" sentinel (needs the crate's source, not just docs.rs rendered API).
- `fs4` 1.1.0's exact lock-trait method signature (docs.rs served only a metadata page for this fetch).
- Whether Railway's PR Environments provision a separate persistent volume per PR, or omit volumes entirely — not documented on the pages reached.
- Whether Doppler's Railway sync is poll-based or push-on-change.
- Whether reqwest's blocking-inside-spawn_blocking panic is documented verbatim anywhere as a single sentence (currently inferred from two separate sources).
- Railway's own constraints, if any, on using a distroless (shell-less) runtime image in its build/deploy pipeline.

## 5. Reserved-word verification


Check date: 2026-09-12. Repo read-only; nothing in it was modified.

Sources actually fetched (with method, since Swift's live page is a JS SPA that returns
only an empty shell to both WebFetch and `curl`):

- Rust: `curl` of <https://doc.rust-lang.org/reference/keywords.html> (raw HTML, parsed directly).
- Java: `curl` of <https://docs.oracle.com/javase/specs/jls/se21/html/jls-3.html#jls-3.9> (raw HTML, ¬ 3.9 table extracted).
- Kotlin: `curl` of <https://kotlinlang.org/docs/keyword-reference.html> (raw HTML, parsed directly).
- Swift: <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/lexicalstructure/>
  returned only a Vue-SPA shell (confirmed via both WebFetch and `curl --compressed`; page body is
  JS-rendered). Fell back to the DocC markdown source that page is built from:
  <https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/ReferenceManual/LexicalStructure.md>,
  same content as swiftlang/swift-book (mirror of the former apple/swift-book), "Keywords and
  Punctuation" section — this is the primary source for the rendered page, not a secondary summary.
- Windows device names: WebFetch of
  <https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file> (static page, fetched fully).

### Rust — 51/51 agree, 0 disagree

`RUST_KEYWORDS` is exactly {strict keywords, minus `_` and `Self` (folded into `self`)} ∪
{reserved keywords}. Source lists (verbatim, "in all editions" for strict):

- Strict: `_ as async await break const continue crate dyn else enum extern false fn for if
  impl in let loop match mod move mut pub ref return self Self static struct super trait true
  type unsafe use where while` (async/await/dyn added 2018 edition).
- Reserved: `abstract become box do final gen macro override priv try typeof unsized virtual
  yield` — page confirms "`gen` keyword was added as a reserved keyword in the 2024 edition"
  (`try` added 2018 edition).
- Weak (correctly excluded, remain valid identifiers): `'static macro_rules raw safe union`.

Every strict + reserved word above (self-folded) is present in `RUST_KEYWORDS`; no extras, no
gaps. No action needed.

### Java — 50/50 agree, 0 disagree

JLS §3.9 `ReservedKeyword` (verbatim, one block): `abstract continue for new switch assert
default if package synchronized boolean do goto private this break double implements
protected throw byte else import public throws case enum instanceof return transient catch
extends int short try char final interface static void class finally long strictfp volatile
const float native super while _ (underscore)`. That's the 50 words in `JAVA_KEYWORDS` plus
`_`, which cannot appear in a slug anyway.

`ContextualKeyword` (legal as identifiers/package segments outside their special syntax,
confirmed by the page's own text): `exports opens requires uses yield module permits sealed
var non-sealed provides to when open record transitive with`. Correctly excluded, same as
Rust's weak keywords and Kotlin's soft/modifier keywords. No action needed.

### Kotlin — 28/28 agree (hard keywords), 0 disagree

Hard keywords (verbatim, operator forms `as?`, `!in`, `!is` excluded — they contain
characters no slug word can, matching the existing code comment's rationale): `as break class
continue do else false for fun if in interface is null object package return super this throw
true try typealias typeof val var when while`. This is all 28 present in `KOTLIN_KEYWORDS`,
in the same set. No action needed.

Soft keywords (page: "act as keywords in the context in which they are applicable, and they
can be used as identifiers in other contexts"): `by catch constructor delegate dynamic field
file finally get import init param property receiver set setparam value where`. Modifier
keywords (same "can be used as identifiers" wording): `abstract actual annotation companion
const crossinline data enum expect external final infix inline inner internal lateinit
noinline open operator out override private protected public reified sealed suspend tailrec
vararg`. Both groups are correctly excluded — the page itself says they're valid identifiers
outside their keyword context, so as Java-style package segments neither would be a problem.

### Swift — 54/57 agree, 3 missing from `reserved.rs`

`reserved.rs`'s doc comment scopes itself to "declarations, statements, expressions, and
types," excluding "keywords reserved only in particular patterns or contexts." That's the
right scope call, but three keywords were added to the **declarations** group since the code
was written (Swift's ownership/concurrency features) and are missing:

| word | in source (group) | in `reserved.rs` |
|---|---|---|
| `borrowing` | yes (declarations) | no |
| `consuming` | yes (declarations) | no |
| `nonisolated` | yes (declarations) | no |

Source quote (declarations group, verbatim): "`associatedtype`, `borrowing`, `class`,
`consuming`, `deinit`, `enum`, `extension`, `fileprivate`, `func`, `import`, `init`, `inout`,
`internal`, `let`, `nonisolated`, `open`, `operator`, `precedencegroup`, `private`,
`protocol`, `public`, `rethrows`, `static`, `struct`, `subscript`, `typealias`, and `var`."

Everything else in declarations/statements/expressions-and-types (54 words, including the
lowercase folds `Any`→`any` and `Self`→`self`) is already in `RUST`/`SWIFT_KEYWORDS`
correctly — no extras beyond scope. Two notes, not action items:

- `package` is real but sits in Swift's "reserved in particular contexts" group (a
  soft/contextual access modifier since Swift 5.9) — correctly out of scope by the same rule
  that excludes Rust's weak and Kotlin's soft/modifier keywords, not a gap.
- `Protocol`/`Type` (capitalized, contextual metatype keywords) fold to `protocol`/`type`,
  already covered: `protocol` via Swift's own declarations-group keyword, `type` via Rust's
  keyword list — same lowercase collision the case-insensitive check is built on.

### Windows device names — 22/22 agree, 0 disagree, `com0`/`lpt0` confirmed correctly excluded

Source quote (verbatim): "Do not use the following reserved names for the name of a file:
CON, PRN, AUX, NUL, COM1, COM2, COM3, COM4, COM5, COM6, COM7, COM8, COM9, COM¹, COM², COM³,
LPT1, LPT2, ..., LPT9, LPT¹, LPT², and LPT³." Plus a note that Windows treats the ISO 8859-1
superscript digits ¹²³ as valid parts of COM#/LPT# names too, making COM¹–COM³/LPT¹–LPT³
reserved as well.

The page does **not** list `COM0` or `LPT0` — the numbering is 1–9 (plus the superscript
1–3 duplicates), never 0. `WINDOWS_DEVICE_NAMES` in `reserved.rs` already matches this
exactly (`com1`–`com9`, `lpt1`–`lpt9`, `con`, `prn`, `aux`, `nul`). The superscript forms are
non-ASCII and can never appear in a slug (`[a-z][a-z0-9]*`), so they need no entry.

This directly answers the todo's open question and the test that pins it: `com0`/`lpt0`/
`com10`/`lpt10`/`com`/`lpt` are correctly accepted as valid slugs today, and must **stay**
accepted — do not add them.

### Recommendation

Add, test-first, before the first real provisioning run (the slug grammar is frozen once a
project exists, so a later addition orphans resources on the next run):

- `borrowing`, `consuming`, `nonisolated` to `SWIFT_KEYWORDS` in
  `crates/willikins-types/src/reserved.rs` (keep the sorted/lowercase invariant the existing
  `assert_sorted_and_lowercase` test checks).

Nothing to add for Rust, Java, Kotlin, or Windows device names — all verified as exact
matches against their primary sources, given the deliberate, already-documented scope
exclusions (Rust weak keywords, Java contextual keywords, Kotlin soft/modifier keywords,
Swift context-reserved keywords all correctly stay out).

Keep as-is with a reason, nothing else needs an "extra" justification: every word currently
in `reserved.rs` is backed by a live source line; there are no unexplained extras in any of
the five lists.

### Tests to extend

`grep -rn "is_reserved" crates/willikins-types/tests/` hits only
`crates/willikins-types/tests/naming_properties.rs:9,81` — a proptest that calls `is_reserved`
directly (not a hardcoded list), so it needs no change: adding new reserved words only makes
it filter more inputs, safely.

The word-level pins live in `crates/willikins-types/tests/naming_adversarial.rs`:

- Line 168, `reserved_words_are_matched_case_insensitively_and_only_alone`: asserts
  `"match", "Match", "MATCH", "self", "Self", "type", "native", "default", "nul"` are each
  rejected as single-word slugs. This is the test to extend with `"borrowing", "consuming",
  "nonisolated"` (and their multi-word forms, e.g. `"consuming-actor"`, added to the
  `ok` list right below it, mirroring `"nul-island"`).
- Line 185, `windows_device_names_are_reserved_only_for_the_documented_numbers`: already
  pins `com0`/`com10`/`lpt0`/`lpt10`/`com`/`lpt` as accepted (not reserved) — this matches
  the source and must **not** be changed.
- Line 214 (`EnvironmentSlug::parse("nul")`) and the Rust-2024 test
  (`edition_2024_rust_keywords_are_reserved`, `gen`) are unaffected by this verification.

No changes needed in `naming_properties.rs`, `naming_v1_properties.rs`, or `catalog.rs` —
none hardcode a keyword list; they all call `is_reserved` or filter through it.
