#!/usr/bin/env bash
# Bats-free test for deploy/teardown.sh: stubs `willikins` and `curl` on
# PATH, so no real network call, real repository, or real Doppler
# project is ever touched. The stub `curl` records every call's argv,
# its stdin and the `WILLIKINS_*` variables it started with, so a
# scenario can assert where a token did and did not travel. Every scenario below is a case this script
# must get right or exit non-zero -- run by the workspace gate through
# crates/willikins-cli/tests/teardown_script.rs, which only spawns this
# file and checks its exit code.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
teardown="$script_dir/teardown.sh"

failures=0
fail() {
  echo "FAIL: $1" >&2
  failures=$((failures + 1))
}

# Distinctive markers for the two tokens, so a scenario can assert each
# reached the stub `curl` only on its stdin -- never on its argv, and
# never in its environment either.
github_token_marker="teardown-test-github-token-MARKER-3af1"
doppler_token_marker="teardown-test-doppler-token-MARKER-8f2c"
buildkite_token_marker="teardown-test-buildkite-token-MARKER-9c17"

# The exit code the stub `willikins` leaves with (the real CLI exits 1
# for a failed or still-running run while printing the whole record).
stub_willikins_exit=0

# --- one fresh sandbox per scenario: stub bin dir, fake run record,
# canned GitHub/Doppler responses, and a call log every stub appends
# to. ------------------------------------------------------------------
new_sandbox() {
  local dir
  dir="$(mktemp -d)"
  mkdir -p "$dir/bin"
  echo "$dir"
}

# Args: sandbox, repo value, project value, [run state] (default
# "succeeded"; "failed" for the case the stub also exits 1 on).
write_run_record() {
  local dir="$1" repo="$2" project="$3" state="${4:-succeeded}"
  cat > "$dir/run.json" <<JSON
{
  "run_id": "01000000-0000-7000-8000-000000000000",
  "plan_id": "01000000-0000-7000-8000-000000000001",
  "principal": "test",
  "started_at": "2026-01-01T00:00:00+00:00",
  "state": "$state",
  "nodes": [
    {
      "node": "repo",
      "instance": null,
      "status": "created",
      "outputs": {
        "repo": {"type": "GitHubRepo", "list": false, "state": "known", "value": "$repo"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "https://github.com/$repo"}
      }
    },
    {
      "node": "doppler",
      "instance": null,
      "status": "created",
      "outputs": {
        "project": {"type": "DopplerProject", "list": false, "state": "known", "value": "$project"}
      }
    }
  ],
  "outputs": {},
  "error": null,
  "finished_at": "2026-01-01T00:05:00+00:00"
}
JSON
}

# Args: sandbox, repo value, project value, pipeline org, pipeline slug,
# [run state] (default "succeeded"). Like `write_run_record`, but the
# plan also carries a `pipeline` node -- the shape
# `workflows/new-rust-service-buildkite.yaml` produces, whose only two
# outputs are `slug` and `url` (never the organisation on its own: that
# is why teardown.sh parses it back out of `url`).
write_run_record_with_pipeline() {
  local dir="$1" repo="$2" project="$3" pipeline_org="$4" pipeline_slug="$5" \
    state="${6:-succeeded}"
  cat > "$dir/run.json" <<JSON
{
  "run_id": "01000000-0000-7000-8000-000000000000",
  "plan_id": "01000000-0000-7000-8000-000000000001",
  "principal": "test",
  "started_at": "2026-01-01T00:00:00+00:00",
  "state": "$state",
  "nodes": [
    {
      "node": "repo",
      "instance": null,
      "status": "created",
      "outputs": {
        "repo": {"type": "GitHubRepo", "list": false, "state": "known", "value": "$repo"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "https://github.com/$repo"}
      }
    },
    {
      "node": "doppler",
      "instance": null,
      "status": "created",
      "outputs": {
        "project": {"type": "DopplerProject", "list": false, "state": "known", "value": "$project"}
      }
    },
    {
      "node": "pipeline",
      "instance": null,
      "status": "created",
      "outputs": {
        "slug": {"type": "BuildkitePipelineSlug", "list": false, "state": "known", "value": "$pipeline_slug"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "https://buildkite.com/$pipeline_org/$pipeline_slug"}
      }
    }
  ],
  "outputs": {},
  "error": null,
  "finished_at": "2026-01-01T00:05:00+00:00"
}
JSON
}

