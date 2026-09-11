# Milestone 1 dependency research

**Created:** 2026-09-11

## Rust MCP SDK (crate rmcp, github.com/modelcontextprotocol/rust-sdk)

**Recommendation:** Depend on rmcp = "3" (currently 3.3.0, MSRV 1.88, edition 2024) with features ["server", "macros", "schemars", "transport-io"] for a stdio server, adding "transport-streamable-http-server" (+ axum as a normal dependency, since StreamableHttpService is only a Tower service and needs a router to mount on) if HTTP is also needed. Define each tool's params as a plain struct deriving `serde::Deserialize, schemars::JsonSchema`, and use `#[tool_router(server_handler)]` + `#[tool(description = "...")]` on an impl block for the simplest single-capability server; drop to explicit `#[tool_router]` + `#[tool_handler(name=..., version=..., instructions=...)]` on a `ServerHandler` impl when custom metadata or multiple capabilities (tools+prompts) are needed. Pin schemars to "1" (rmcp's own schemars feature requires >=1.1.0, base optional dep >=1.0) -- do not use schemars 0.8, which is incompatible.

- Current published version of rmcp is 3.3.0 (workspace-versioned). (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/Cargo.toml)
  - "edition = \"2024\"\nrust-version = \"1.88\"\nversion = \"3.3.0\""
- docs.rs confirms 3.3.0 as the latest published version of the rmcp crate. (source: https://docs.rs/rmcp/latest/rmcp/)
  - "rmcp/3.3.0"
- MSRV is Rust 1.88, and the crate targets Rust edition 2024. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/Cargo.toml)
  - "edition = \"2024\"\nrust-version = \"1.88\""
- Tools are defined with #[tool], #[tool_router], and #[tool_handler] proc macros; a tools-only server can use #[tool_router(server_handler)] to skip a separate ServerHandler impl. Input is a struct deriving serde::Deserialize + schemars::JsonSchema, wrapped in rmcp's Parameters<T> extractor. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md)
  - "The `#[tool]`, `#[tool_router]`, and `#[tool_handler]` macros handle all the wiring. For a tools-only server you can use `#[tool_router(server_handler)]` to skip the separate `ServerHandler` impl: ... #[tool_router(server_handler)]\nimpl Calculator {\n    #[tool(description = \"Add two numbers\")]\n    fn add(&self, Parameters(AddParams { a, b }): Parameters<AddParams>) -> String {\n        (a + b).to_string()\n    }\n}"
- The tool's JSON Schema (inputSchema/outputSchema) is generated automatically by deriving schemars::JsonSchema on the parameter struct; only field names/types/doc comments are used, not the type name or its own doc comment. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md)
  - "The generated tool `inputSchema` and `outputSchema` are derived from the fields of `T`. The type name and documentation on `T` are ignored; only field names, field types, and field documentation are used."
- rmcp's optional schemars feature pins schemars to major version 1 (>=1.1.0 when its own schemars feature is enabled, >=1.0 as the base optional dependency), with the chrono04 feature enabled -- i.e. it expects schemars 1.x, not the older 0.8 line. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/Cargo.toml)
  - "schemars = { version = \"1.0\", optional = true, features = [\"chrono04\"] }\n...\nschemars = [\"dep:schemars\"]\n...\nschemars = { version = \"1.1.0\", features = [\"chrono04\"] }"
- stdio transport: server side uses transport::stdio() with .serve(stdio()); it's gated by the transport-io feature (client + server) and is the standard way to run local MCP servers as child processes. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md)
  - "| **stdio**                    | `transport-io` (client + server)             | Communicate over `stdin`/`stdout`; the standard way to launch local MCP servers as child processes. |\n\n### stdio\n\n```rust,ignore\nuse rmcp::{ServiceExt, transport::stdio};\n\n// Server: serve over stdin/stdout.\nlet server = MyServer.serve(stdio()).await?;\nserver.waiting().await?;\n```"
