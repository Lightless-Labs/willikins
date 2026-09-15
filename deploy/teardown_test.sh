#!/usr/bin/env bash
# Bats-free test for deploy/teardown.sh: stubs `willikins`, `gh`, and
# `curl` on PATH, so no real network call, real repository, or real
# Doppler project is ever touched. Every scenario below is a case this
# script must get right or exit non-zero -- run by the workspace gate
# through crates/willikins-cli/tests/teardown_script.rs, which only
# spawns this file and checks its exit code.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
teardown="$script_dir/teardown.sh"

failures=0
fail() {
  echo "FAIL: $1" >&2
  failures=$((failures + 1))
}

# A distinctive marker for the Doppler token, so a scenario can assert
# it never reached the stub `curl`'s argv -- only its stdin.
doppler_token_marker="teardown-test-doppler-token-MARKER-8f2c"

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

# Args: sandbox, topics-json-array (e.g. '["managed-by-willikins"]')
write_stub_gh() {
  local dir="$1" topics="$2"
  cat > "$dir/gh_get_response.json" <<JSON
{"topics": $topics}
JSON
  cat > "$dir/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
# Called either as `gh api repos/OWNER/NAME` (read) or
# `gh api -X DELETE repos/OWNER/NAME` (delete).
shift # drop "api"
method="GET"
if [ "${1:-}" = "-X" ]; then
  method="$2"
  shift 2
fi
path="${1:-}"
echo "GH_CALL method=$method path=$path" >> "$STUB_LOG"
if [ "$method" = "DELETE" ]; then
  echo "{}"
else
  cat "$GH_GET_RESPONSE_FILE"
fi
STUB
  chmod +x "$dir/bin/gh"
}

# Args: sandbox, description-or-empty
write_stub_curl() {
  local dir="$1" description="$2"
  cat > "$dir/curl_get_response.json" <<JSON
{"project": {"description": "$description"}}
JSON
  cat > "$dir/bin/curl" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
method="GET"
args=("$@")
for i in "${!args[@]}"; do
  if [ "${args[$i]}" = "-X" ]; then
    method="${args[$((i + 1))]}"
  fi
done
stdin_content="$(cat)"
{
  echo "CURL_CALL method=$method args=[${args[*]}]"
  echo "CURL_STDIN=[$stdin_content]"
} >> "$STUB_LOG"
if [ "$method" = "DELETE" ]; then
  echo '{"project": {}}'
else
  cat "$CURL_GET_RESPONSE_FILE"
fi
STUB
  chmod +x "$dir/bin/curl"
}