# Args: sandbox, repo value, project value, [run state] -- like
# `write_run_record_with_pipeline`, but the `pipeline` node is present
# and was never reached: empty outputs, `status: "not_run"`. Distinct
# from a document with no pipeline concept at all (the node is simply
# absent from `.nodes[]`), which `write_run_record` above already models
# for every one of scenarios 1 through 14.
write_run_record_with_unreached_pipeline() {
  local dir="$1" repo="$2" project="$3" state="${4:-failed}"
  cat > "$dir/run.json" <<JSON
{
  "run_id": "01000000-0000-7000-8000-000000000000",
  "plan_id": "01000000-0000-7000-8000-000000000001",
  "principal": "test",
  "started_at": "2026-01-01T00:00:00+00:00",
  "state": "$state",
  "nodes": [
    {
      "node": "repo",
      "instance": null,
      "status": "created",
      "outputs": {
        "repo": {"type": "GitHubRepo", "list": false, "state": "known", "value": "$repo"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "https://github.com/$repo"}
      }
    },
    {
      "node": "doppler",
      "instance": null,
      "status": "created",
      "outputs": {
        "project": {"type": "DopplerProject", "list": false, "state": "known", "value": "$project"}
      }
    },
    {
      "node": "pipeline",
      "instance": null,
      "status": {"kind": "not_run"},
      "outputs": {}
    }
  ],
  "outputs": {},
  "error": null,
  "finished_at": "2026-01-01T00:05:00+00:00"
}
JSON
}

# The stub exits with `$STUB_WILLIKINS_EXIT` (0 unless a scenario says
# otherwise) *after* printing the record, exactly as the real `willikins
# run --json` does: it prints the whole RunRecord and then exits 1 for a
# run whose state is `failed` or `running`
# (crates/willikins-cli/src/commands.rs, `exit_for_run_state`).
write_stub_willikins() {
  local dir="$1"
  cat > "$dir/bin/willikins" <<'STUB'
#!/usr/bin/env bash
set -uo pipefail
echo "WILLIKINS_CALL $*" >> "$STUB_LOG"
cat "$RUN_JSON_FILE"
exit "${STUB_WILLIKINS_EXIT:-0}"
STUB
  chmod +x "$dir/bin/willikins"
}

# Args: sandbox, topics-json-array (e.g. '["managed-by-willikins"]') --
# what the stub answers a GitHub repository read with.
write_github_get_response() {
  local dir="$1" topics="$2"
  cat > "$dir/github_get_response.json" <<JSON
{"topics": $topics}
JSON
}

# Args: sandbox, description-or-empty -- what the stub answers a Doppler
# project read with.
write_doppler_get_response() {
  local dir="$1" description="$2"
  cat > "$dir/doppler_get_response.json" <<JSON
{"project": {"description": "$description"}}
JSON
}

# Args: sandbox, description-or-empty -- what the stub answers a
# Buildkite pipeline read with.
write_buildkite_get_response() {
  local dir="$1" description="$2"
  cat > "$dir/buildkite_get_response.json" <<JSON
{"description": "$description"}
JSON
}

# `deploy/teardown.sh` now speaks to both providers through `curl` --
# there is no `gh` any more. One stub answers both: it tells GitHub's
# `/repos/...` calls from Doppler's `/v3/projects/project...` calls by
# the URL each carries (the last argument starting with `http`), and
# `GET` from `DELETE` the same way the script's own two providers do.
write_stub_curl() {
  local dir="$1"
  cat > "$dir/bin/curl" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
method="GET"
args=("$@")
url=""
for i in "${!args[@]}"; do
  case "${args[$i]}" in
    -X)
      method="${args[$((i + 1))]}"
      ;;
    http://*|https://*)
      url="${args[$i]}"
      ;;
  esac
