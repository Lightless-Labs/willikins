# Milestone 3a dependency and provider research: Buildkite

**Created:** 2026-09-16
**Plan:** `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`
**Previous:** `docs/research/2026-09-12-m2-dependencies.md`

Three research passes run in parallel on 2026-09-16, in the same format as the milestone 1 and
milestone 2 notes: facts each carrying a source URL and a verbatim quote, then an unresolved
list. A fact taken from a search snippet or a rendered page's paraphrase rather than a fetched
primary source is marked **(unverified)**. Anything that could not be settled verbatim is
repeated in "Verify with a browser before relying on them" at the end, and in the plan's own
verify list.

Two method notes worth keeping, both the Buildkite equivalents of the Doppler `llms.txt` note
in the milestone 2 research.

- `buildkite.com/docs` publishes an `llms.txt` index at <https://buildkite.com/docs/llms.txt>
  (118 KB, every page with a one-line description), and **every documentation page has a raw
  markdown twin at the same path plus `.md`**. `https://buildkite.com/docs/apis/rest-api/clusters.md`
  returns `text/markdown` in 12 KB where the rendered HTML is 383 KB. Prefer the `.md` twin for
  every future Buildkite fact.
- The docs' own source repository is `buildkite/docs`, and its page sources are fetchable at
  `https://raw.githubusercontent.com/buildkite/docs/main/pages/<path>.md`. The default branch was
  at commit `8d9bf35667b261dcc13db3e822a1e78c0b73bc5e` when read (2026-09-16, from an
  unauthenticated `GET` of `api.github.com/repos/buildkite/docs/commits/main`). The negative
  findings in section 3 rest on a recursive tree listing of that commit (2,550 paths) plus greps
  over ten downloaded pages.

**Unlike Doppler, Buildkite publishes no machine-readable schema for anything this milestone
needs.** The only OpenAPI description is for Test Engine's `/v2/analytics` endpoints. The prose
docs are the primary source, and everything they do not state is a verify item rather than
frozen code. This is the single biggest difference from the milestone 2 research, and it is why
section 4's error mapping is built to fail closed.

**No credentialed call was made by any of the three readers.** Every request was an
unauthenticated `GET` against `buildkite.com/docs`, `raw.githubusercontent.com`, or
`api.github.com`. No call reached `api.buildkite.com`. `~/.config/willikins/sandbox.env` was
never opened, sourced, listed, or referenced, and no `WILLIKINS_*` variable was read. No `gh`
command was run.

Sections:

1. Pipelines: create, read, update, delete, slug derivation, configuration
2. Clusters, queues, and agent tokens
3. Authentication, scopes, errors, rate limits, pagination
4. Conclusions for willikins (these are conclusions, not quotes)
5. Verify with a browser before relying on them


## 1. Pipelines

Every endpoint below is under `https://api.buildkite.com/v2/organizations/{org.slug}/pipelines`.

| Method | Path | Scope | Success |
| --- | --- | --- | --- |
| `GET` | `/` | `read_pipelines` | `200 OK`, `Link`-paginated |
| `GET` | `/{slug}` | `read_pipelines` | `200 OK` |
| `POST` | `/` | `write_pipelines` | `201 Created` |
| `PATCH` | `/{slug}` | `write_pipelines` | `200 OK` |
| `DELETE` | `/{slug}` | `write_pipelines` | `204 No Content` |
| `POST` | `/{slug}/archive` | `write_pipelines` | `200 OK` |
| `POST` | `/{slug}/unarchive` | `write_pipelines` | `200 OK` |

### Create

- Creating a YAML pipeline is `POST /v2/organizations/{org.slug}/pipelines` with
  `Content-Type: application/json` and a bearer token. The documented **required** request body
  properties are exactly four: `name`, `cluster_id`, `repository`, `configuration`. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#create-a-yaml-pipeline>)
  - "curl -H \"Authorization: Bearer $TOKEN\" \\\n  -X POST \"https://api.buildkite.com/v2/organizations/{org.slug}/pipelines\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\n      \"name\": \"My Pipeline X\",\n      \"cluster_id\": \"xxx\",\n      \"repository\": \"git@github.com:acme-inc/my-pipeline.git\",\n      \"configuration\": \"env:\\n \\\"FOO\\\": \\\"bar\\\"\\nsteps:\\n - command: \\\"script/release.sh\\\"\\n   \\\"name\\\": \\\"Build :package:\\\"\"\n    }'"
