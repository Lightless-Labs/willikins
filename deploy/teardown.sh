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
# Dry-run by default: prints what it would delete and exits 0 without
# calling either provider's delete endpoint. `--yes` deletes for real.
#
# Authentication: GitHub through `gh api`, which holds its own
# credential (never the fine-grained PAT `WILLIKINS_GITHUB_TOKEN` names
# for the server); Doppler through `curl`, with the token read from
# `WILLIKINS_DOPPLER_TOKEN` and passed to curl only via `--config -` on
# stdin (a `header = "Authorization: Bearer <token>"` directive), never
# as a command-line argument -- a token on argv would sit in `ps`
# output and any shell history for as long as the process runs.
#
# Exits non-zero, printing why, on any refusal: a missing argument, a
# run with no `repo` or `doppler` output, a resource that does not
# carry its ownership marker, or a provider call that fails outright.
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
doppler_api_base_url="${DOPPLER_API_BASE_URL:-https://api.doppler.com}"
managed_topic="managed-by-willikins"
managed_description="managed-by: willikins"

run_json=$("$willikins_bin" run "$run_id" --journal "$journal_path" --json) || {
  echo "teardown.sh: refusing -- could not read run $run_id from $journal_path" >&2
  exit 1
}

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

repo_json=$(gh api "repos/$repo") || {
  echo "teardown.sh: refusing -- could not read repository $repo via gh api" >&2
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
    | curl -sS --config - \
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
gh api -X DELETE "repos/$repo" > /dev/null

echo "teardown.sh: deleting Doppler project $project"
printf 'header = "Authorization: Bearer %s"\n' "$WILLIKINS_DOPPLER_TOKEN" \
  | curl -sS --config - -X DELETE "$doppler_api_base_url/v3/projects/project" \
      -H "Content-Type: application/json" \
      -d "{\"project\":\"$project\"}" \
  > /dev/null

echo "teardown.sh: done"