done
stdin_content="$(cat)"
# Every WILLIKINS_* variable this process was started with. A token in
# here is a token `ps -E` would show to anything running as this user
# for as long as the call lasts, so it is recorded next to argv and
# asserted against just as hard.
env_seen="$(env | grep -E '^WILLIKINS_' | tr '\n' ' ' || true)"
{
  echo "CURL_CALL method=$method url=$url args=[${args[*]}]"
  echo "CURL_STDIN=[$stdin_content]"
  echo "CURL_ENV=[$env_seen]"
} >> "$STUB_LOG"
case "$url" in
  *api.github.com/repos/*)
    if [ "$method" = "DELETE" ]; then
      echo "{}"
    else
      cat "$GITHUB_GET_RESPONSE_FILE"
    fi
    ;;
  *api.doppler.com/v3/projects/project*)
    if [ "$method" = "DELETE" ]; then
      echo '{"project": {}}'
    else
      cat "$DOPPLER_GET_RESPONSE_FILE"
    fi
    ;;
  *api.buildkite.com/v2/organizations/*/pipelines/*)
    if [ "$method" = "DELETE" ]; then
      echo '{}'
    else
      cat "$BUILDKITE_GET_RESPONSE_FILE"
    fi
    ;;
  *)
    echo "stub curl: unrecognized URL: $url" >&2
    exit 1
    ;;
esac
STUB
  chmod +x "$dir/bin/curl"
}

# Runs teardown.sh inside `dir`'s stubbed environment. Extra args (e.g.
# `--yes`) are forwarded. `WILLIKINS_BUILDKITE_TOKEN` and
# `BUILDKITE_GET_RESPONSE_FILE` are passed unconditionally: teardown.sh
# only ever reads either when the run record carries a `pipeline` node,
# so scenarios 1 through 14 (none of which do) are unaffected by their
# presence, and every scenario below that does carry one does not need
# its own copy of this function.
run_teardown() {
  local dir="$1"
  shift
  PATH="$dir/bin:$PATH" \
    STUB_LOG="$dir/calls.log" \
    STUB_WILLIKINS_EXIT="${stub_willikins_exit:-0}" \
    RUN_JSON_FILE="$dir/run.json" \
    GITHUB_GET_RESPONSE_FILE="$dir/github_get_response.json" \
    DOPPLER_GET_RESPONSE_FILE="$dir/doppler_get_response.json" \
    BUILDKITE_GET_RESPONSE_FILE="$dir/buildkite_get_response.json" \
    WILLIKINS_GITHUB_TOKEN="$github_token_marker" \
    WILLIKINS_DOPPLER_TOKEN="$doppler_token_marker" \
    WILLIKINS_BUILDKITE_TOKEN="$buildkite_token_marker" \
    "$teardown" "01000000-0000-7000-8000-000000000000" "$dir/journal.jsonl" "$@"
}

# =======================================================================
# Scenario 1: both markers present, dry run -- exits 0, prints "would
# delete" for both resources, calls neither delete endpoint.
# =======================================================================
scenario1() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" 2>&1) && status=0 || status=$?

  [ "$status" -eq 0 ] || fail "scenario1: expected exit 0, got $status; output: $output"
  echo "$output" | grep -q "would delete GitHub repository: Willikins-Test/teardown-test-repo" \
    || fail "scenario1: missing 'would delete' line for the repository"
  echo "$output" | grep -q "would delete Doppler project:   teardown-test-project" \
    || fail "scenario1: missing 'would delete' line for the project"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario1: a dry run must not call either delete endpoint"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 2: the GitHub repository is missing the ownership topic --
# refuses (non-zero), never reaches Doppler or either delete endpoint.
# =======================================================================
scenario2() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '[]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario2: expected a non-zero exit when the topic is missing"
  echo "$output" | grep -q "managed-by-willikins" \
    || fail "scenario2: refusal message should name the missing topic"
  grep -q "CURL_CALL.*api.doppler.com" "$dir/calls.log" \
    && fail "scenario2: must never call Doppler once the GitHub check has refused"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 3: the Doppler project is missing (or carries the wrong)
# description -- refuses, never deletes anything.
# =======================================================================
scenario3() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "some other description"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario3: expected a non-zero exit when the description is wrong"
  echo "$output" | grep -q "managed-by: willikins" \
    || fail "scenario3: refusal message should name the expected description"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario3: must never delete anything once the Doppler check has refused"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 4: both markers present, `--yes` -- actually calls both
# delete endpoints, and neither token reached `curl` as one of its
# command-line arguments *or* in its environment; both reach it, with
# the three GitHub headers and the Doppler Authorization header, only
# through `--config -`.
# =======================================================================
scenario4() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -eq 0 ] || fail "scenario4: expected exit 0, got $status; output: $output"
  grep -q "CURL_CALL method=DELETE url=https://api.github.com/repos/Willikins-Test/teardown-test-repo" \
    "$dir/calls.log" \
    || fail "scenario4: curl DELETE was not called for the GitHub repository"
  grep -q "CURL_CALL method=DELETE url=https://api.doppler.com/v3/projects/project" \
    "$dir/calls.log" \
    || fail "scenario4: curl DELETE was not called for the Doppler project"

  if grep -q "CURL_CALL.*$github_token_marker" "$dir/calls.log"; then
    fail "scenario4: the GitHub token leaked into curl's own arguments"
  fi
  if grep -q "CURL_CALL.*$doppler_token_marker" "$dir/calls.log"; then
    fail "scenario4: the Doppler token leaked into curl's own arguments"
  fi
  # Not only this test's own markers: no argument may hold anything
  # shaped like either provider's real token at all.
  if grep -qE "CURL_CALL.*(ghp_|github_pat_)" "$dir/calls.log"; then
    fail "scenario4: a GitHub-token-shaped argument reached curl's argv"
  fi
  if grep -qE "CURL_CALL.*dp\.(sa|st|pt)\." "$dir/calls.log"; then
    fail "scenario4: a Doppler-token-shaped argument reached curl's argv"
  fi

  # argv is only half of what a process listing gives away: `ps -E`
  # shows a process's environment to anything running as the same user,
  # for as long as the call lasts. `curl` must therefore start with
  # neither token in its environment at all. The CURL_ENV line count is
  # asserted too, so "no token in CURL_ENV" cannot pass by the stub
  # having recorded no environment.
  local env_lines
  env_lines=$(grep -c "^CURL_ENV=" "$dir/calls.log" || true)
  [ "$env_lines" -eq 4 ] \
    || fail "scenario4: expected 4 recorded curl environments, got $env_lines"
  if grep -q "CURL_ENV.*$github_token_marker" "$dir/calls.log"; then
    fail "scenario4: the GitHub token was in curl's own environment"
  fi
  if grep -q "CURL_ENV.*$doppler_token_marker" "$dir/calls.log"; then
    fail "scenario4: the Doppler token was in curl's own environment"
  fi

  # Every one of the four calls (two reads, two deletes) carries
  # --config -: the one way a header-borne token stays out of `ps`.
  local curl_calls config_calls
  curl_calls=$(grep -c "CURL_CALL" "$dir/calls.log" || true)
  config_calls=$(grep -c "CURL_CALL.*--config -" "$dir/calls.log" || true)
  [ "$curl_calls" -eq 4 ] || fail "scenario4: expected 4 curl calls, got $curl_calls"
  [ "$config_calls" -eq 4 ] \
    || fail "scenario4: every curl call must carry --config -; $config_calls of $curl_calls do"

  grep -q "Authorization: Bearer $github_token_marker" "$dir/calls.log" \
    || fail "scenario4: the GitHub token never reached curl's stdin (--config -)"
  grep -q "Authorization: Bearer $doppler_token_marker" "$dir/calls.log" \
    || fail "scenario4: the Doppler token never reached curl's stdin (--config -)"
  grep -q 'Accept: application/vnd.github+json' "$dir/calls.log" \
    || fail "scenario4: the GitHub Accept header was not passed on stdin"
  grep -q 'X-GitHub-Api-Version: 2022-11-28' "$dir/calls.log" \
    || fail "scenario4: the GitHub X-GitHub-Api-Version header was not passed on stdin"
  grep -q 'User-Agent: willikins-teardown' "$dir/calls.log" \
    || fail "scenario4: the GitHub User-Agent header was not passed on stdin"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 5: a run record with no `doppler` output -- refuses before
# calling curl at all (never guesses a project name).
# =======================================================================
scenario5() {
  local dir
  dir="$(new_sandbox)"
  cat > "$dir/run.json" <<'JSON'
{
  "run_id": "01000000-0000-7000-8000-000000000000",
  "plan_id": "01000000-0000-7000-8000-000000000001",
  "principal": "test",
  "started_at": "2026-01-01T00:00:00+00:00",
  "state": "failed",
  "nodes": [
    {
      "node": "repo",
      "instance": null,
      "status": "created",
      "outputs": {
        "repo": {"type": "GitHubRepo", "list": false, "state": "known", "value": "Willikins-Test/teardown-test-repo"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "https://github.com/Willikins-Test/teardown-test-repo"}
      }
    }
  ],
  "outputs": {},
  "error": null,
  "finished_at": "2026-01-01T00:01:00+00:00"
}
JSON
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario5: expected a non-zero exit with no doppler output"
  if grep -qE "^CURL_CALL" "$dir/calls.log"; then
    fail "scenario5: must not call curl at all: $(cat "$dir/calls.log")"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 6: the run failed. `willikins run --json` prints the whole
# record and then exits 1 for a run whose state is `failed` -- and a
# failed run is the one that leaves resources behind, so it is the case
# this script exists for. The exit code must not be read as "could not
# read the run".
# =======================================================================
scenario6() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project" "failed"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  stub_willikins_exit=1
  output=$(run_teardown "$dir" 2>&1) && status=0 || status=$?
  stub_willikins_exit=0

  [ "$status" -eq 0 ] \
    || fail "scenario6: a failed run must still be tearable down; got $status; output: $output"
  echo "$output" | grep -q "would delete GitHub repository: Willikins-Test/teardown-test-repo" \
    || fail "scenario6: missing 'would delete' line for the repository"
  echo "$output" | grep -q "would delete Doppler project:   teardown-test-project" \
    || fail "scenario6: missing 'would delete' line for the project"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 7: an unknown run id. The real CLI prints its own error
# document (`{"kind": "UnknownRun", ...}`, no `run_id`) and exits 1.
# That is not a run record, so the script must refuse and call curl not
# at all -- the exit code alone can no longer tell it so.
# =======================================================================
scenario7() {
  local dir
  dir="$(new_sandbox)"
  cat > "$dir/run.json" <<'JSON'
{
  "kind": "UnknownRun",
  "message": "no run with id 01000000-0000-7000-8000-000000000000"
}
JSON
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  stub_willikins_exit=1
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?
  stub_willikins_exit=0

  [ "$status" -ne 0 ] || fail "scenario7: expected a non-zero exit for an unknown run"
  echo "$output" | grep -q "could not read run" \
    || fail "scenario7: refusal should say the run could not be read; output: $output"
  if grep -qE "^CURL_CALL" "$dir/calls.log"; then
    fail "scenario7: must not call curl at all: $(cat "$dir/calls.log")"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 8: every curl call carries --fail. `curl` exits 0 on a 4xx or
# a 5xx unless it is given `--fail`, so without it the script would
# report a delete that never happened. All four calls this run makes
# (GitHub read, GitHub delete, Doppler read, Doppler delete) must carry
# it.
# =======================================================================
scenario8() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?
  [ "$status" -eq 0 ] || fail "scenario8: setup run failed: $output"
  local curl_calls
  curl_calls=$(grep -c "CURL_CALL" "$dir/calls.log" || true)
  [ "$curl_calls" -eq 4 ] || fail "scenario8: expected 4 curl calls, got $curl_calls"
  local failing_calls
  failing_calls=$(grep -c "CURL_CALL.*--fail" "$dir/calls.log" || true)
  [ "$failing_calls" -eq 4 ] \
    || fail "scenario8: every curl call must carry --fail; $failing_calls of $curl_calls do"
  rm -rf "$dir"
}

# A `curl` stub that answers both providers normally except for exactly
# one call, named by its method and its host. That one call fails the
# way real curl fails: `22` is what `--fail` turns a 4xx or a 5xx into
# (nothing useful on stdout), `7` is curl dying without an HTTP status
# at all. Scenarios 9 to 12 use it to prove that a failed call to either
# provider, read or delete, is a refusal and never a silent "done".
write_stub_curl_one_call_fails() {
  local dir="$1" fail_method="$2" fail_host="$3" fail_code="${4:-22}"
  cat > "$dir/bin/curl" <<STUB
#!/usr/bin/env bash
set -uo pipefail
fail_method="$fail_method"
fail_host="$fail_host"
fail_code="$fail_code"
STUB
  cat >> "$dir/bin/curl" <<'STUB'
method="GET"
args=("$@")
url=""
for i in "${!args[@]}"; do
  case "${args[$i]}" in
    -X)
      method="${args[$((i + 1))]}"
      ;;
    http://*|https://*)
      url="${args[$i]}"
      ;;
  esac
done
stdin_content="$(cat)"
# Every WILLIKINS_* variable this process was started with. A token in
# here is a token `ps -E` would show to anything running as this user
# for as long as the call lasts, so it is recorded next to argv and
# asserted against just as hard.
env_seen="$(env | grep -E '^WILLIKINS_' | tr '\n' ' ' || true)"
{
  echo "CURL_CALL method=$method url=$url args=[${args[*]}]"
  echo "CURL_STDIN=[$stdin_content]"
  echo "CURL_ENV=[$env_seen]"
} >> "$STUB_LOG"
if [ "$method" = "$fail_method" ] && [[ "$url" == *"$fail_host"* ]]; then
  echo "curl: ($fail_code) the $fail_method to $url failed" >&2
  exit "$fail_code"
fi
case "$url" in
  *api.github.com/repos/*)
    if [ "$method" = "DELETE" ]; then
      echo "{}"
    else
      cat "$GITHUB_GET_RESPONSE_FILE"
    fi
    ;;
  *api.doppler.com/v3/projects/project*)
    if [ "$method" = "DELETE" ]; then
      echo '{"project": {}}'
    else
      cat "$DOPPLER_GET_RESPONSE_FILE"
    fi
    ;;
  *api.buildkite.com/v2/organizations/*/pipelines/*)
    if [ "$method" = "DELETE" ]; then
      echo '{}'
    else
      cat "$BUILDKITE_GET_RESPONSE_FILE"
    fi
    ;;
  *)
    echo "stub curl: unrecognized URL: $url" >&2
    exit 1
    ;;
esac
STUB
  chmod +x "$dir/bin/curl"
}

# =======================================================================
# Scenario 9: GitHub answers the delete with an HTTP error, Doppler's
# delete would succeed -- the script must stop before ever reaching the
# Doppler delete, and never print "done".
# =======================================================================
scenario9() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl_one_call_fails "$dir" DELETE "api.github.com" 22
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?
  [ "$status" -ne 0 ] || fail "scenario9: a failing GitHub delete must not exit 0"
  echo "$output" | grep -q "teardown.sh: done" \
    && fail "scenario9: must not print 'done' after a delete that failed"
  grep -q "CURL_CALL method=DELETE url=https://api.doppler.com" "$dir/calls.log" \
    && fail "scenario9: must never reach the Doppler delete once the GitHub delete has failed"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 10: GitHub's delete succeeds, Doppler answers its delete with
# an HTTP error -- same rule, same non-zero exit, no "done", proven
# independently of scenario 9's failure.
# =======================================================================
scenario10() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl_one_call_fails "$dir" DELETE "api.doppler.com" 22
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?
  [ "$status" -ne 0 ] || fail "scenario10: a failing Doppler delete must not exit 0"
  echo "$output" | grep -q "teardown.sh: done" \
    && fail "scenario10: must not print 'done' after a delete that failed"
  grep -q "CURL_CALL method=DELETE url=https://api.github.com" "$dir/calls.log" \
    || fail "scenario10: the GitHub delete should have run before the Doppler delete failed"
  rm -rf "$dir"
}

# One scenario body for both credentials: `$1` is the variable to unset,
# `$2` the scenario's own name for its failure messages. The script must
# refuse, name the variable it is missing, and reach neither provider --
# a half-done teardown is worse than one that never started.
assert_missing_token_refuses() {
  local missing="$1" label="$2"
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  # Both tokens are exported *first* and `env -u` then removes the one
  # under test: `env -u X X=v` would set it straight back, and the
  # scenario would prove nothing.
  local output status
  output=$(
    export WILLIKINS_GITHUB_TOKEN="$github_token_marker"
    export WILLIKINS_DOPPLER_TOKEN="$doppler_token_marker"
    env -u "$missing" \
      PATH="$dir/bin:$PATH" \
      STUB_LOG="$dir/calls.log" \
      STUB_WILLIKINS_EXIT="0" \
      RUN_JSON_FILE="$dir/run.json" \
      GITHUB_GET_RESPONSE_FILE="$dir/github_get_response.json" \
      DOPPLER_GET_RESPONSE_FILE="$dir/doppler_get_response.json" \
      "$teardown" "01000000-0000-7000-8000-000000000000" "$dir/journal.jsonl" --yes 2>&1
  ) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "$label: expected a non-zero exit when $missing is unset"
  echo "$output" | grep -q "$missing must be set" \
    || fail "$label: refusal should name $missing; output: $output"
  if grep -qE "^CURL_CALL" "$dir/calls.log"; then
    fail "$label: must not call curl before the token check: $(cat "$dir/calls.log")"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 11: `WILLIKINS_GITHUB_TOKEN` is unset -- refuses with a named
# message, before ever calling curl.
# =======================================================================
scenario11() {
  assert_missing_token_refuses WILLIKINS_GITHUB_TOKEN scenario11
}

# =======================================================================
# Scenario 12: `WILLIKINS_DOPPLER_TOKEN` is unset -- the same refusal.
# Both credentials are demanded together, before the first provider call,
# so a missing Doppler token cannot be discovered only after the GitHub
# repository has already been deleted.
# =======================================================================
scenario12() {
  assert_missing_token_refuses WILLIKINS_DOPPLER_TOKEN scenario12
}

# =======================================================================
# Scenario 13: the GitHub *ownership read* fails -- a 404 (the
# repository is gone, or the PAT cannot see it), a 401, a 403, a 5xx:
# `--fail` turns all of them into a non-zero exit with no document. The
# script must refuse by name, reach Doppler not at all, and delete
# nothing. An unreadable marker is not an absent marker, and neither is
# a licence to delete.
# =======================================================================
scenario13() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl_one_call_fails "$dir" GET "api.github.com" 22
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario13: a failing GitHub ownership read must not exit 0"
  echo "$output" | grep -q "could not read repository" \
    || fail "scenario13: refusal should say the repository could not be read; output: $output"
  if grep -q "CURL_CALL.*api.doppler.com" "$dir/calls.log"; then
    fail "scenario13: must never reach Doppler once the GitHub read has failed"
  fi
  if grep -q "CURL_CALL method=DELETE" "$dir/calls.log"; then
    fail "scenario13: must delete nothing when the ownership read failed"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 14: the Doppler ownership read fails, this time the way curl
# fails when the call never got an HTTP answer at all (exit 7). Same
# rule: refuse by name, delete nothing -- including the GitHub
# repository, whose own marker did check out.
# =======================================================================
scenario14() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl_one_call_fails "$dir" GET "api.doppler.com" 7
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario14: a failing Doppler ownership read must not exit 0"
  echo "$output" | grep -q "could not read Doppler project" \
    || fail "scenario14: refusal should say the project could not be read; output: $output"
  if grep -q "CURL_CALL method=DELETE" "$dir/calls.log"; then
    fail "scenario14: must delete nothing when the ownership read failed"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 15: a run record whose document has a `pipeline` node, dry
# run -- exits 0, prints "would delete" for all three resources
# (including the pipeline, its organisation parsed out of the recorded
# `url`), calls no delete endpoint, and reads exactly three resources
# (no fourth call for a fourth resource that does not exist).
# =======================================================================
scenario15() {
  local dir
  dir="$(new_sandbox)"
  write_run_record_with_pipeline "$dir" "Willikins-Test/teardown-test-repo" \
    "teardown-test-project" "teardown-test-org" "teardown-test-pipeline"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_buildkite_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" 2>&1) && status=0 || status=$?

  [ "$status" -eq 0 ] || fail "scenario15: expected exit 0, got $status; output: $output"
  echo "$output" \
    | grep -q "would delete Buildkite pipeline: teardown-test-org/teardown-test-pipeline" \
    || fail "scenario15: missing 'would delete' line for the pipeline"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario15: a dry run must not call any delete endpoint"
  local curl_calls
  curl_calls=$(grep -c "CURL_CALL" "$dir/calls.log" || true)
  [ "$curl_calls" -eq 3 ] || fail "scenario15: expected 3 curl calls (three reads), got $curl_calls"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 16: the same pipeline-bearing run, `--yes` -- deletes all
# three resources including the Buildkite pipeline, and the Buildkite
# token never reaches curl's argv or environment, only its stdin
# (`--config -`), mirroring scenario 4's proof for the other two tokens.
# =======================================================================
scenario16() {
  local dir
  dir="$(new_sandbox)"
  write_run_record_with_pipeline "$dir" "Willikins-Test/teardown-test-repo" \
    "teardown-test-project" "teardown-test-org" "teardown-test-pipeline"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_buildkite_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -eq 0 ] || fail "scenario16: expected exit 0, got $status; output: $output"
  grep -q "CURL_CALL method=DELETE url=https://api.buildkite.com/v2/organizations/teardown-test-org/pipelines/teardown-test-pipeline" \
    "$dir/calls.log" \
    || fail "scenario16: curl DELETE was not called for the Buildkite pipeline"

  if grep -q "CURL_CALL.*$buildkite_token_marker" "$dir/calls.log"; then
    fail "scenario16: the Buildkite token leaked into curl's own arguments"
  fi
  if grep -qE "CURL_CALL.*bkua_" "$dir/calls.log"; then
    fail "scenario16: a Buildkite-token-shaped argument reached curl's argv"
  fi

  local env_lines
  env_lines=$(grep -c "^CURL_ENV=" "$dir/calls.log" || true)
  [ "$env_lines" -eq 6 ] \
    || fail "scenario16: expected 6 recorded curl environments, got $env_lines"
  if grep -q "CURL_ENV.*$buildkite_token_marker" "$dir/calls.log"; then
    fail "scenario16: the Buildkite token was in curl's own environment"
  fi

  local curl_calls config_calls failing_calls
  curl_calls=$(grep -c "CURL_CALL" "$dir/calls.log" || true)
  config_calls=$(grep -c "CURL_CALL.*--config -" "$dir/calls.log" || true)
  failing_calls=$(grep -c "CURL_CALL.*--fail" "$dir/calls.log" || true)
  [ "$curl_calls" -eq 6 ] || fail "scenario16: expected 6 curl calls, got $curl_calls"
  [ "$config_calls" -eq 6 ] \
    || fail "scenario16: every curl call must carry --config -; $config_calls of $curl_calls do"
  [ "$failing_calls" -eq 6 ] \
    || fail "scenario16: every curl call must carry --fail; $failing_calls of $curl_calls do"

  grep -q "Authorization: Bearer $buildkite_token_marker" "$dir/calls.log" \
    || fail "scenario16: the Buildkite token never reached curl's stdin (--config -)"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 17: the Buildkite pipeline is missing (or carries the wrong)
# description -- refuses, deletes nothing at all, even though GitHub's
# and Doppler's own markers checked out.
# =======================================================================
scenario17() {
  local dir
  dir="$(new_sandbox)"
  write_run_record_with_pipeline "$dir" "Willikins-Test/teardown-test-repo" \
    "teardown-test-project" "teardown-test-org" "teardown-test-pipeline"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_buildkite_get_response "$dir" "some other description"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] \
    || fail "scenario17: expected a non-zero exit when the pipeline description is wrong"
  echo "$output" | grep -q "managed-by: willikins" \
    || fail "scenario17: refusal message should name the expected description"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario17: must never delete anything once the Buildkite check has refused"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 18: the Buildkite ownership read fails outright -- refuses by
# name, deletes nothing, including the GitHub repository and Doppler
# project whose own markers did check out (all reads happen before any
# delete).
# =======================================================================
scenario18() {
  local dir
  dir="$(new_sandbox)"
  write_run_record_with_pipeline "$dir" "Willikins-Test/teardown-test-repo" \
    "teardown-test-project" "teardown-test-org" "teardown-test-pipeline"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_buildkite_get_response "$dir" "managed-by: willikins"
  write_stub_curl_one_call_fails "$dir" GET "api.buildkite.com" 22
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario18: a failing Buildkite ownership read must not exit 0"
  echo "$output" | grep -q "could not read Buildkite pipeline" \
    || fail "scenario18: refusal should say the pipeline could not be read; output: $output"
  if grep -q "CURL_CALL method=DELETE" "$dir/calls.log"; then
    fail "scenario18: must delete nothing when the ownership read failed"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 19: a document with no `pipeline` node at all (milestone 1's
# or 2's own shape) never needs `WILLIKINS_BUILDKITE_TOKEN` -- unset it
# entirely and the run still tears down cleanly.
# =======================================================================
scenario19() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(
    export WILLIKINS_GITHUB_TOKEN="$github_token_marker"
    export WILLIKINS_DOPPLER_TOKEN="$doppler_token_marker"
    env -u WILLIKINS_BUILDKITE_TOKEN \
      PATH="$dir/bin:$PATH" \
      STUB_LOG="$dir/calls.log" \
      STUB_WILLIKINS_EXIT="0" \
      RUN_JSON_FILE="$dir/run.json" \
      GITHUB_GET_RESPONSE_FILE="$dir/github_get_response.json" \
      DOPPLER_GET_RESPONSE_FILE="$dir/doppler_get_response.json" \
      "$teardown" "01000000-0000-7000-8000-000000000000" "$dir/journal.jsonl" 2>&1
  ) && status=0 || status=$?

  [ "$status" -eq 0 ] \
    || fail "scenario19: a document with no pipeline node must not need" \
      "WILLIKINS_BUILDKITE_TOKEN; got $status; output: $output"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 20: a `pipeline` node *is* present and `WILLIKINS_BUILDKITE_
# TOKEN` is unset -- refuses by name, before calling curl at all, the
# same rule scenarios 11 and 12 pin for the other two credentials.
# =======================================================================
scenario20() {
  local dir
  dir="$(new_sandbox)"
  write_run_record_with_pipeline "$dir" "Willikins-Test/teardown-test-repo" \
    "teardown-test-project" "teardown-test-org" "teardown-test-pipeline"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_buildkite_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(
    export WILLIKINS_GITHUB_TOKEN="$github_token_marker"
    export WILLIKINS_DOPPLER_TOKEN="$doppler_token_marker"
    env -u WILLIKINS_BUILDKITE_TOKEN \
      PATH="$dir/bin:$PATH" \
      STUB_LOG="$dir/calls.log" \
      STUB_WILLIKINS_EXIT="0" \
      RUN_JSON_FILE="$dir/run.json" \
      GITHUB_GET_RESPONSE_FILE="$dir/github_get_response.json" \
      DOPPLER_GET_RESPONSE_FILE="$dir/doppler_get_response.json" \
      BUILDKITE_GET_RESPONSE_FILE="$dir/buildkite_get_response.json" \
      "$teardown" "01000000-0000-7000-8000-000000000000" "$dir/journal.jsonl" --yes 2>&1
  ) && status=0 || status=$?

  [ "$status" -ne 0 ] \
    || fail "scenario20: expected a non-zero exit when WILLIKINS_BUILDKITE_TOKEN is unset" \
      "and a pipeline node is present"
  echo "$output" | grep -q "WILLIKINS_BUILDKITE_TOKEN must be set" \
    || fail "scenario20: refusal should name WILLIKINS_BUILDKITE_TOKEN; output: $output"
  if grep -qE "^CURL_CALL" "$dir/calls.log"; then
    fail "scenario20: must not call curl before the token check: $(cat "$dir/calls.log")"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 21: the `pipeline` node is present but was never reached (a
# run that failed before getting there) -- refuses by name, exactly like
# an unreached `repo` or `doppler` node always has, rather than being
# silently skipped the way a document with no pipeline concept at all
# is.
# =======================================================================
scenario21() {
  local dir
  dir="$(new_sandbox)"
  write_run_record_with_unreached_pipeline "$dir" "Willikins-Test/teardown-test-repo" \
    "teardown-test-project"
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] \
    || fail "scenario21: expected a non-zero exit with a present-but-unreached pipeline node"
  echo "$output" | grep -q "pipeline node but no slug/url" \
    || fail "scenario21: refusal should name the missing pipeline output; output: $output"
  if grep -qE "^CURL_CALL" "$dir/calls.log"; then
    fail "scenario21: must not call curl at all: $(cat "$dir/calls.log")"
  fi
  rm -rf "$dir"
}

# =======================================================================
# Scenario 22: the `pipeline` node's `url` output does not parse into an
# organisation and the recorded slug -- refuses by name rather than
# guessing, before calling curl at all.
# =======================================================================
scenario22() {
  local dir
  dir="$(new_sandbox)"
  cat > "$dir/run.json" <<'JSON'
{
  "run_id": "01000000-0000-7000-8000-000000000000",
  "plan_id": "01000000-0000-7000-8000-000000000001",
  "principal": "test",
  "started_at": "2026-01-01T00:00:00+00:00",
  "state": "succeeded",
  "nodes": [
    {
      "node": "repo",
      "instance": null,
      "status": "created",
      "outputs": {
        "repo": {"type": "GitHubRepo", "list": false, "state": "known", "value": "Willikins-Test/teardown-test-repo"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "https://github.com/Willikins-Test/teardown-test-repo"}
      }
    },
    {
      "node": "doppler",
      "instance": null,
      "status": "created",
      "outputs": {
        "project": {"type": "DopplerProject", "list": false, "state": "known", "value": "teardown-test-project"}
      }
    },
    {
      "node": "pipeline",
      "instance": null,
      "status": "created",
      "outputs": {
        "slug": {"type": "BuildkitePipelineSlug", "list": false, "state": "known", "value": "teardown-test-pipeline"},
        "url": {"type": "HttpsUrl", "list": false, "state": "known", "value": "not-a-buildkite-url"}
      }
    }
  ],
  "outputs": {},
  "error": null,
  "finished_at": "2026-01-01T00:05:00+00:00"
}
JSON
  write_stub_willikins "$dir"
  write_github_get_response "$dir" '["managed-by-willikins"]'
  write_doppler_get_response "$dir" "managed-by: willikins"
  write_buildkite_get_response "$dir" "managed-by: willikins"
  write_stub_curl "$dir"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario22: expected a non-zero exit for a malformed pipeline url"
  echo "$output" | grep -q "could not parse a Buildkite organization" \
    || fail "scenario22: refusal should say the url could not be parsed; output: $output"
  if grep -qE "^CURL_CALL" "$dir/calls.log"; then
    fail "scenario22: must not call curl at all: $(cat "$dir/calls.log")"
  fi
  rm -rf "$dir"
}

scenario1
scenario2
scenario3
scenario4
scenario5
scenario6
scenario7
scenario8
scenario9
scenario10
scenario11
scenario12
scenario13
scenario14
scenario15
scenario16
scenario17
scenario18
scenario19
scenario20
scenario21
scenario22

if [ "$failures" -eq 0 ]; then
  echo "teardown_test.sh: all scenarios passed"
  exit 0
else
  echo "teardown_test.sh: $failures scenario(s) failed"
  exit 1
fi