- The required-properties table names `cluster_id` and states that a missing or blank
  `configuration` is a `422`. (source: same page)
  - "Required [request body properties](/docs/api#request-body-properties):\n\n| `name` | The name of the pipeline. _Example:_ `\"New Pipeline\"` |\n| --- | --- |\n| `cluster_id` | The ID value of the cluster the pipeline will be associated with. _Example:_ `\"018e5a22-d14c-7085-bb28-db0f83f43a1c\"` |\n| `repository` | The repository URL. _Example:_ `\"git@github.com:acme-inc/my-pipeline.git\"` |\n| `configuration` | The YAML pipeline that consists of the build pipeline steps. Must be non-empty. A missing or blank value returns a `422` error."
- Documented **optional** properties: `allow_rebuilds`, `branch_configuration`,
  `cancel_running_branch_builds`, `cancel_running_branch_builds_filter`, `clone_mirror_url`,
  `color`, `default_branch`, `default_command_step_timeout`, `description`, `emoji`,
  `maximum_command_step_timeout`, `pipeline_template_uuid`, `provider_settings`,
  `skip_queued_branch_builds`, `skip_queued_branch_builds_filter`, `slug`, `tags`, `teams`,
  `visibility` (default `"private"`). (source: same page)
- The `201` response carries the whole pipeline object. The four fields willikins needs —
  `id`, `slug`, `web_url`, `repository` — are all in it, and `web_url` is
  `https://buildkite.com/{org}/{slug}`. (source: same page)
  - "{\n  \"id\": \"ad93b461-96ab-4a1e-9281-260ead506a0e\",\n  \"graphql_id\": \"UGlwZWxpbmUtLS1hZDkzYjQ2MS05NmFiLTRhMWUtOTI4MS0yNjBlYWQ1MDZhMGU=\",\n  \"url\": \"https://api.buildkite.com/v2/organizations/acme-inc/pipelines/my-pipeline-x\",\n  \"web_url\": \"https://buildkite.com/acme-inc/my-pipeline-x\",\n  \"name\": \"My Pipeline X\",\n  \"description\": null,\n  \"slug\": \"my-pipeline-x\",\n  \"repository\": \"git@github.com:acme-inc/my-pipeline.git\",\n  \"cluster_id\": null,"
- The same response example is **internally inconsistent**: the request passed
  `"cluster_id": "xxx"` and the response shows `"cluster_id": null`. Treat the required-properties
  table as authoritative and the example as stale. (source: same page; this bullet is an
  observation about the two quotes above, not a quote)
- A duplicate name "results in an error", with **no status code and no body given anywhere on
  the page**. There is no `409` anywhere in it. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#deriving-a-pipeline-slug-from-the-pipelines-name>)
  - "Any attempt to create a new pipeline with a name that matches an existing pipeline's name, results in an error."

### Read

- A pipeline is fetchable by slug directly; the path segment is the slug, not the UUID. Scope
  `read_pipelines`, `200 OK`. **No error table is documented for this endpoint**, so the
  not-found status and body are not established by the docs. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#get-a-pipeline>)
  - "curl -H \"Authorization: Bearer $TOKEN\" \\\n  -X GET \"https://api.buildkite.com/v2/organizations/{org.slug}/pipelines/{slug}\""
- The list endpoint's two filters, `name` and `repository`, are **partial-match and
  case-insensitive**, so neither is safe as an existence check. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#list-pipelines>)
  - "| `name` | Filters the results by the pipeline name. Supports partial matches and is case insensitive. _Example:_ `?name=agent` |\n| --- | --- |\n| `repository` | Filters the results by the repository URL of the source repository. Supports partial matches and is case insensitive. _Example:_ `?repository=agent` |"

### Update and delete

- `PATCH .../pipelines/{slug}` "Updates one or more properties", so omitted top-level properties
  are left alone — except `configuration`, where a partial update is a trap: setting it replaces
  the whole YAML document. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#update-a-pipeline>)
  - "| `configuration` | The YAML pipeline that consists of the build pipeline steps. Setting this attribute replaces the entire configuration for the pipeline, so include all existing steps and settings, not just the ones you want to change."
- On a YAML pipeline the same endpoint ignores `env` and rejects `steps` with a `422` when
  `configuration` is absent. (source: same page)
  - "Two attributes below behave differently for YAML pipelines. This endpoint ignores `env`. To set environment variables, get the current `configuration` for the pipeline, add or update the top-level `env` key, then PATCH the complete `configuration` back. This endpoint only rejects `steps` with a `422` error when `configuration` is absent from the request. If both are present, it ignores `steps` and applies `configuration` instead."
- Changing `name` without also sending `slug` regenerates the slug, which moves the pipeline's
  URL. (source: the slug-derivation quote below, plus the `PATCH` table's `slug` row on the same
  page)
- The only REST-immutable field the docs name is `teams`: the `PATCH` table lists it, and the
  endpoint carries an explicit warning sending team changes to GraphQL. (source: same page)
  - "To update a pipeline's teams, please use the <a href=\"/docs/apis/graphql-api\">GraphQL API</a>."