# Runs teardown.sh inside `dir`'s stubbed environment. Extra args (e.g.
# `--yes`) are forwarded.
run_teardown() {
  local dir="$1"
  shift
  PATH="$dir/bin:$PATH" \
    STUB_LOG="$dir/calls.log" \
    STUB_WILLIKINS_EXIT="${stub_willikins_exit:-0}" \
    RUN_JSON_FILE="$dir/run.json" \
    GH_GET_RESPONSE_FILE="$dir/gh_get_response.json" \
    CURL_GET_RESPONSE_FILE="$dir/curl_get_response.json" \
    WILLIKINS_DOPPLER_TOKEN="$doppler_token_marker" \
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
  write_stub_gh "$dir" '["managed-by-willikins"]'
  write_stub_curl "$dir" "managed-by: willikins"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" 2>&1) && status=0 || status=$?

  [ "$status" -eq 0 ] || fail "scenario1: expected exit 0, got $status; output: $output"
  echo "$output" | grep -q "would delete GitHub repository: Willikins-Test/teardown-test-repo" \
    || fail "scenario1: missing 'would delete' line for the repository"
  echo "$output" | grep -q "would delete Doppler project:   teardown-test-project" \
    || fail "scenario1: missing 'would delete' line for the project"
  grep -q "GH_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario1: gh DELETE must not be called on a dry run"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario1: curl DELETE must not be called on a dry run"
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
  write_stub_gh "$dir" '[]'
  write_stub_curl "$dir" "managed-by: willikins"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario2: expected a non-zero exit when the topic is missing"
  echo "$output" | grep -q "managed-by-willikins" \
    || fail "scenario2: refusal message should name the missing topic"
  grep -q "CURL_CALL" "$dir/calls.log" \
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
  write_stub_gh "$dir" '["managed-by-willikins"]'
  write_stub_curl "$dir" "some other description"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario3: expected a non-zero exit when the description is wrong"
  echo "$output" | grep -q "managed-by: willikins" \
    || fail "scenario3: refusal message should name the expected description"
  grep -q "GH_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario3: must never delete the repository once the Doppler check has refused"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    && fail "scenario3: must never delete the Doppler project once its own check has refused"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 4: both markers present, `--yes` -- actually calls both
# delete endpoints, and the Doppler token reached `curl` only via
# stdin, never as one of its command-line arguments.
# =======================================================================
scenario4() {
  local dir
  dir="$(new_sandbox)"
  write_run_record "$dir" "Willikins-Test/teardown-test-repo" "teardown-test-project"
  write_stub_willikins "$dir"
  write_stub_gh "$dir" '["managed-by-willikins"]'
  write_stub_curl "$dir" "managed-by: willikins"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -eq 0 ] || fail "scenario4: expected exit 0, got $status; output: $output"
  grep -q "GH_CALL method=DELETE path=repos/Willikins-Test/teardown-test-repo" "$dir/calls.log" \
    || fail "scenario4: gh DELETE was not called with the expected repository"
  grep -q "CURL_CALL method=DELETE" "$dir/calls.log" \
    || fail "scenario4: curl DELETE was not called for the Doppler project"
  if grep -q "CURL_CALL.*$doppler_token_marker" "$dir/calls.log"; then
    fail "scenario4: the Doppler token leaked into curl's own arguments"
  fi
  grep -q "CURL_STDIN=.*$doppler_token_marker" "$dir/calls.log" \
    || fail "scenario4: the Doppler token never reached curl's stdin (--config -)"
  rm -rf "$dir"
}

# =======================================================================
# Scenario 5: a run record with no `doppler` output -- refuses before
# calling gh or curl at all (never guesses a project name).
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
  write_stub_gh "$dir" '["managed-by-willikins"]'
  write_stub_curl "$dir" "managed-by: willikins"
  touch "$dir/calls.log"

  local output status
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?

  [ "$status" -ne 0 ] || fail "scenario5: expected a non-zero exit with no doppler output"
  if grep -qE "^(GH_CALL|CURL_CALL)" "$dir/calls.log"; then
    fail "scenario5: must not call gh or curl at all: $(cat "$dir/calls.log")"
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
  write_stub_gh "$dir" '["managed-by-willikins"]'
  write_stub_curl "$dir" "managed-by: willikins"
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
# That is not a run record, so the script must refuse and call neither
# provider -- the exit code alone can no longer tell it so.
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
  write_stub_gh "$dir" '["managed-by-willikins"]'
  write_stub_curl "$dir" "managed-by: willikins"
  touch "$dir/calls.log"

  local output status
  stub_willikins_exit=1
  output=$(run_teardown "$dir" --yes 2>&1) && status=0 || status=$?
  stub_willikins_exit=0

  [ "$status" -ne 0 ] || fail "scenario7: expected a non-zero exit for an unknown run"
  echo "$output" | grep -q "could not read run" \
    || fail "scenario7: refusal should say the run could not be read; output: $output"
  if grep -qE "^(GH_CALL|CURL_CALL)" "$dir/calls.log"; then
    fail "scenario7: must not call gh or curl at all: $(cat "$dir/calls.log")"
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

if [ "$failures" -eq 0 ]; then
  echo "teardown_test.sh: all scenarios passed"
  exit 0
else
  echo "teardown_test.sh: $failures scenario(s) failed"
  exit 1
fi
