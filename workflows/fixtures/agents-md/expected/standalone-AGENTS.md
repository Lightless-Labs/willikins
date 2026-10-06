# sample-service

Conventions for every Example-Org repository:

- Commits land on `main`; there is no long-lived branch.
- Every secret lives in Doppler, never in a committed file.
- CI is Buildkite; a pipeline reads `.buildkite/pipeline.yml`.

Commands:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