- `DELETE .../pipelines/{slug}`, scope `write_pipelines`, `204 No Content` with no body; **no
  error responses are documented**, so the behaviour on an unknown slug is not established.
  (source: <https://buildkite.com/docs/apis/rest-api/pipelines#delete-a-pipeline>)
  - "curl -H \"Authorization: Bearer $TOKEN\" \\\n  -X DELETE \"https://api.buildkite.com/v2/organizations/{org.slug}/pipelines/{slug}\"\n\nRequired scope: `write_pipelines`\n\nSuccess response: `204 No Content`"
- A non-destructive alternative to delete exists: `POST .../pipelines/{slug}/archive`, `200 OK`,
  with documented errors `403 {"message": "Forbidden"}` and
  `422 {"message": "Pipeline could not be archived."}`, and a matching `/unarchive`. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#archive-a-pipeline>)

### Slug derivation

- The slug is derived from the name unless the optional `slug` parameter is supplied; spaces
  (including runs) collapse to one hyphen, uppercase folds to lowercase, the maximum length is
  100 characters, and the validating regex is `/\A[a-zA-Z0-9]+[a-zA-Z0-9\-]*\z/` — it must start
  with an alphanumeric and may then contain alphanumerics and hyphens only. No underscore, no
  dot, no leading hyphen. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#deriving-a-pipeline-slug-from-the-pipelines-name>)
  - "This derivation process involves converting all space characters (including consecutive ones) in the pipeline's name to single hyphen `-` characters, and all uppercase characters to their lowercase counterparts. Therefore, pipeline names of either `Hello there friend` or `Hello    There Friend` are converted to the slug `hello-there-friend`.\n\nThe maximum permitted length for a pipeline slug is 100 characters.\n\n> 📘\n> The following regular expression is used to derive and convert the pipeline name to its slug:\n> `/\\A[a-zA-Z0-9]+[a-zA-Z0-9\\-]*\\z/`"
- The update endpoint restates the rule from the other side: "It can only contain alphanumeric
  characters or dashes and cannot begin with a dash." (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#update-a-pipeline>)

### The configuration string

- `configuration` is a single JSON string whose content Buildkite parses as YAML: quotes escaped,
  line breaks written as `\n`. The documented minimal configuration — the one that keeps the real
  pipeline definition in the repository — is exactly one step invoking the agent's upload
  command. (source: <https://buildkite.com/docs/apis/rest-api/pipelines#create-a-yaml-pipeline>)
  - "When setting pipeline configuration using the API, you must pass in a string that Buildkite parses as valid YAML, escaping quotes and line breaks.\nTo avoid writing an entire YAML file in a single string, you can place a <code>pipeline.yml</code> file in a <code>.buildkite</code> directory at the root of your repo, and use the <code>pipeline upload</code> command in your configuration to tell Buildkite where to find it. This means you only need the following:\n<code>\"configuration\": \"steps:\\n - command: \\\"buildkite-agent pipeline upload\\\"\"</code>"

### Provider settings

- `provider_settings` is accepted on create and update; for GitHub the accepted keys are
  `build_branches`, `build_pull_requests`, `build_tags`, `ignore_default_branch_pull_requests`,
  `publish_commit_status`, `publish_commit_status_per_step`, `pull_request_branch_filter_enabled`,
  `pull_request_branch_filter_configuration`,
  `skip_pull_request_builds_for_existing_commits`, plus the all-provider `filter_enabled` and
  `filter_condition`. The create response's defaults already include `build_branches: true`,
  `build_pull_requests: true` and `publish_commit_status: true`, so omitting `provider_settings`
  yields push- and PR-triggered builds with commit statuses. (source:
  <https://buildkite.com/docs/apis/rest-api/pipelines#provider-settings-properties>)
  - "The [Create a YAML pipeline](#create-a-yaml-pipeline) and [Update pipeline](#update-a-pipeline) endpoints accept a `provider_settings` property, which allows you to configure how the pipeline is triggered based on source code provider events."
- How Buildkite chooses `provider.id` from the repository URL is **never documented**. That
  `git@github.com:org/repo.git` yields `"provider": {"id": "github", ...}` is an inference from
  every response example, not a stated rule. **(unverified)**
- `provider.webhook_url` is populated only when the token has `write_pipelines` and the user can
  edit the pipeline; it is a credential-bearing delivery URL whose path segment is the shared
  secret, and must never be surfaced, logged, or journalled. (conclusion drawn from the create
  response example on the create page; the page states the population rule, and the
  credential-bearing property is this project's own assessment — see section 4)


## 2. Clusters, queues, and agent tokens

- Clusters are listed at `GET /v2/organizations/{org.slug}/clusters`, scope `read_clusters`,
  `200 OK`, paginated. A cluster is identified by an opaque UUID `id` (plus `graphql_id`). There
  is no slug; the only human-facing handle is the free-form `name`. Get-one is by `id`, never by
  name, so a name must be resolved client-side by scanning the list. (source:
  <https://buildkite.com/docs/apis/rest-api/clusters.md>)
  - "| `id` | ID of the cluster |\n| `graphql_id` | [GraphQL ID](/docs/apis/graphql-api#graphql-ids) of the cluster |\n| `default_queue_id` | ID of the cluster's default queue. Agents that connect to the cluster without specifying a queue will accept jobs from this queue. |\n| `name` | Name of the cluster |\n ... \n### List clusters\n\nReturns a [paginated list](/docs/rest-api#pagination) of an organization's clusters.\n\n```bash\ncurl -H \"Authorization: Bearer $TOKEN\" \\\n  -X GET \"https://api.buildkite.com/v2/organizations/{org.slug}/clusters\"\n```\n ... \nRequired scope: `read_clusters`\n\nSuccess response: `200 OK`"
- **A cluster name is not a stable, unique natural key.** The docs never state that names are
  unique within an organization, there is no name-keyed lookup and no `?name=` filter, and the
  default cluster is only *initially* named "Default cluster" — the name is mutable. (source:
  <https://buildkite.com/docs/pipelines/security/clusters/manage.md>)
  - "When a new Buildkite organization is created, a single default cluster (initially named **Default cluster**) is also created."
- Pipelines must be assigned to a cluster, and that assignment is the isolation boundary. The
  same page records that one default cluster is the normal arrangement for a small or medium
  organization. (source: <https://buildkite.com/docs/pipelines/security/clusters.md>)
  - "- Pipelines must be assigned to a cluster, ensuring their builds run only on the agents connected to this cluster. These pipelines can also trigger builds only on other pipelines in the same cluster.\n ... \n### How should I structure my clusters\n\nIn a small to medium organization, a single default cluster will often suffice. There is no need to create extra clusters."
- Omitting `cluster_id` is answered by an organization-level **default cluster** setting, not by
  "the org has only one cluster". (source:
  <https://buildkite.com/docs/pipelines/security/clusters/manage.md>)
  - "## Set a default cluster for new pipelines\n\nA [_Buildkite organization administrator_](/docs/pipelines/security/permissions#manage-teams-and-permissions-organization-level-permissions) can nominate one cluster as the _default cluster_ for new pipelines in the organization. When a default is set:\n\n- New pipelines created without an explicit cluster assignment (using the Buildkite interface, REST API, or GraphQL API) are automatically assigned to the default cluster.\n- Organization members can create pipelines in the default cluster without needing an explicit per-cluster pipeline creation permission. The organization administrator's choice of default acts as the permission grant for that cluster."
- That default is **not observable without an org-admin credential**: `default_cluster_id` lives
  on `GET /v2/organizations/{org.slug}/pipeline-settings`, which needs
  `read_organization_settings` *and* the `change_organization` permission. (source:
  <https://buildkite.com/docs/apis/rest-api/organizations/pipeline-settings.md>)
  - "The pipeline settings API endpoint lets organization administrators read and update organization-level pipeline settings. These settings correspond to the options available in the Buildkite **Pipeline Settings** page for your organization.\n\nBoth read and write operations require organization administrator privileges (the `change_organization` permission).\n ... \n| `default_cluster_id` | The UUID of the default cluster for new pipelines, or `null` if no default cluster is set. |"
- **No queue field exists on the pipeline object or its create body.** A queue is named in the
  pipeline YAML, at the root level or per step, so through REST it reaches Buildkite only inside
  the `configuration` string — and under the documented upload bootstrap, inside the repository's
  own `.buildkite/pipeline.yml`. (source: <https://buildkite.com/docs/agent/queues.md>)
  - "Target specific queues (either [self-hosted](/docs/agent/queues/managing#create-a-self-hosted-queue) or [Buildkite hosted](/docs/agent/queues/managing#create-a-buildkite-hosted-queue) ones) using the `agents` attribute on your pipeline steps, or at the root level for the entire pipeline.\n\nFor example, the following pipeline would run on the `priority` queue as determined by the root level `agents` attribute (and ignores the agents running the `default` queue)."
- A queue's own natural key is the pair (cluster id, `key`); create is
  `POST /v2/organizations/{org.slug}/clusters/{cluster.id}/queues` with `key` required, scope
  `write_clusters`, `201 Created`. (source:
  <https://buildkite.com/docs/apis/rest-api/clusters/queues.md>)
  - "## Create a self-hosted queue\n\n```bash\ncurl -H \"Authorization: Bearer $TOKEN\" \\\n  -X POST \"https://api.buildkite.com/v2/organizations/{org.slug}/clusters/{cluster.id}/queues\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{ \"key\": \"default\", \"description\": \"The default queue for this cluster\" }'\n```\n ... \nRequired [request body properties](/docs/api#request-body-properties):\n\n| `key` | Key for the queue. _Example:_ `\"default\"` |\n ... \nRequired scope: `write_clusters`\n\nSuccess response: `201 Created`"
- A queue key allows letters, numbers, hyphens and underscores — wider than `ProjectSlug`'s
  kebab intersection, so a queue key is not slug-derived. (source:
  <https://buildkite.com/docs/agent/queues/managing.md>)
  - "1. In the **Create a key** field, enter a unique _key_ for the queue, which can only contain letters, numbers, hyphens, and underscores, as valid characters."
- **An agent token is scoped to a cluster, not to a pipeline or a queue**, and one token already
  registers agents against every queue in that cluster. A new pipeline in an existing cluster is
  therefore reachable by the agents already running, with no new credential. (source:
  <https://buildkite.com/docs/agent/self-hosted/tokens.md>)
  - "## Scope of access\n\nAn agent token is specific to the cluster it was associated when created (within a Buildkite organization), and can be used to register an agent with any [queue](/docs/agent/queues) defined in that cluster. Agent tokens can not be shared between different clusters within an organization, or between different organizations."
- An agent token's value is returned **only** in its create response. (source:
  <https://buildkite.com/docs/apis/rest-api/clusters/agent-tokens.md>)
  - "> 📘 Token visibility\n> To ensure the security of tokens, the value is only included in the response for the request to create the token. Subsequent responses do not contain the token value."
- A cluster created through the API gets **no** default queue, unlike one created through the
  interface. (source: <https://buildkite.com/docs/pipelines/security/clusters/manage.md>)
  - "> 📘 A default queue is not automatically created\n> Unlike creating a new cluster through the [Buildkite interface](#create-a-cluster-using-the-buildkite-interface), a default queue is not automatically created using this API call. To create a new/default queue for any new cluster created through an API call, you need to manually [create a new queue](/docs/agent/queues/managing#create-a-self-hosted-queue)."
- Resolving a cluster **by name into an id** is Buildkite's own infrastructure-as-code shape:
  their Terraform provider exposes a `buildkite_cluster` data source keyed on `name`, and the
  pipeline resource consumes `data.buildkite_cluster.<x>.id`. (source:
  <https://buildkite.com/docs/platform/terraform-provider/getting-started-with-managing-pipelines.md>)
  - "# Data source for existing cluster (name) to assign pipelines to\ndata \"buildkite_cluster\" \"default\" {\n  name = \"Default cluster\"\n}"


## 3. Authentication, scopes, errors, rate limits, pagination

- Authentication is a bearer token in the `Authorization` header; basic auth is not supported.
  Base URL `https://api.buildkite.com`, version path `/v2`, HTTPS only, JSON only. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api.md>)
  - "To authenticate an API call using an access token, set the <code>Authorization</code> HTTP header to the word <code>Bearer</code>, followed by a space, followed by the access token. For example:\n\n```bash\ncurl -H \"Authorization: Bearer $TOKEN\" \\\n  -X GET \"https://api.buildkite.com/v2/user\"\n```\n\nAPI access using basic HTTP authentication is not supported."
- A public-key/JWT credential type exists but is a gated preview and irrelevant here; the header
  shape is identical either way. (source: same page)
  - "> 📘 This feature is currently available in preview and must be enabled by Buildkite for your organization. The **Public Key** credential type only appears after the feature is enabled. ... To authenticate API calls, sign a JWT with your private key using the `RS256` algorithm."
- **There is no separate delete grant for pipelines.** The scope table's columns are
  Read/Write/Delete and the Pipelines row is read `true`, write `true`, delete `false`:
  `write_pipelines` alone covers create, update and delete. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/managing_api_tokens.md>)
  - "{\n        name: \"Pipelines\",\n        key: \"read_pipelines, write_pipelines\",\n        description: \"List and retrieve details of pipelines—create, update, and delete pipelines.\",\n        read: true, write: true, delete: false\n      },"
- A token's own scopes, description, `created_at` and `expires_at` are readable with **no scope
  at all**, which makes it a safe credential probe. `DELETE` on the same path revokes the token —
  willikins must never call it. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api/access_token.md>)
  - "Returns details about the API access token that was used to authenticate the request.\n\n```bash\ncurl -H \"Authorization: Bearer $TOKEN\" \\\n  -X GET \"https://api.buildkite.com/v2/access-token\"\n```\n\n```json\n{\n  \"uuid\": \"b63254c0-3271-4a98-8270-7cfbd6c2f14e\",\n  \"scopes\": [\"read_build\"],\n  \"description\": \"Development Token\",\n  \"created_at\": \"2025-07-16 06:07:42 UTC\",\n  \"expires_at\": \"2025-07-23 06:07:42 UTC\",\n...\nRequired scope: none\n\nSuccess response: `200 OK`"
- That example spells `"read_build"` (singular) while the scope table spells the CI/CD scopes
  plural (`read_builds`, `read_pipelines`). The docs cannot settle the exact returned string, so
  any code asserting `scopes.contains("write_pipelines")` rests on an unverified spelling.
  (source: the two quotes above, compared)
- **The 422 body is not uniform across the API.** Three shapes are documented: pipelines
  create/update answer `{ "message": "Validation Failed", "errors": [ ... ] }`; clusters create
  answers `{ "message": "Validation failed: Reason for failure" }` with no `errors` array at all;
  archive/unarchive/webhook answer a bare `{ "message": "..." }`. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api/clusters.md>)
  - "Required scope: `write_clusters`\n\nSuccess response: `201 Created`\n\nError responses:\n\n<table class=\"responsive-table\">\n<tbody>\n  <tr>\n    <th><code>422 Unprocessable Entity</code></th>\n    <td><code>{ \"message\": \"Validation failed: Reason for failure\" }</code></td>\n  </tr>\n</tbody>\n</table>"
- Inside the pipelines `errors[]` array, the one worked example has two string fields, `field`
  and `code` — and `code` holds a **human sentence, not a machine code**. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api/pipelines.md>)
  - "    <th><code>422 Unprocessable Entity</code></th>\n    <td><code>{ \"message\": \"Validation Failed\", \"errors\": [ ... ] }</code>. When <code>configuration</code> is missing or blank, the error includes <code>{ \"field\": \"configuration\", \"code\": \"Step configuration is missing, expected `steps: { yaml: \\\"...\\\" }`\" }</code>.</td>"
- The only non-validation error the pipelines endpoints document is `403 Forbidden` with the body
  `{ "message": "Forbidden" }`. (source: same page)
  - "  <tr>\n    <th><code>403 Forbidden</code></th>\n    <td><code>{ \"message\": \"Forbidden\" }</code></td>\n  </tr>"
- **`401` and `404` are documented nowhere in the Buildkite docs tree.** A recursive listing of
  the docs repository at the commit above (2,550 paths) contains no status-code or errors page
  under `pages/apis/`, and the REST overview has no Errors section — its headings run Schema,
  Endpoints, Query string parameters, Request body properties, Authentication, Pagination, Rate
  limiting, CORS headers, OpenAPI specification, Migrating from v1 to v2, Clients. Grepping seven
  downloaded endpoint pages for "401", "Unauthorized", "404" and "Not Found" returns zero hits.
  (source: <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api.md>)
  - "## Query string parameters\n...\n## Request body properties\n...\n## Authentication\n### Public key\n## Pagination\n## Rate limiting\n## CORS headers\n## OpenAPI specification\n## Migrating from v1 to v2\n## Clients"
- The only machine-readable description Buildkite publishes covers Test Engine's `/v2/analytics`
  endpoints. Nothing covers Pipelines. (source: same page)
  - "The Buildkite Test Engine REST API publishes a machine-readable [OpenAPI](https://www.openapis.org/) specification describing the `/v2/analytics` endpoints intended for general consumption."
- **Buildkite does not send `Retry-After`.** The string appears nowhere in the rate-limit page or
  the REST overview. Rate limits are reported in two independent header families, and both
  `*-Reset` values are relative seconds, not epoch timestamps. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api/rate_limits.md>)
  - "Organization-level headers:\n\n- `RateLimit-Scope`: The scope of the organization-level rate limit. Set to `rest`.\n- `RateLimit-Remaining`: The remaining requests within the current organization time window.\n- `RateLimit-Limit`: The organization rate limit.\n- `RateLimit-Reset`: The number of seconds remaining until the organization time window resets.\n\nPer-user headers:\n\n- `RateLimit-User-Scope`: The scope of the per-user rate limit. Set to `rest_user`.\n- `RateLimit-User-Remaining`: The remaining requests for the authenticated user within the current time window.\n- `RateLimit-User-Limit`: The per-user rate limit.\n- `RateLimit-User-Reset`: The number of seconds remaining until the per-user time window resets."
- A `429` carries a five-field JSON body naming which limit was exceeded; defaults are 200
  requests/minute per organization and 50 per user, and a request counts against both. (source:
  same page)
  - "The `429` response body includes additional context about which limit was exceeded:\n\n```json\n{\n  \"message\": \"You have exceeded your API rate limit. Please wait 42 seconds before making more requests.\",\n  \"scope\": \"rest_user\",\n  \"limit\": 50,\n  \"current\": 55,\n  \"reset\": 42\n}\n```\n\nThe `scope` field indicates which limit was exceeded. For example, `rest` for the organization limit or `rest_user` for the per-user limit."
- Pagination is a `Link` response header plus `page` and `per_page` query parameters, `per_page`
  defaulting to 30 and capped at 100. **The documented `Link` example embeds an `api_key` query
  parameter**, so a `Link` header from Buildkite can carry a credential and must never be logged,
  journalled, or followed blindly. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/rest_api.md>)
  - "For endpoints which support pagination, the pagination information can be found in the `Link` HTTP response header containing zero or more of `next`, `prev`, `first` and `last`.\n\n| `page` | The page of results to return _Default:_ `1` |\n| --- | --- |\n| `per_page` | How many results to return per-page _Default:_ `30` _Maximum:_ `100` |"
- Token prefixes, with the bodies masked in the source (the asterisk runs are a mask, not a
  format specification — no length may be inferred from them). The API access token family is
  `bkua_` ("Buildkite user access"); the agent/cluster token family is `bkct_`. The prefix rule
  binds only tokens created after March 2023 (API access) or April 2025 (agent). (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/platform/security/tokens.md>)
  - "### API access tokens\n\nBuildkite [API access tokens](/docs/apis/managing-api-tokens) are also known as _Buildkite user access_ tokens, whose acronym forms the prefix for these types of tokens.\n\n- Prefix: `bkua_`\n- Example: `bkua_*****************************************************`\n\n_Applies to API access tokens created after:  March, 2023_"
- The full published family, each an underscore-suffixed acronym followed by a long run:
  `bkua_` API access, `bkaa_` agent session, `bkaj_` agent job, `bkar_` unclustered agent,
  `bkct_` agent (cluster), `bkpt_` registry, `bkpat_` portal, `bkps_` portal secret, `bkjat_`
  job acquisition. (source: same page)
  - "### Agent tokens\n\nBuildkite [agent tokens](/docs/agent/self-hosted/tokens) are also known as _Buildkite cluster tokens_, whose acronym forms the prefix for these types of tokens.\n\n- Prefix: `bkct_`\n ... \n_Applies to agent tokens created after: April, 2025_"
- **A correction worth recording.** A WebSearch snippet claimed agent tokens use the `bkua_`
  prefix and invented a `bka_` API token format. The primary source above contradicts both. This
  is the second time in this project a summarizing tool's paraphrase differed from the verbatim
  source (the first was rmcp's 4 MiB / `legacy_session_mode: true`), which is the argument for
  the repository's verbatim-fetch rule. Nothing from that snippet is used here.
- REST is authoritative for everything this slice needs. Granular token scopes and
  `provider_settings` on create/update are REST-only; two things run the other way and are out of
  reach of a REST-only provider: updating a pipeline's **teams**, and deleting or rotating a
  provider webhook. (source:
  <https://raw.githubusercontent.com/buildkite/docs/main/pages/apis/api_differences.md>)
  - "- <%= pill \"PIPELINES\", \"pipelines\" %> [Set source code provider settings](/docs/apis/rest-api/pipelines#provider-settings-properties) when creating or updating a pipeline.\n...\n- <%= pill \"PIPELINES\", \"pipelines\" %> [Delete a source code provider webhook](/docs/apis/graphql/schemas/mutation/pipelinedeletewebhook) or [rotate a pipeline webhook URL](/docs/apis/graphql/schemas/mutation/pipelinerotatewebhookurl)."


## 4. Conclusions for willikins

These are conclusions, not quotes. Each is argued in the plan; they are collected here so the
plan can cite one place.

1. **The minimum credential grant is `read_pipelines` + `write_pipelines` + `read_clusters`.**
   `read_organization_settings` is deliberately *not* needed, because willikins always sends
   `cluster_id` rather than relying on the org default (section 2), and asking for it would push
   the butler's Buildkite credential to organization administrator for no gain.
2. **`write_pipelines` is also delete.** There is no least-privilege split available (section 3),
   unlike GitHub's and Doppler's. A Buildkite credential that can create a pipeline can destroy
   one. This belongs in the plan's trust boundaries, stated out loud, not discovered later.
3. **Idempotence is read-then-create by slug, never error-body parsing.** The duplicate-create
   status and body are undocumented, so no willikins code may branch on them; `GET /pipelines/{slug}`
   first, create only on absence, and on an ambiguous create failure re-read — the exact shape
   `doppler.project.ensure` already uses.
4. **Two response values are credential-bearing and must never be parsed into an output:**
   `provider.webhook_url` and the `Link` pagination header. The pipeline response struct should
   deserialize only the fields willikins needs, and the cluster listing should page with explicit
   `page`/`per_page` parameters rather than reading `Link` at all.
5. **The one shell-looking string in the whole surface is the upload bootstrap**, and it is a
   frozen constant of the create tool rather than a caller port, which is what keeps "no tool may
   take a raw URL, shell command, or arbitrary API path as input" satisfied rather than bent.
6. **The repository URL is likewise not a raw-URL port:** the provider constructs
   `git@github.com:{owner}/{name}.git` from an already-typed `GitHubRepo`.
7. **Send `slug` explicitly on create.** Derivation from `name` is lossy and out of willikins'
   control, and a `PATCH` of `name` without `slug` silently moves the pipeline's URL. The design
   doc's naming table already carries the row "Buildkite pipeline slug | kebab | `third-thoughts`",
   so a `naming::v1` row is a row addition, which the design doc says is not a version bump.
8. **A cluster name cannot be a natural key** (mutable, nowhere stated unique, no name-keyed
   lookup), so a name-to-id lookup must distinguish found / absent / ambiguous and must never be
   cached or treated as an idempotence key. Only the UUID is stable.
9. **The secret-literal guard must learn the Buildkite family, not just `bkua_`.** A guard that
   knew only the token willikins itself holds would miss every agent, portal and registry token.
   Match the prefix plus a long alphanumeric run with a length floor; never an exact length,
   because the published bodies are masked.
10. **Backoff cannot rest on `Retry-After` for this provider.** Buildkite's own headers are
    `RateLimit-Reset` and `RateLimit-User-Reset`, in relative seconds, selected by the 429 body's
    `scope`.


## 5. Verify with a browser before relying on them

Everything below is unsettled by the documentation. None of it may enter frozen code; each is
either answered by the opt-in live probe against `willikins-test` (which should run while the
sandbox token lives — it expires seven days from 2026-09-16) or left as a documented unknown.

1. **The `404` body and status on an unknown pipeline slug.** `GET /pipelines/{slug}` has no
   documented error table, and the `Absent` arm of `buildkite.pipeline.ensure`'s `read` rests on
   it. Read-only probe: `GET /v2/organizations/willikins-test/pipelines/<absent-slug>`.
2. **The `401` body.** Documented nowhere. Read-only probe with a deliberately malformed token
   against `GET /v2/user` — needs no real credential.
3. **The duplicate-create status and body.** Documented only as "results in an error". The
   recommended design never branches on it; a friendly message would need a live create.
4. **Whether an explicitly supplied `slug` is preserved verbatim or lowercased**, and whether a
   slug that collides under a *different* name errors like a name collision. Only name collisions
   are documented.
5. **The exact scope strings `GET /v2/access-token` returns** — the example says `read_build`
   (singular), the table says `read_pipelines` / `write_pipelines` (plural). Settled read-only by
   the probe, which also reports `expires_at`.
6. **Whether the sandbox token actually carries `write_pipelines` and `read_clusters`** — not
   knowable from documentation. Same probe.
7. **Whether the sandbox token carries the `bkua_` prefix.** The prefix rule binds only tokens
   created after March 2023, so a strict prefix check would refuse a legitimate older token.
   Checkable inside the live probe without printing the value, by emitting only a boolean.
8. **Whether `cluster_id` is genuinely rejected when omitted**, given that the required table,
   the clusters-manage page and the stale response example disagree (section 1). Moot under the
   recommended design, which always sends it.
9. **Whether cluster `name` is unique within an organization.** Decides whether the lookup's
   ambiguity arm is reachable; assume it is until disproved.
10. **Whether the sandbox cluster has a `default_queue_id`, and whether that queue is self-hosted
    or Buildkite-hosted.** Irrelevant to creating a pipeline, but it decides what a sensible
    `agents: queue:` value is in the repository's own `.buildkite/pipeline.yml`.
11. **Whether `cluster_id` can actually be changed by `PATCH`.** The table lists it with an empty
    description. willikins refuses rather than reconciles either way, so this only decides
    whether the refusal's message may suggest a fix.
12. **The Buildkite organization slug's grammar.** Undocumented: no page states the character
    class or length of `{org.slug}`. `BuildkiteOrg`'s pattern is therefore chosen conservatively
    (lowercase kebab, the shape Buildkite issues, e.g. `willikins-test`), and an organization
    whose slug uses an underscore or uppercase letter would be refused at parse time with a
    named type error rather than mis-routed.
13. **Exact body lengths and character classes of each token prefix family.** The security page
    masks every body, so the guard's floor is chosen, not derived; a tighter rule would need a
    real sample, which no one should paste into the tree.
14. **How Buildkite derives `provider.id` from the repository URL.** Inferred from examples only.
15. **Whether the per-organization and per-user rate limits for `willikins-test` are the
    documented defaults.** `GET /v2/organizations/{slug}/rate_limit` reads the real values and,
    per the docs, does not itself count against either limit.
