#!/usr/bin/env bash
# Tear down what one applied run of the positive fixture created: the
# GitHub repository and the Doppler project. Task 12, step E (used by
# task 14's live smoke run).
#
# Usage: teardown.sh <run-id> <journal-path> [--yes]
#
# Identifies the two resources from `willikins run <run-id> --journal
# <journal-path> --json` -- specifically the `repo` node's `repo` output
# and the `doppler` node's `project` output, never re-derived from a
# project slug (a run's own recorded outputs are the only source of
# truth for what it actually created; a slug alone cannot tell a caller
# whether the run got as far as creating anything at all).
#
# Refuses to delete either resource unless it still carries willikins'
# own ownership marker -- the GitHub repository topic `managed-by-
# willikins`, the Doppler project description `managed-by: willikins`
# (`crates/willikins-providers-github/src/lib.rs`'s `MANAGED_TOPIC`,
# `crates/willikins-providers-doppler/src/client.rs`'s
# `MANAGED_DESCRIPTION`) -- so a resource whose marker was removed, or a
# name that happens to collide with something willikins never created,
# is left alone.
#
# Works on a failed run as well as a succeeded one: a run that stopped
# part way is the run most likely to have left something behind.
#
# Dry-run by default: prints what it would delete and exits 0 without
# calling either provider's delete endpoint. `--yes` deletes for real.
#
# Authentication: both providers through `curl`, each with its own
# fine-grained credential -- `WILLIKINS_GITHUB_TOKEN` for GitHub,
# `WILLIKINS_DOPPLER_TOKEN` for Doppler, the same two variables the
# server itself reads for live mode. Neither token is ever passed to
# `curl` as a command-line argument: each is written to a `header =
# "Authorization: Bearer <token>"` directive and piped to `curl --config
# -` on stdin, so it never sits in `ps` output or shell history for as
# long as the process runs. The GitHub token is a sandbox fine-grained
# PAT scoped to the throwaway `Willikins-Test` organization, holding
# repository administration there -- the same credential and scope
# `crates/willikins-providers-github/tests/live_write_cycle.rs` already
# uses to delete a repository through `Http::delete`, so deleting one
# here needs nothing new of it. GitHub's REST API also requires an
# `Accept` and a `User-Agent` header on every request; this script sends
# the same three headers `crates/willikins-providers-github/src/
# client.rs`'s `default_headers` does.
#
# Exits non-zero, printing why, on any refusal: a missing argument, an
# unset credential, a run with no `repo` or `doppler` output, a resource
# that does not carry its ownership marker, or a provider call that
# fails outright. Every `curl` call carries `--fail`: without it curl
# exits 0 on a 403 or a 500, and the script would report a delete that
# never happened.
set -euo pipefail

usage() {
  echo "usage: teardown.sh <run-id> <journal-path> [--yes]" >&2
}

if [ "$#" -lt 2 ]; then
  usage
  exit 2
fi

run_id="$1"
journal_path="$2"
shift 2

yes=0
for arg in "$@"; do
  case "$arg" in
    --yes)
      yes=1
      ;;
    *)
      echo "teardown.sh: unknown argument: $arg" >&2
      usage
      exit 2
      ;;
  esac
done

willikins_bin="${WILLIKINS_BIN:-willikins}"
github_api_base_url="${GITHUB_API_BASE_URL:-https://api.github.com}"
doppler_api_base_url="${DOPPLER_API_BASE_URL:-https://api.doppler.com}"
managed_topic="managed-by-willikins"
managed_description="managed-by: willikins"

# `willikins run --json` prints the whole run record and *then* exits 1
# for a run whose state is `failed` or `running`
# (`exit_for_run_state` in crates/willikins-cli/src/commands.rs). A
# failed run is exactly the run this script exists for -- it is the one
# that leaves a repository or a project behind -- so its exit code
# cannot stand in for "the run could not be read".
#
# Decide from the document instead: a run record carries `run_id`; the
# CLI's own error documents (`UnknownRun`, an unreadable journal) carry
# `kind` and `message` and no `run_id`.
run_json=$("$willikins_bin" run "$run_id" --journal "$journal_path" --json) || true
if ! printf '%s' "$run_json" | jq -e 'has("run_id")' > /dev/null 2>&1; then
  echo "teardown.sh: refusing -- could not read run $run_id from $journal_path" >&2
  exit 1