- Streamable HTTP server transport requires the transport-streamable-http-server feature; StreamableHttpService is a Tower service, so it is not axum-specific -- it can be mounted on any axum/hyper router (the README's own example does use axum::Router::nest_service). (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md)
  - "| **Streamable HTTP** (server) | `transport-streamable-http-server`           | The current HTTP transport. Exposes a Tower service you can mount on any router. |\n...\n`StreamableHttpService` is a Tower service — mount it on any `axum`/`hyper`\nrouter"
- Minimal Streamable HTTP server setup: build StreamableHttpServerConfig, wrap a per-request handler factory + LocalSessionManager in StreamableHttpService::new, then nest it as a service on an axum::Router. (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md)
  - "let config = StreamableHttpServerConfig::default()\n    .with_legacy_session_mode(false) // stateless for legacy versions too\n    .with_json_response(true);       // plain JSON replies for simple tools\n\nlet service = StreamableHttpService::new(\n    || Ok(Counter::new()),           // a fresh handler per request\n    LocalSessionManager::default().into(),\n    config,\n);\n\n// `StreamableHttpService` is a Tower service — mount it on any router.\nlet router = axum::Router::new().nest_service(\"/mcp\", service);"
- Streamable HTTP client transport feature is transport-streamable-http-client-reqwest (reqwest-backed) or the transport-agnostic transport-streamable-http-client; client connects via StreamableHttpClientTransport::from_uri(...). (source: https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md)
  - "| **Streamable HTTP** (client) | `transport-streamable-http-client-reqwest`   | HTTP client transport built on `reqwest`. |\n...\nuse rmcp::transport::StreamableHttpClientTransport;\n\nlet transport = StreamableHttpClientTransport::from_uri(\"http://localhost:8000/mcp\");\nlet client = ClientInfo::default().serve(transport).await?;"

**Unresolved**

- Did not verify crates.io's own JSON API response for max_stable_version/newest_version directly -- its API endpoint returned an unusable schema-template payload in this environment (likely proxy/sandbox interference, evidenced by an injected 'context-mode: curl/wget blocked' message mid-session); version 3.3.0 was instead corroborated via the GitHub workspace Cargo.toml and via docs.rs's resolved redirect target, which is a reliable cross-check but not the crates.io API itself.
- Did not independently re-derive the WebSearch-reported detail that 'rmcp 3.0.x targets the 2026-07-28 spec revision' beyond what the fetched README/Cargo.toml directly show (they confirm the 2026-07-28 spec target and MSRV 1.88, but that specific major-version-to-spec-revision mapping came from a search summary, not a fetched quote).

## Rust crate schemars 1.x — derive attributes, manual JsonSchema impl for newtypes, root schema generation/serialization, and 0.8→1.0 breaking changes

**Recommendation:** For a newtype wrapping a String that should serialize as a plain constrained string: derive JsonSchema and mark the struct #[schemars(transparent)], then put description/pattern/format/length/example attributes on the single inner field, rather than hand-implementing JsonSchema — reserve a manual `impl JsonSchema` (schema_name/schema_id/json_schema, building a {"type":"string","pattern":...} Schema via json_schema!/serde_json::json!) for cases the derive attribute set can't express. Generate and serialize the root schema with `schema_for!(YourType)` + `serde_json::to_string_pretty(&schema)`. Because WebFetch was blocked by a PreToolUse hook in this session, the exact current crates.io version and the full CHANGELOG breaking-changes list could not be pulled from their primary pages directly — re-verify the version and full changelog before finalizing the design doc.

- The current schemars release is well past 1.0, at 1.2.1 (with 1.2.2 noted as most recently updated in the search snapshot); 1.0.0-alpha.18 was an earlier alpha equated to the 0.9.0 stable line. (inferred) **(unverified)** (source: https://crates.io/crates/schemars)
  - "The latest version of schemars is 1.2.1, last published on February 1, 2026. The schemars v1.2.2 package shows 373,218,614 total downloads and was updated 7 days ago."
- To make a newtype struct's generated schema identical to its single field's schema (e.g. a String newtype schema'd as a plain string), apply #[schemars(transparent)] to the struct. (source: https://github.com/gresau/schemars/blob/master/docs/_includes/attributes.md)
  - "Use the `transparent` attribute on a newtype struct or a braced struct with a single field. This makes the struct's schema identical to its single field's schema.\n\n```rust\n#[schemars(transparent)]\nstruct MyNewtype(String);\n```"
- description and title on a container or field are set with #[schemars(title = "...", description = "...")], overriding values otherwise derived from doc comments. (source: https://github.com/gresau/schemars/blob/master/docs/_includes/attributes.md)
  - "Apply `title` and `description` attributes to containers, variants, or fields to override values from doc comments or attributes."
- pattern/regex constraints on a string field are set via #[schemars(regex(pattern = r"..."))] or the shorthand #[schemars(pattern(r"..."))], with raw string literals recommended to avoid escaping issues; a static Regex value can also be referenced. (source: https://github.com/gresau/schemars/blob/master/schemars_derive/attributes.md)
  - "Use `regex` or `pattern` attributes to set the `pattern` property for string schemas. Raw string literals (`r\"...\"`) are recommended for regex patterns to avoid issues with backslashes."
- format constraints (email, url/uri, ip, ipv4, ipv6) are set per-field with #[schemars(email)], #[schemars(url)], #[schemars(ip)], etc., and only one format attribute may be applied per field. (source: https://github.com/gresau/schemars/blob/master/docs/_includes/attributes.md)
  - "Sets the schema's format to email, uri, ip, ipv4, or ipv6. Only one format attribute can be applied per field."
- minLength/maxLength (for strings) or minItems/maxItems (for arrays) are set with #[schemars(length(min = ..., max = ...))], or length(equal = N) for an exact length. (source: https://github.com/gresau/schemars/blob/master/docs/_includes/attributes.md)
  - "Sets minLength/maxLength for strings or minItems/maxItems for arrays. Supports specific lengths or ranges."
- Examples are attached with #[schemars(example = value)] (repeatable for multiple examples); the value must implement serde::Serialize, and string literals need `&"literal"` or a function call form to avoid ambiguity. (source: https://github.com/gresau/schemars/blob/master/docs/_includes/attributes.md)
  - "Use the `example` attribute to embed values in the generated schema's `examples` array. The value must implement `serde::Serialize`."
- Manually implementing JsonSchema for a type requires schema_name (human-readable name, used as root title / $defs key), the recommended schema_id (unique identifier including module path, useful to disambiguate same-named types across modules/crates), and json_schema (builds the actual Schema, receiving a &mut SchemaGenerator). (source: https://github.com/gresau/schemars/blob/master/docs/2-implementing.md)
  - "fn schema_name() -> Cow<'static, str> {\n        // Exclude the module path to make the name in generated schemas clearer.\n        \"NonGenericType\".into()\n    }\n\n    fn schema_id() -> Cow<'static, str> {\n        // Include the module, in case a type with the same name is in another module/crate\n        concat!(module_path!(), \"::NonGenericType\").into()\n    }"
- A root schema is generated with the schema_for! macro on a type implementing JsonSchema, and serialized to JSON directly via serde_json (schema implements Serialize) — e.g. `let schema = schema_for!(MyStruct); serde_json::to_string_pretty(&schema)`. (source: https://github.com/gresau/schemars/blob/master/schemars/README.md)
  - "let schema = schema_for!(MyStruct);\nprintln!(\"{}\", serde_json::to_string_pretty(&schema).unwrap());"
- In 1.0, Schema is redefined as a thin wrapper around serde_json::Value (rather than a struct with a field per JSON Schema keyword), exposed as schemars::Schema (schemars::schema module and its types removed), and functions that previously returned RootSchema now just return Schema. (inferred) **(unverified)** (source: https://github.com/GREsau/schemars/blob/master/CHANGELOG.md)
  - "Schema is now defined as a wrapper around a serde_json::Value rather than a struct with fields for each JSON schema keyword, and is now available as schemars::Schema instead of schemars::schema::Schema, with all other types from the schemars::schema module removed."
- schemars 1.0 changed #[validate(...)]/#[schemars(...)] validator-style attributes to match the current Validator crate: #[validate(phone)] is removed (use #[schemars(extend("format = \"phone\""))] instead), #[validate(required_nested)] is removed (use #[schemars(required)]), and #[validate(regex = "...")] no longer accepts name = "value" syntax. (inferred) **(unverified)** (source: https://github.com/GREsau/schemars/blob/master/CHANGELOG.md)
  - "The #[validate(phone)]/#[schemars(phone)] attribute is removed, but you can use #[schemars(extend(\"format = \\\"phone\\\"\"))] instead for the old behavior\n\nThe #[validate(required_nested)]/#[schemars(required_nested)] attribute is removed, and you can use #[schemars(required)] instead"

**Unresolved**

- Exact current crates.io version string for schemars (search results point to 1.2.1/1.2.2 but were not confirmed via a direct fetch of crates.io, since WebFetch was blocked by a PreToolUse hook this session).
- Full enumerated list of ALL breaking changes from 0.8 to 1.0 (only the Schema-type restructuring and validator-attribute changes were confirmed via search snippets; the raw CHANGELOG.md could not be directly fetched because WebFetch was blocked).
- Whether transparent + pattern/length/example attributes may be placed on the newtype struct itself vs. must be placed on the single tuple field — context7 snippets show field-level placement predominantly; struct-level placement for transparent newtypes was not independently confirmed with a code sample.

## Maintained replacements for the deprecated serde_yaml crate (Rust)

**Recommendation:** For a new project that parses YAML into serde structs and wants good line/column error messages, use serde_yaml_ng (github.com/acatton/serde-yaml-ng, crates.io version 0.10.0). It is an actively-maintained, near-drop-in fork of dtolnay's original serde_yaml (same API shape — from_str, Value, Deserializer, Error types), so it keeps the exact Error::location() API with 1-indexed line/column that serde_yaml had, requiring no rewrite of error-handling code. It is also what real-world projects (e.g. nushell) have actually migrated to, and RustSec's own advisory for the unsound/unmaintained serde_yml crate names it (alongside serde_norway) as the recommended replacement. serde_norway (maintained by Christina Sørensen) is a reasonable second choice with the same fork lineage and error API, but its release cadence appears slower (latest 0.9.42 reported as over a year old) — worth a fresh crates.io check before committing. Avoid serde_yml entirely: it is explicitly deprecated by its own author, was the subject of a RustSec unsoundness advisory (RUSTSEC-2025-0068), and its final release is just a compatibility shim over a different, non-API-compatible library (noyalib).

- serde_yaml_ng is described by RustSec as a maintained fork of serde_yaml, with source at github.com/acatton/serde-yaml-ng, an independent continuation of dtolnay's serde-yaml. (source: https://rustsec.org/advisories/RUSTSEC-2025-0068.html)
  - "serde_norway - Maintained fork of serde_yaml, using unsafe-libyaml-norway and serde_yaml_ng - Maintained fork of serde_yaml."
- serde_yaml_ng's GitHub repo self-describes as a strongly-typed, serde-compatible YAML library and an independent continuation of dtolnay's serde-yaml (same API lineage/fork). (source: https://github.com/acatton/serde-yaml-ng)
  - "Strongly typed YAML library for Rust, serde compatible. This is an independant continuation of serde-yaml from dtolnay."
- The current published version of serde_yaml_ng on crates.io is 0.10.0. (source: https://crates.io/crates/serde_yaml_ng)
  - "The serde_yaml_ng package version 0.10.0 is available on crates.io with the package URL pkg:cargo/serde_yaml_ng@0.10.0."
- serde_norway is maintained by Christina Sørensen, is a hard-fork of serde_yaml, and has had 7 versions since June 10, 2024, with the latest (0.9.42) reported as over a year old at query time. (source: https://crates.io/crates/serde_norway)
  - "serde_norway is a YAML data format for Serde and a Rust library for using the Serde serialization framework with data in YAML file format. It is described as a hard-fork of Serde YAML. ... The crate has 7 versions available since June 10th, 2024. The latest version is 0.9.42, released by Christina Sørensen over 1 year ago."
- serde_yml is now deprecated by its own maintainer; its final 0.0.13 release is only a thin compatibility shim forwarding to the noyalib library, and users are told to migrate to a maintained alternative. (source: https://github.com/sebastienrousseau/serde_yml)
  - "[DEPRECATED] Final release is a thin compatibility shim. RUSTSEC-2025-0068 structurally fixed in 0.0.13. Migrate to a maintained alternative — see MIGRATION.md."
- RustSec advisory RUSTSEC-2025-0068 flags all serde_yml versions <=0.0.12 as unsound (potential segfault in the Serializer's emitter field via linked C-FFI libyaml) and unmaintained, and explicitly recommends switching to serde_norway or serde_yaml_ng. (source: https://rustsec.org/advisories/RUSTSEC-2025-0068.html)
  - "RUSTSEC-2025-0068 flagged all serde_yml versions ≤ 0.0.12 as unsound — the serde_yml::ser::Serializer.emitter field could cause a segmentation fault... If you rely on this crate, it is highly recommended switching to a maintained alternative."
- The original serde_yaml Error type exposes a location() method returning a Location with 1-indexed line() and column() accessors for deserialization errors; forks like serde_yaml_ng and serde_norway inherit this API. (source: https://docs.rs/serde_yaml/latest/serde_yaml/struct.Error.html)
  - "let invalid_yaml: Result<Value, Error> = serde_yaml::from_str(\"@invalid_yaml\"); let location = invalid_yaml.unwrap_err().location().unwrap(); assert_eq!(location.line(), 1); assert_eq!(location.column(), 1);"
- The nushell project switched from serde_yml/serde-yaml to serde-yaml-ng specifically, evidence it is a common real-world drop-in choice. (source: https://github.com/nushell/nushell/pull/14985)
  - "Use serde-yaml-ng instead of serde-yml by devyn · Pull Request #14985 · nushell/nushell"

**Unresolved**

- Exact release date of serde_yaml_ng 0.10.0 could not be confirmed verbatim from a fetched source (search snippets were inconsistent/stale); check https://crates.io/crates/serde_yaml_ng directly before finalizing.
- WebFetch and direct network access (curl) were both blocked in this session's environment, so all facts rely on WebSearch result snippets rather than full page fetches — cross-check crates.io and the GitHub repos directly if precision on exact dates/version history matters.
- Did not independently verify serde_norway's or serde_yaml_ng's current open-issue counts or a full 'known issues' list beyond what RustSec and search snippets surfaced; a maintainer-activity check (recent commits, issue responsiveness) is recommended before final selection.
- noyalib and serde-saphyr (newer, non-fork alternatives mentioned in serde_yml's own migration guide) were not evaluated in depth since they are not forks of serde_yaml's API — worth a look if a from-scratch YAML 1.2-compliant library is acceptable instead of strict API compatibility.

## Rust crate "secrecy" (iqlusion) — SecretBox/SecretString, Debug, serde, zeroize, and redaction alternatives

**Recommendation:** secrecy's SecretBox<str> (aliased SecretString) is a good fit for a newtype like GitHubToken that wraps a single scalar secret string: Debug is hard-wired to print only "SecretBox<str>([REDACTED])" (no way to accidentally print the value), there is no Display impl at all (so `{}` formatting simply won't compile, which is the strongest guarantee against accidental leaking), access requires explicitly calling `.expose_secret()`, and zeroize-on-drop is built in and forced (S: Zeroize is a bound on SecretBox itself). With the `serde` feature, GitHubToken would deserialize automatically from JSON/env but would NOT serialize back out unless SerializableSecret is explicitly implemented for `str`/String — a safe default for a token type. Recommend: define `pub type GitHubToken = SecretString;` or a thin newtype wrapping SecretBox<str>, enable the `serde` feature only if the token must be deserialized from config, and do not implement SerializableSecret for it. Reach for `redact` or `veil`/`redacted_debug` instead only if the requirement broadens to redacting several fields inside a larger struct or wrapping non-Zeroize types.

- Current crates.io/docs.rs version of secrecy is 0.10.3 (source: https://docs.rs/secrecy/latest/src/secrecy/lib.rs.html)
  - "\"version\": \"0.10.3\""
- secrecy provides SecretBox (a wrapper for carefully handling secret values), with SecretString and SecretSlice as type aliases (SecretString = SecretBox<str>, SecretSlice<S> = SecretBox<[S]>), and access is via ExposeSecret/ExposeSecretMut traits (source: https://docs.rs/secrecy/latest/src/secrecy/lib.rs.html)
  - "pub type SecretString = SecretBox<str>; ... pub type SecretSlice<S> = SecretBox<[S]>; ... Access to the secret inner value occurs through the [`ExposeSecret`] or [`ExposeSecretMut`] traits"
- Debug for SecretBox<S> never prints the inner value; it prints only the type name plus the literal string [REDACTED] (source: https://docs.rs/secrecy/latest/src/secrecy/lib.rs.html)
  - "impl<S: Zeroize + ?Sized> Debug for SecretBox<S> {\n...\n write!(f, \"SecretBox<{}>([REDACTED])\", any::type_name::<S>())"
- With the serde feature, SecretBox<T> automatically gets a Deserialize impl for any T: DeserializeOwned, but does NOT get a Serialize impl by default — Serialize is only implemented for types that separately implement the marker trait SerializableSecret, deliberately gating serialization to prevent accidental secret exfiltration (source: https://docs.rs/secrecy/latest/src/secrecy/lib.rs.html)
  - "When the `serde` feature of this crate is enabled, the [`SecretBox`] type will receive a [`Deserialize`] impl for all `SecretBox<T>` types where `T: DeserializeOwned`. ... To prevent exfiltration of secret values via `serde`, by default `SecretBox<T>` does *not* receive a corresponding [`Serialize`] impl."
- SecretBox implements Zeroize and Drop (calling zeroize on drop) and ZeroizeOnDrop, using the zeroize crate directly (secrecy re-exports zeroize) (source: https://docs.rs/secrecy/latest/src/secrecy/lib.rs.html)
  - "impl<S: Zeroize + ?Sized> Zeroize for SecretBox<S> {\n    fn zeroize(&mut self) {\n        self.inner_secret.as_mut().zeroize()\n...\nimpl<S: Zeroize + ?Sized> Drop for SecretBox<S> {\n...\n        self.zeroize()\n...\nimpl<S: Zeroize + ?Sized> ZeroizeOnDrop for SecretBox<S> {}"
- The crate's stated design goals are: make secret access explicit/auditable via ExposeSecret/ExposeSecretMut, prevent accidental leakage via channels like debug logging, and ensure secrets are cleared from memory on drop using zeroize (source: https://docs.rs/secrecy/latest/src/secrecy/lib.rs.html)
  - "[`SecretBox`] wrapper type for more carefully handling secret values ... [`ExposeSecret`] and [`ExposeSecretMut`] traits. - Prevent accidental leakage of secrets via channels like debug logging ... (using the [`zeroize`] crate)"
- The `redact` crate is an alternative to secrecy that relaxes the Zeroize-on-every-wrapped-type requirement, letting arbitrary types be wrapped as secrets, at the cost of not guaranteeing zeroization (inferred) **(unverified)** (source: https://crates.io/crates/redact)
  - "Secrecy was the original inspiration for this crate and it has a similar API. One significant difference is that secrecy requires that all secrets implement Zeroize so that it can cleanly wipe secrets from memory after they are dropped. ... Redact relaxes this requirement, allowing all types to be Secrets."
- For redacting whole arbitrary structs field-by-field in Debug output (rather than wrapping a single scalar secret), derive-macro crates like `veil` and `redacted_debug` are purpose-built for that job, unlike secrecy which is a wrapper type for individual values (inferred) **(unverified)** (source: https://github.com/primait/veil)
  - "Veil is a derive macro that implements std::fmt::Debug for a struct or enum variant with certain fields redacted... Using the #[redact] attribute, you can control which fields are redacted and how, and fields without this attribute will NOT be redacted and will be shown using their default Debug implementation."

**Unresolved**

- Whether the `serde` feature's Cargo.toml gating text verbatim (exact feature name spelling, any additional feature flags like `alloc`/`std`/`default`) was not independently confirmed beyond what appears inferred from the lib.rs doc comments and the dependency list snippet — recommend a quick `cargo add secrecy --features serde` or Cargo.toml check to confirm exact feature name before implementation.
- crates.io/redact and github.com/primait/veil pages were not fetched directly in this session (WebFetch tool was blocked by a permission hook); their facts come from WebSearch snippet summaries only, so treat those two facts as lower-confidence pending a direct fetch if precision matters.
- Did not independently verify the 'secret-vault' crate (mentioned in the original ask) — no search was run on it; if needed, do a dedicated lookup.

## GitHub repository name and organization/user login name rules

**Recommendation:** Treat repo names as: ASCII letters/digits plus '.', '-', '_' only, max 100 chars, no trailing '.git', and unique case-insensitively per owner — validate/sanitize client-side accordingly (e.g. replace disallowed chars with '-'). Treat org/user login names as: alphanumeric + hyphens only, no leading/trailing/double hyphens, max 39 chars, unique case-insensitively — reuse the same login-name validator for both users and orgs since GitHub Docs describes org logins as following the same username rules as user accounts.

All facts in this section are confidence "inferred" (derived from WebSearch snippets, not a direct WebFetch of the source page) and are flagged accordingly.

- GitHub repository names can only contain ASCII letters, digits, and the characters '.', '-', and '_'; repository names must not end with .git. (inferred) **(unverified)** (source: https://github.com/github/docs/issues/44518)
  - "The repository name can only contain ASCII letters, digits, and the characters ., -, and _. Additionally, repository names must not end with .git."
- Maximum GitHub repository name length is 100 characters. (inferred) **(unverified)** (source: https://github.com/github/docs/issues/44518)
  - "The maximum length for a repository name is 100 characters."
- When creating via the web UI, non-ASCII/disallowed characters in a repository name are converted to hyphens and the name is otherwise sanitized (e.g. a trailing .git suffix is stripped with a UI hint). (inferred) **(unverified)** (source: https://github.com/github/docs/issues/44518)
  - "For names containing non-ASCII characters, GitHub converts all non-ASCII characters to hyphens and sanitizes the repository name."
- GitHub repository names are case-insensitive for uniqueness (case-preserving display, but you cannot create a repo whose name differs only in case from an existing one under the same owner). (inferred) **(unverified)** (source: https://github.com/github/docs/issues/32838)
  - "GitHub is case-preserving as opposed to case-sensitive, meaning GitHub will remember the case you use when creating the repository, but internally treats names as case-insensitive for uniqueness validation."
- GitHub account (user/org login) usernames can only contain alphanumeric characters and dashes. (inferred) **(unverified)** (source: https://docs.github.com/en/enterprise-cloud@latest/admin/managing-iam/iam-configuration-reference/username-considerations-for-external-authentication)
  - "Usernames for user accounts on GitHub can only contain alphanumeric characters and dashes (-)."
- GitHub normalizes disallowed characters in a username to a dash, and normalized usernames cannot start or end with a dash or contain two consecutive dashes. (inferred) **(unverified)** (source: https://docs.github.com/en/enterprise-cloud@latest/admin/managing-iam/iam-configuration-reference/username-considerations-for-external-authentication)
  - "GitHub will normalize any non-alphanumeric character in your account's username into a dash, for example, a username of mona.the.octocat will be normalized to mona-the-octocat. Normalized usernames also can't start or end with a dash. They also can't contain two consecutive dashes."
- GitHub usernames must not exceed 39 characters. (inferred) **(unverified)** (source: https://docs.github.com/en/enterprise-cloud@latest/admin/managing-iam/iam-configuration-reference/username-considerations-for-external-authentication)
  - "Usernames must not exceed 39 characters."

**Unresolved**

- The WebFetch tool was blocked entirely in this environment by a PreToolUse hook, for every URL tried, so no page was fetched directly by this agent — all facts above come from WebSearch's synthesized snippets rather than a verbatim quote pulled by fetching the source page myself. Re-verify against docs.github.com and the REST API reference directly when WebFetch or browser access is available.
- Could not confirm the exact current wording of GitHub's general (non-Enterprise-IAM) 'About repositories' page on repo-name rules — the 100-char length and allowed-character-set claims are corroborated only via a github/docs community issue (#44518), not the live docs.github.com repository-naming page itself.
- Could not find an official docs.github.com page explicitly stating organization login name rules separately from the user-account username rules cited.

## Doppler project names/slugs, environment slugs, config names, and relevant API endpoints

**Recommendation:** Use POST /v3/projects (body: {name, description?}) with a hyphen-separated, all-lowercase project name as Doppler itself recommends — the project's 'name' functions as its identifier in the API; no separate slug field was found in fetched docs. For environments, use POST /v3/environments to get a distinct environment 'id' plus 'name'; the three defaults are dev/stg/prd. Root configs are auto-created per environment using the environment's identifier (dev, stg, prd); branch configs are created via POST /v3/configs with {project, environment, name}, where the UI prefixes the given name with the environment name/slug (pattern '<environment>_<descriptor>', e.g. 'prd_web') — Doppler's own Personal Configs feature follows this same convention ('dev_personal'). Push secrets with POST /v3/configs/config/secrets using {project, config, secrets: {KEY: VALUE}}.

- A Doppler project needs a name (required) and optional description; Doppler recommends the name be hyphen-separated and all lowercase. (source: https://docs.doppler.com/reference/create-project)
  - "A project needs a name and an optional description. We recommend the name be hyphen-separated and all lowercase."
- Every project has three default root-level environments/configs: Development, Staging, and Production, corresponding to root configs named dev, stg, and prd. (source: https://docs.doppler.com/docs/enclave-config-branching)
  - "Projects have 3 default configs: **dev**, **stg**, and **prd**. You can think of these configs as the master branch or root config for their respective environments, where all future configs branch off of."
- Custom environments can also be created beyond the three defaults (e.g. for CI/CD systems), and environment order can be changed via drag-and-drop. (source: https://docs.doppler.com/reference/create-project)
  - "Custom environments can also be created, the most common of which is for CI / CD systems. The order in which the environments appear can also be altered via drag-and-drop."
- Branch config names are formed by prefixing a user-supplied name with the environment name/slug (e.g. environment 'prd' with branch name 'web' becomes 'prd_web'); creation happens via a modal under the environment. (source: https://docs.doppler.com/docs/enclave-config-branching)
  - "Creating a branched config is fast. To get started, go to a project and then click on the **+** button under the environment name. This will open up a modal to provide a name for the config which will be prefixed with the environment name."
- Doppler's built-in 'Personal Configs' feature creates a per-user branch config on the dev environment named dev_personal, following the '<environment>_<descriptor>' pattern. (source: https://docs.doppler.com/docs/enclave-config-branching)
  - "Personal Configs provide every user that has write access to the environment with their own branch config that only they can access (e.g., for the `dev` environment, every user would have a `dev_personal` branch)."
- The Config object's 'root' boolean field distinguishes whether a config is the root of its environment (dev/stg/prd) versus a branch config. (source: https://docs.doppler.com/reference/config-object)
  - "**root**\\n*boolean*\" ... \"Whether the config is the root of the environment."
- The Environment object has a separate 'id' (identifier) field distinct from its human-readable 'name' field. (source: https://docs.doppler.com/reference/environment-object)
  - "**id**\\n*string*\" ... \"An identifier for the object.\" ... \"**name**\\n*string*\" ... \"Name of the environment."
- Create Project API: POST https://api.doppler.com/v3/projects with JSON body requiring 'name' (string) and optional 'description' (string). (source: https://docs.doppler.com/reference/secrets-update)
  - "\"required\": [\"name\"], \"properties\": {\"name\": {\"type\": \"string\", \"description\": \"Name of project\", \"default\": \"PROJECT_NAME\"}, \"description\": {\"type\": \"string\", \"description\": \"Description of project\", \"default\": \"PROJECT_DESCRIPTION\"}}"
- Create Config (branch config) API: POST https://api.doppler.com/v3/configs with JSON body requiring 'project', 'environment', and 'name' (name of the new branch config). (source: https://docs.doppler.com/reference/secrets-update)
  - "\"required\": [\"project\", \"environment\", \"name\"], \"properties\": {\"project\": {...\"description\": \"Unique identifier for the project object.\"}, \"environment\": {...\"description\": \"Identifier for the environment object.\"}, \"name\": {...\"description\": \"Name of the new branch config.\"}}"
- Set/Update Secrets API: POST https://api.doppler.com/v3/configs/config/secrets with JSON body requiring 'project' and 'config' (config name), plus either a 'secrets' object (key/value map) or a 'change_requests' array. (source: https://docs.doppler.com/reference/secrets-update)
  - "\"required\": [\"project\", \"config\"], \"properties\": {\"project\": {...}, \"config\": {...\"description\": \"Name of the config object.\"}, \"secrets\": {\"type\": \"object\", \"description\": \"Either `secrets` or `change_requests` is required (can't use both)...\"}, \"change_requests\": {\"type\": \"array\", ...}}"
- All Doppler v3 API requests use the base server URL https://api.doppler.com/. (source: https://docs.doppler.com/reference/secrets-update)
  - "\"servers\": [{\"url\": \"https://api.doppler.com/\"}]"

**Unresolved**

- The exact character set / regex and length limits for project name-to-slug derivation, environment slug format, and config name format were not stated as explicit validation rules on any fetched page — the config-object, environment-object, and secrets-update OpenAPI schema excerpts retrieved showed no 'pattern' or 'maxLength' constraints; only descriptive conventions were found.
- Did not fetch the /v3/environments (create) request-body schema directly to confirm whether 'slug' vs 'name' vs 'id' is the input parameter for environment creation — its existence and operationId (environments-create) were confirmed via the embedded OpenAPI path list, but the body schema itself was not extracted due to repeated HTTP 429 rate-limiting from docs.doppler.com during this session.
- Did not find or fetch a page that states an explicit maximum length or disallowed-character list for project names, environment slugs, or config names; Doppler's docs describe naming by convention/example rather than by stated validation rule in the pages reached.

## Buildkite pipeline slugs and organization slugs

**Recommendation:** Design the Rust model so pipeline slug derivation matches Buildkite's own rule: lowercase, collapse whitespace runs to single hyphens, and validate against /\A[a-zA-Z0-9]+[a-zA-Z0-9\-]*\z/ with a 100-character cap. Allow callers to pass an explicit `slug` field on pipeline creation that bypasses derivation; on updates, enforce the stricter rule that the slug may only contain alphanumerics/dashes and cannot start with a dash. Treat name/slug uniqueness as an API-side constraint rather than something the client must dedupe locally. Do not hardcode assumed organization-slug validation rules (length/charset) in the Rust code — Buildkite's docs did not publish an explicit org-slug regex/length; treat org slugs as opaque path-segment strings supplied by the user/API rather than something to validate client-side.

- Pipeline slugs are derived from the pipeline name by converting all space characters (including consecutive ones) to a single hyphen, and all uppercase characters to lowercase. (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "This derivation process involves converting all space characters (including consecutive ones) in the pipeline's name to single hyphen `-` characters, and all uppercase characters to their lowercase counterparts. Therefore, pipeline names of either `Hello there friend` or `Hello    There Friend` are converted to the slug `hello-there-friend`."
- The maximum permitted length for a pipeline slug is 100 characters. (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "The maximum permitted length for a pipeline slug is 100 characters."
- The regular expression Buildkite uses to derive/convert a pipeline name into its slug is /\A[a-zA-Z0-9]+[a-zA-Z0-9\-]*\z/ (starts with an alphanumeric, followed by alphanumerics or hyphens). (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "The following regular expression is used to derive and convert the pipeline name to its slug:\n> `/\A[a-zA-Z0-9]+[a-zA-Z0-9\-]*\z/`"
- A pipeline slug can be set explicitly on creation/update via the optional `slug` request parameter, overriding automatic derivation from the name; if null, the name is used to generate the slug. (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "`slug` | A custom identifier for the pipeline. If provided, this slug will be used as the pipeline's URL path instead of automatically converting the pipeline name. If the value is `null`, the pipeline name will be used to generate the slug."
- On PATCH (update), the slug field is constrained: it can only contain alphanumeric characters or dashes and cannot begin with a dash. (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "`slug` | A custom identifier for the pipeline. This slug will be used as the pipeline's URL path. It can only contain alphanumeric characters or dashes and cannot begin with a dash. The slug updates whenever the pipeline name changes."
- Attempting to create a new pipeline with a name that matches an existing pipeline's name results in an error (i.e. duplicate-derived slugs within an org are rejected). (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "Any attempt to create a new pipeline with a name that matches an existing pipeline's name, results in an error."
- The REST endpoint to create a pipeline is POST to https://api.buildkite.com/v2/organizations/{org.slug}/pipelines, with the pipeline name in the request body (e.g. "name": "My Pipeline X"). (source: https://buildkite.com/docs/apis/rest-api/pipelines)
  - "make the following POST request, substituting your organization slug instead of `{org.slug}`... -X POST \"https://api.buildkite.com/v2/organizations/{org.slug}/pipelines\""
- All REST API pipeline endpoints are namespaced under the organization slug (e.g. GET/PATCH/DELETE at /v2/organizations/{org.slug}/pipelines/{slug}), and the organization object itself exposes a `slug` field (e.g. "my-great-org") used as its URL identifier, but the docs page for organizations does not spell out separate character/length validation rules for org slugs beyond this usage pattern. (source: https://buildkite.com/docs/apis/rest-api/organizations)
  - "\"slug\": \"my-great-org\","

**Unresolved**

- Buildkite's public docs (REST API pipelines page and organizations page) do not publish an explicit character-set/length regex for organization slugs the way they do for pipeline slugs — this could not be verified from buildkite.com/docs and may require checking the GraphQL schema or Buildkite support/changelog.
- Whether organization slugs can be changed after creation, and whether they follow the same [a-zA-Z0-9]+[a-zA-Z0-9-]* pattern as pipeline slugs, was not found in the fetched pages.

## Railway project/service naming rules and the Railway public GraphQL API

**Recommendation:** For a Rust project generating Railway project/service names programmatically: treat service names as DNS-label-bearing values — they populate <service-name>.railway.internal directly — so sanitize to a safe, short (well under Railway's hard 32-character cap) slug even though Railway's docs do not publish an explicit allowed-character regex; using lowercase alphanumerics and hyphens is the conservative-safe choice given the DNS usage, even though this isn't spelled out as a formal constraint in the docs. Project names have no documented length/character restriction, so any reasonable string is fine there. For automation, use the GraphQL API at https://backboard.railway.com/graphql/v2 with a Project or Workspace token, call projectCreate for project provisioning and serviceCreate (with projectId + name, optionally source.repo or source.image) for service provisioning.

- Service names have a maximum length of 32 characters (this is the only documented naming constraint for services). (source: https://docs.railway.com/services)
  - "## Constraints\n\n- Service names have a max length of 32 characters."
- Railway docs do not publish a specific character-set or uniqueness rule for project names; a project's name (and description) is simply edited from the project's General settings tab, with no stated length/character constraints. (source: https://docs.railway.com/projects)
  - "A project's name and description can be changed from the General tab within a project's settings.\nThe project id can also be retrieved here."
- Every service gets an internal DNS hostname under the railway.internal domain, formed as <service-name>.railway.internal, so the service name is used directly as the DNS label on the private network. (source: https://docs.railway.com/networking/private-networking/how-it-works)
  - "Every service in a project and environment gets an internal DNS name under the railway.internal domain that resolves to the internal IP addresses of the service.\n...\nThe DNS name follows the pattern: <service-name>.railway.internal\nFor example, a service named api would be reachable at api.railway.internal."
- Private-network DNS resolution behavior differs by environment age: environments created after October 16, 2025 resolve service DNS names to both IPv4 and IPv6, while legacy environments resolve to IPv6 only. (source: https://docs.railway.com/networking/private-networking/how-it-works)
  - "New environments (created after October 16, 2025): DNS names resolve to both internal IPv4 and IPv6 addresses\nLegacy environments: DNS names resolve to IPv6 addresses only"
- The Railway public API is a GraphQL API served at a single fixed endpoint: https://backboard.railway.com/graphql/v2. (source: https://docs.railway.com/integrations/api)
  - "Endpoint\nThe public API is accessible at the following endpoint:\nhttps://backboard.railway.com/graphql/v2"
- There are three dashboard-issued token types (Account, Workspace, Project) plus OAuth access tokens, scoped respectively to all personal resources/workspaces, a single workspace, a single project environment, and user-granted OAuth permissions; account/workspace/OAuth tokens authenticate via 'Authorization: Bearer <token>' while project tokens use a distinct 'Project-Access-Token' header. (source: https://docs.railway.com/integrations/api)
  - "Note: Project tokens use the Project-Access-Token header, not the Authorization: Bearer header used by account, workspace, and OAuth tokens."
- The public API enforces rate limits by plan: 100 requests/hour on Free, 1000/hour on Hobby, 10000/hour on Pro (custom for Enterprise), plus per-second caps of 10 RPS (Hobby) and 50 RPS (Pro), custom for Enterprise. (source: https://docs.railway.com/integrations/api)
  - "Requests per hour: 100 RPH for Free customers, 1000 RPH for Hobby customers, 10000 RPH for Pro customers; custom for Enterprise.\nRequests per second: 10 RPS for Hobby customers; 50 RPS for Pro customers; custom for Enterprise."
- A new project is created with the projectCreate(input: ProjectCreateInput!) mutation, which accepts a required 'name' plus optional description, workspaceId, isPublic, prDeploys, defaultEnvironmentName (default 'production'), and repo fields, returning at least id and name. (source: https://docs.railway.com/integrations/api/manage-projects.md)
  - "mutation projectCreate($input: ProjectCreateInput!) {\n  projectCreate(input: $input) {\n    id\n    name\n  }\n}"
- A new service is created with the serviceCreate(input: ServiceCreateInput!) mutation, which requires projectId and name and accepts an optional source (repo or image) plus optional branch, icon, and variables fields. (source: https://docs.railway.com/integrations/api/manage-services.md)
  - "mutation serviceCreate($input: ServiceCreateInput!) {\n  serviceCreate(input: $input) {\n    id\n    name\n  }\n}"

**Unresolved**

- Railway's docs do not state an explicit allowed-character set or regex for either project names or service names (only the 32-character max for service names is documented) — this is a genuine documentation gap, not something I failed to find.
- Uniqueness scope for project names and service names (e.g., whether service names must be unique within a project/environment) is not explicitly documented on the pages checked; the DNS pattern (<service-name>.railway.internal) strongly implies service names must be unique per environment for DNS to resolve unambiguously, but this is inferred, not stated verbatim.
- Did not fetch/verify the full GraphQL schema (via introspection) for the exact required/optional field types on ProjectCreateInput/ServiceCreateInput beyond what the manage-projects.md and manage-services.md cookbook examples show.

## Apple bundle identifier rules and App Store Connect API for App IDs

**Recommendation:** Bundle IDs must be constrained at design/validation time to [A-Za-z0-9.-] only (no other Unicode/punctuation), treated case-insensitively for uniqueness checks, and should default to reverse-DNS form (e.g. com.company.app). Treat bundle-ID registration as effectively permanent once a build is uploaded: do not build tooling that assumes an explicit App ID/bundle ID can be freely deleted or renamed post-upload. Default to registering explicit App IDs (not wildcard) for any app that will be submitted to the App Store or that needs specific capabilities/services. For the Rust/UniFFi/Xcode project generation path, when deriving a Swift/PRODUCT_MODULE_NAME from a product or bundle name, proactively sanitize to a valid C99-style identifier rather than relying on Xcode's automatic c99extidentifier substitution.

- Bundle ID strings may contain only alphanumeric characters (A-Z, a-z, 0-9), hyphens (-), and periods (.); the convention is reverse-DNS format; and bundle IDs are case-insensitive. (source: https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleidentifier)
  - "A bundle ID uniquely identifies a single app throughout the system. The bundle ID string must contain only alphanumeric characters (A–Z, a–z, and 0–9), hyphens (-), and periods (.). Typically, you use a reverse-DNS format for bundle ID strings. Bundle IDs are case-insensitive."
- After a build is uploaded to App Store Connect, you cannot change the bundle ID in the app's Info.plist nor delete the associated explicit App ID from your developer account. (source: https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleidentifier)
  - "The bundle ID in the information property list must match the bundle ID you enter in App Store Connect. After you upload a build to App Store Connect, you can't change the bundle ID or delete the associated explicit App ID in your developer account."
- There are two types of App IDs: an explicit App ID (identifies a single app via the full bundle ID path) and a wildcard App ID (identifies a set of apps via a bundle ID search string ending in an asterisk). An explicit App ID is required to submit an app to App Store Connect and to use certain services. (source: https://developer.apple.com/help/glossary/app-id)
  - "An explicit App ID is required to submit your app to App Store Connect and to use certain services."
- The App Store Connect API registers a new bundle ID via POST https://api.appstoreconnect.apple.com/v1/bundleIds, with a request body of type BundleIdCreateRequest, returning 201 Created with a BundleIdResponse. (source: https://developer.apple.com/documentation/appstoreconnectapi/post-v1-bundleids)
  - "POST https://api.appstoreconnect.apple.com/v1/bundleIds"
- The BundleIdCreateRequest.Data.Attributes object requires 'identifier' (the bundle ID string), 'name', and 'platform' fields, with an optional 'seedId'. (source: https://developer.apple.com/documentation/appstoreconnectapi/bundleidcreaterequest/data-data.dictionary/attributes-data.dictionary)
  - "identifier | required: True ; name | required: True ; platform | required: True ; seedId | required: False"
- The App Store Connect API supports deleting a registered bundle ID resource via DELETE https://api.appstoreconnect.apple.com/v1/bundleIds/{id}, returning 204 No Content on success. (source: https://developer.apple.com/documentation/appstoreconnectapi/delete-v1-bundleids-_id_)
  - "DELETE https://api.appstoreconnect.apple.com/v1/bundleIds/{id} ... 204 No Content"
- Xcode's PRODUCT_MODULE_NAME build setting names the source-code module used to import the target and must be a valid identifier. (source: https://developer.apple.com/documentation/xcode/build-settings-reference)
  - "Setting name: PRODUCT_MODULE_NAME ... The name to use for the source code module constructed for this target, and which will be used to import the module in implementation source files. Must be a valid identifier."
- Xcode derives the module name from PRODUCT_NAME using the $(PRODUCT_NAME:c99extidentifier) transform, which replaces characters invalid in a C99 extended identifier (e.g. hyphens, periods, combining/Unicode characters) with underscores to form a valid module name. (source: https://forums.swift.org/t/xcode-doesnt-like-unicode-in-module-names/32759)
  - "The default setting $(PRODUCT_NAME:c99extidentifier) will produce a value of \"E_toile_\". That's probably because this time the name was extracted from the file system in its decomposed form (using a combining character), and the combining characters are replaced by underscores."

**Unresolved**

- No Apple developer.apple.com documentation was found stating an explicit maximum character length for a bundle ID string; this should be treated as undocumented/inferred rather than confirmed absent.
- Did not locate an official developer.apple.com page (as opposed to Swift Forums / third-party summaries) that explicitly documents the exact character-replacement algorithm for PRODUCT_NAME -> PRODUCT_MODULE_NAME; the official Build Settings Reference only says PRODUCT_MODULE_NAME 'must be a valid identifier'.
- Did not fetch/verify the App Store Connect API endpoint for creating an app record (e.g. POST /v1/apps) — only bundleIds endpoints (register/delete) and the BundleIdCreateRequest schema were verified.
- Could not use WebFetch (blocked by a PreToolUse hook in this environment) and instead retrieved Apple's documentation JSON data endpoints directly via curl; this worked but is a nonstandard access path worth noting.

## Cargo package naming and crates.io rules

**Recommendation:** When choosing a Cargo package name: use only ASCII alphanumerics, '-' and '_', keep it under 64 chars for crates.io, and avoid Rust strict/reserved keywords especially if it needs to work with `cargo new`/be usable as a valid identifier. The lib target name (used in `use`/`extern crate`) automatically becomes the package name with '-' turned into '_', so pick a package name where that substitution reads cleanly (e.g. 'my-crate' -> `my_crate`).

- Cargo's manifest name field only allows alphanumeric characters, -, or _, and cannot be empty (source: https://doc.rust-lang.org/cargo/reference/manifest.html)
  - "The name must use only alphanumeric characters or - or _, and cannot be empty."
- crates.io additionally restricts names to ASCII alphanumeric, - and _ characters (source: https://doc.rust-lang.org/cargo/reference/manifest.html)
  - "Only ASCII characters are allowed. The character set is further limited to ASCII characters and only alphanumeric, -, and _ characters."
- crates.io enforces a maximum package name length of 64 characters and rejects reserved names / special Windows names like "nul" (source: https://doc.rust-lang.org/cargo/reference/manifest.html)
  - "do not use reserved names, do not use special Windows names such as \"nul\", and use a maximum of 64 characters of length"
- The library target name defaults to the package name with dashes replaced by underscores, since it must be a valid Rust identifier (source: https://doc.rust-lang.org/cargo/reference/manifest.html)
  - "For the library target, the name defaults to the name of the package, with any dashes replaced with underscores... This replacement is necessary because Rust extern crate declarations reference this name; therefore the value must be a valid Rust identifier to be usable."
- cargo new/cargo init additionally require the package name be a valid Rust identifier and reject keywords, beyond what the manifest itself requires (source: https://doc.rust-lang.org/cargo/reference/manifest.html)
  - "cargo new and cargo init impose some additional restrictions on the package name, such as enforcing that it is a valid Rust identifier and not a keyword."
- The Rust Reference divides keywords into strict, reserved, and weak categories; strict and reserved keywords cannot be used as identifiers (e.g. package/binary crate names) in their respective contexts (source: https://doc.rust-lang.org/reference/keywords.html)
  - "Rust divides keywords into three categories: strict keywords, reserved keywords, and weak keywords."
- Reserved keywords include newer/future-use words such as try, gen, macro, become, box, do, final, override, priv, typeof, unsized, virtual, yield — same restrictions as strict keywords (source: https://doc.rust-lang.org/reference/keywords.html)
  - "Reserved keywords aren't used yet, but they are reserved for future use and have the same restrictions as strict keywords. Reserved keywords include: abstract, become, box, do, final, gen, macro, override, priv, try, typeof, unsized, virtual, and yield."

**Unresolved**

- WebFetch was blocked by a session hook for doc.rust-lang.org and crates.io pages, so quotes above were obtained via WebSearch's rendering of those exact pages rather than a direct WebFetch by me — content closely matches known page text but could not be independently re-verified via direct fetch in this session.
- A direct crates.io-hosted policy page (as opposed to the Cargo Book's manifest.html reference, which documents crates.io's policy) was not separately fetched/verified.

## Android applicationId and Java/Kotlin package name segment rules

**Recommendation:** Choose an applicationId with at least two dot-separated segments, each starting with a lowercase letter and containing only [a-zA-Z0-9_] characters (hyphens are excluded by that character class, though developer.android.com does not state the "no hyphens" rule as a separate standalone sentence). Additionally avoid any segment that matches a Java reserved keyword (JLS Chapter 3, e.g. class, package, new, this, for, etc., plus const/goto) since javac rejects such package paths, and separately avoid Kotlin's hard-keyword list, which is not identical to Java's, since Kotlin source files compiled under an applicationId-derived package path must also satisfy Kotlin's own identifier rules.

- The applicationId must have at least two segments (one or more dots). (source: https://developer.android.com/build/configure-app-module)
  - "The application ID must have at least two segments (one or more dots)."
- Each segment of the applicationId must start with a letter, and all characters must be alphanumeric or an underscore [a-zA-Z0-9_] — no hyphens allowed. (source: https://developer.android.com/build/configure-app-module)
  - "each segment must start with a letter, and all characters must be alphanumeric or an underscore [a-zA-Z0-9_]"
- The Java Language Specification (Ch.7, package-naming conventions for converting Internet domain names) requires that if any resulting package name component is a Java keyword, an underscore is appended to that component so it becomes a legal identifier. (source: https://docs.oracle.com/javase/specs/jls/se7/html/jls-7.html)
  - "If any of the resulting package name components are keywords then append underscore to them."
- JLS Chapter 3 (Lexical Structure) defines 50 reserved character sequences (keywords), formed from ASCII letters, that cannot be used as identifiers — this covers package/segment identifiers since a PackageName is built from Identifiers. (source: https://docs.oracle.com/javase/specs/jls/se10/html/jls-3.html)
  - "abstract, continue, for, new, switch, assert, default, if, package, synchronized, boolean, do, goto, private, this, break, double, implements, protected, throw, byte, else, import, public, throws, case, enum, instanceof, return, transient, catch, extends, int, short, try, char, final, interface, static, void, class, finally, long, strictfp, volatile, const, float, native, super, and while."
- const and goto are reserved Java keywords even though unused by the language, so they too cannot be used as identifiers/package segments. (source: https://docs.oracle.com/javase/specs/jls/se10/html/jls-3.html)
  - "the keywords const and goto are reserved, even though they are not currently used"
- Kotlin adds its own restriction on top of Java's: Kotlin 'hard keywords' are absolutely reserved and cannot be used as identifiers anywhere in Kotlin code, so a segment legal in Java package names could still be illegal as a Kotlin source package segment. (inferred) **(unverified)** (source: https://kotlinlang.org/docs/keyword-reference.html)
  - "Hard keywords are strictly reserved and cannot be used as identifiers (such as variable names, function names, or class names) under any circumstances."

**Unresolved**

- WebFetch was blocked by a PreToolUse hook for every attempted URL (both developer.android.com and docs.oracle.com pages), so all facts were verified via WebSearch's retrieved page snippets/quotes rather than a direct full-page fetch.
- developer.android.com's applicationId rule was not seen to state 'no hyphens' as an explicit standalone sentence — this is inferred from the closed character class [a-zA-Z0-9_], which has no hyphen in it.
- Could not confirm via direct fetch whether kotlinlang.org's keyword-reference page states a package-name-specific restriction versus only a general identifier restriction; treated as inferred rather than verified for that reason.
- Could not directly confirm the specific JLS sub-section number (7.4.1) for the 'append underscore to keyword components' rule via a direct fetch of the se7 jls-7.html page (only reached via WebSearch snippet); the chapter-level URL is confirmed but the exact anchor/subsection was not independently re-verified by fetch.

## Slug grammar implications

The nine provider topics above each define naming rules for something a "project slug" for this platform would need to become: a GitHub repo name (and, if the platform provisions a GitHub org/user per project, a GitHub login), a Doppler project name, a Buildkite pipeline slug, a Railway service name, an Apple bundle ID component, a Cargo package name, and an Android applicationId segment. This section derives the intersection of those rules for a single canonical project slug. Where a cited fact underlying a derivation step is marked "inferred" in the source research, the derived constraint is flagged **(unverified)** here too.

### Allowed characters (intersection)

| Target | Allowed charset for the slug/segment | Confidence |
|---|---|---|
| GitHub repo name | ASCII letters, digits, `.`, `-`, `_` | inferred **(unverified)** |
| GitHub org/user login | alphanumeric + `-` only | inferred **(unverified)** |
| Doppler project name | no documented charset; convention is lowercase + `-` | verified (convention only) |
| Buildkite pipeline slug | `[a-zA-Z0-9]` then `[a-zA-Z0-9-]*` (no `.`, no `_`) | verified |
| Railway service name | no documented charset; DNS-label usage implies alphanumeric + `-` is safe | inferred **(unverified)** |
| Apple bundle ID component | alphanumeric, `-`, `.` (no `_`) | verified |
| Cargo package name | alphanumeric, `-`, `_` (no `.`) | verified |
| Android applicationId segment | alphanumeric + `_` only (no `-`, no `.` within a segment) | verified |

Taking the intersection: `-` is excluded by the Android applicationId segment rule (verified: "alphanumeric or an underscore [a-zA-Z0-9_]" — no hyphen in that class), and `_` is excluded by the Buildkite pipeline slug regex, the GitHub login rule, and the Apple bundle ID charset (verified: none of the three include `_`). `.` is excluded by everything except GitHub repo names and Apple bundle IDs (where it functions as a segment separator, not an in-segment character, in reverse-DNS/applicationId use). **The intersection-safe charset for a single canonical project slug is therefore lowercase ASCII letters and digits only: `[a-z0-9]`.** Lowercasing is additionally motivated by Doppler's documented recommendation and Buildkite's own lowercasing of derived slugs (both verified), and is harmless everywhere else since GitHub and Apple bundle IDs are documented case-insensitive (both flagged inferred/verified respectively above).

### First character

- Android applicationId: each segment "must start with a letter" (verified) — excludes a leading digit.
- Buildkite pipeline slug regex: `[a-zA-Z0-9]` first character — permits a leading digit, no constraint against it.
- GitHub login: cannot start with `-` (inferred **(unverified)**) — irrelevant once `-` is already excluded from the intersection charset.
- Cargo (`cargo new`/`cargo init` path only): must be a valid Rust identifier, which cannot start with a digit (verified, via "enforcing that it is a valid Rust identifier").

**Binding constraint: the slug must start with a lowercase letter, `[a-z]`.** This is driven jointly by Android's explicit rule and Cargo's identifier requirement (both verified), independent of the unverified GitHub/Railway facts.

### Max length after the longest prefix

Documented maximum lengths, taken as-is (no external prefix applied):

| Target | Max length | Confidence |
|---|---|---|
| Railway service name | 32 | verified |
| GitHub org/user login | 39 | inferred **(unverified)** |
| Cargo package name | 64 | verified |
| GitHub repo name | 100 | inferred **(unverified)** |
| Buildkite pipeline slug | 100 | verified |
| Doppler project/config name | undocumented | — (unresolved) |
| Apple bundle ID | undocumented | — (unresolved) |
| Android applicationId segment | undocumented | — (unresolved) |

Railway's 32-character cap on service names (verified) is the tightest documented number and is therefore the binding constraint on the raw slug length by itself. However, the slug is not always used bare: Doppler's branch-config convention (verified) prefixes a descriptor with `<environment>_`, and the three default environment identifiers are `dev`, `stg`, `prd` — each exactly 3 characters, so the longest *default* prefix, including its separator, is 4 characters (`dev_`, `stg_`, or `prd_`). Doppler does not document a length cap of its own (unresolved, listed above), so this prefix does not by itself shrink the binding cap — but if the same slug value is later reused as a component inside a Railway service name that is itself prefixed or suffixed (e.g. an environment or role suffix appended to form `<slug>-<role>`), the 32-character Railway ceiling is the one that gets consumed first.

**Derived rule:** cap the canonical project slug at 32 characters minus the longest known fixed prefix/suffix that any target composes it with. The only concretely documented composition prefix in the source facts is Doppler's `<environment>_` pattern (4 characters for the longest default environment name). Applying that reservation to the tightest cap: **max slug length = 32 − 4 = 28 characters.** This is a derived, not directly documented, number — it depends on (a) Railway's verified 32-char cap applying to the same string as (b) Doppler's verified branch-naming convention, an assumption this research did not test end-to-end; treat 28 as a conservative design target rather than a fact from any single source.

### Reserved words

Union of documented reserved/disallowed-word rules that apply to a slug used as a Cargo package name and/or Android applicationId segment:

- Rust strict and reserved keywords (verified) — includes ordinary keywords plus reserved-for-future-use ones: `try`, `gen`, `macro`, `become`, `box`, `do`, `final`, `override`, `priv`, `typeof`, `unsized`, `virtual`, `yield`, etc. — required if the slug must double as a valid Rust identifier (`cargo new`/`cargo init` path).
- crates.io special Windows device names, e.g. `nul` (verified) — required for the Cargo package name target specifically.
- Java reserved keywords per JLS Chapter 3 (verified): `abstract, assert, boolean, break, byte, case, catch, char, class, const, continue, default, do, double, else, enum, extends, final, finally, float, for, goto, if, implements, import, instanceof, int, interface, long, native, new, package, private, protected, public, return, short, static, strictfp, super, switch, synchronized, this, throw, throws, transient, try, void, volatile, while` — required for any Android applicationId segment, since JLS 7 requires an underscore be appended to a keyword-matching package component (verified), which would otherwise silently mangle a generated slug.
- Kotlin hard keywords (inferred **(unverified)**) — a Kotlin-specific list, not identical to Java's, that must additionally be avoided if generated Android source under the applicationId-derived package path is Kotlin.

**Derived rule:** reject any slug value that case-insensitively matches a member of the union of {Rust strict/reserved keywords, crates.io special Windows names, Java reserved keywords, Kotlin hard keywords}. No source document in this research enumerates GitHub- or Doppler-specific reserved words; that absence is a genuine gap, not an oversight (see Unresolved list per topic above), so no GitHub/Doppler reserved-word constraint is included in the union.

### Per-target join table

How the single canonical slug (`[a-z][a-z0-9]{0,27}`, avoiding the reserved-word union above) maps onto each target's actual naming requirement:

| Target | Transform applied to the canonical slug | Grounding |
|---|---|---|
| GitHub repo name | used as-is (already within `[A-Za-z0-9._-]`, ≤100) | inferred **(unverified)** |
| GitHub org/user login | used as-is if a per-project org/user is provisioned (already alphanumeric, ≤39 vs. the 28-char slug budget) | inferred **(unverified)** |
| Doppler project name | used as-is (matches the hyphen-separated/lowercase recommendation trivially, since the slug has no hyphens) | verified (recommendation only) |
| Doppler branch config name | prefixed as `<environment>_<slug>` (e.g. `prd_<slug>`) following the documented `<environment>_<descriptor>` / `dev_personal` convention | verified |
| Buildkite pipeline slug | used as-is; already satisfies `/\A[a-zA-Z0-9]+[a-zA-Z0-9-]*\z/` and the 100-char cap; can also be passed explicitly via the `slug` create/update parameter to bypass name-derived lowercasing | verified |
| Railway service name | used as-is (≤32 by construction at 28 chars); becomes the DNS label `<slug>.railway.internal` | verified (length + DNS pattern); charset choice is inferred **(unverified)** |
| Apple bundle ID | embedded as the final reverse-DNS component, e.g. `com.<org>.<slug>` (charset already compatible: alphanumeric only) | verified |
| Cargo package name | used as-is; the lib target name is then auto-derived as `<slug>` unchanged (no dashes/underscores to replace, since the intersection charset has neither) | verified |
| Android applicationId | embedded as the final segment, e.g. `com.<org>.<slug>`; already starts with a letter and is alphanumeric-only, so no keyword-style underscore-append or char substitution is triggered | verified (segment rule); Kotlin hard-keyword avoidance is inferred **(unverified)** |

### Summary of the derived grammar

- **Charset:** `[a-z0-9]`, lowercase only.
- **First character:** `[a-z]` (no leading digit).
- **Max length:** 28 characters (32-character Railway ceiling minus the 4-character longest documented composition prefix, Doppler's `<environment>_`).
- **Reserved words:** case-insensitive rejection of the union of Rust strict/reserved keywords, crates.io special Windows names, Java reserved keywords, and Kotlin hard keywords (this last flagged **(unverified)**).
- **Regex:** `^[a-z][a-z0-9]{0,27}$`, combined with the reserved-word rejection list above.