fi

repo=$(printf '%s' "$run_json" \
  | jq -r '.nodes[]? | select(.node == "repo") | .outputs.repo.value // empty')
project=$(printf '%s' "$run_json" \
  | jq -r '.nodes[]? | select(.node == "doppler") | .outputs.project.value // empty')

if [ -z "$repo" ]; then
  echo "teardown.sh: refusing -- run $run_id has no repo output; it may never have" \
    "reached that step" >&2
  exit 1
fi
if [ -z "$project" ]; then
  echo "teardown.sh: refusing -- run $run_id has no doppler project output; it may" \
    "never have reached that step" >&2
  exit 1
fi

echo "teardown.sh: run $run_id created repository '$repo' and Doppler project '$project'"

# --- GitHub: read first, refuse unless the marker is still there. -----

: "${WILLIKINS_GITHUB_TOKEN:?teardown.sh: WILLIKINS_GITHUB_TOKEN must be set}"

# The three headers GitHub's docs require on every request, mirroring
# `crates/willikins-providers-github/src/client.rs`'s `default_headers`:
# `Accept`, `X-GitHub-Api-Version`, and a `User-Agent` (GitHub rejects a
# request with none at all). The `Authorization` directive travels with
# them on the same `--config -` stdin, never on argv.
github_curl_config() {
  printf 'header = "Authorization: Bearer %s"\n' "$WILLIKINS_GITHUB_TOKEN"
  printf 'header = "Accept: application/vnd.github+json"\n'
  printf 'header = "X-GitHub-Api-Version: 2022-11-28"\n'
  printf 'header = "User-Agent: willikins-teardown"\n'
}

repo_json=$(github_curl_config | curl -sS --fail --config - "$github_api_base_url/repos/$repo") || {
  echo "teardown.sh: refusing -- could not read repository $repo via the GitHub API" >&2
  exit 1
}
if ! printf '%s' "$repo_json" \
  | jq -e --arg topic "$managed_topic" '(.topics // []) | index($topic) != null' \
  > /dev/null; then
  echo "teardown.sh: refusing -- repository $repo does not carry the '$managed_topic'" \
    "topic; not deleting a repository willikins may not own" >&2
  exit 1
fi

# --- Doppler: same rule, same order. -----------------------------------

: "${WILLIKINS_DOPPLER_TOKEN:?teardown.sh: WILLIKINS_DOPPLER_TOKEN must be set}"

doppler_json=$(
  printf 'header = "Authorization: Bearer %s"\n' "$WILLIKINS_DOPPLER_TOKEN" \
    | curl -sS --fail --config - \
        "$doppler_api_base_url/v3/projects/project?project=$project"
) || {
  echo "teardown.sh: refusing -- could not read Doppler project $project" >&2
  exit 1
}
project_description=$(printf '%s' "$doppler_json" | jq -r '.project.description // empty')
if [ "$project_description" != "$managed_description" ]; then
  echo "teardown.sh: refusing -- Doppler project $project does not carry the" \
    "'$managed_description' description; not deleting a project willikins may not own" >&2
  exit 1
fi

# --- Both ownership checks passed. --------------------------------------

if [ "$yes" -ne 1 ]; then
  echo "teardown.sh: dry run (pass --yes to actually delete)"
  echo "  would delete GitHub repository: $repo"
  echo "  would delete Doppler project:   $project"
  exit 0
fi

echo "teardown.sh: deleting GitHub repository $repo"
github_curl_config \
  | curl -sS --fail --config - -X DELETE "$github_api_base_url/repos/$repo" \
  > /dev/null

echo "teardown.sh: deleting Doppler project $project"
printf 'header = "Authorization: Bearer %s"\n' "$WILLIKINS_DOPPLER_TOKEN" \
  | curl -sS --fail --config - -X DELETE "$doppler_api_base_url/v3/projects/project" \
      -H "Content-Type: application/json" \
      -d "{\"project\":\"$project\"}" \
  > /dev/null

echo "teardown.sh: done"
