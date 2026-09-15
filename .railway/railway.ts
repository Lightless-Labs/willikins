// Railway Infrastructure as Code for the `willikins` service (task 12, step C of
// docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md).
//
// Provenance. The shape below is what `railway config pull` (CLI 5.57.2, SDK
// `railway` 3.11.0) generated from the live project on 2026-09-15 after the first
// GitHub-triggered deployment succeeded, plus exactly one authored addition: the
// healthcheck. Everything else mirrors the live environment on purpose, because
// Railway IaC treats an omitted resource or field as an instruction to delete it,
// not as "leave it alone": one authoring file, one apply, omit means delete.
// A plan run against this file must therefore show the healthcheck and nothing
// else. If it shows a delete, the live environment moved since this file was
// generated; run `railway config pull --force`, re-add the healthcheck, and read
// the plan again before applying.
//
// Variables. `preserve()` keeps whatever value Railway already holds and never
// writes a value into this file. The five names are the server's own
// configuration (README, "Deploy"). The two provider credentials are absent by
// design: they reach Railway only through Doppler's Railway integration, never
// through this file or a paste.
//
// Source. `github(...)` mirrors the connection the operator made in the dashboard
// (repository Lightless-Labs/willikins, branch main, auto-deploy on push).
// Railway builds from the repository root `Dockerfile` (task 12, step B).
//
// Volume. `willikins-volume` was created once by `railway volume add` on
// 2026-09-15; its region and size are the live values, read back by the pull.
// Decreasing `sizeMB`, detaching the mount, or changing the region is destructive
// (the IaC reference's own words), so change none of them by hand.
//
// Domains. None, and no `domains` key at all: the operator deleted the service's
// public domain on 2026-09-15 and the service gets none until milestone 2c's
// OAuth lands (docs/HANDOFF.md, "RESUME HERE"). Generated Railway domains are
// never part of this file, so omitting the key removes nothing.
//
// Healthcheck. `/healthz` is served outside the allowed-hosts check, so Railway's
// probe (sent from `healthcheck.railway.app`, a host `WILLIKINS_ALLOWED_HOSTS`
// does not name) passes; pinned by
// crates/willikins-server/tests/deploy_host_headers.rs.
//
// Plan and apply from the repository root, with the SDK installed (`npm install`,
// see package.json): `railway config plan --verbose` is read-only and redacts
// variable values; `railway config apply` re-plans, marks destructive lines, and
// asks for confirmation. Never pass `--show-values` or `--decrypt-variables`.
import { defineRailway, github, preserve, project, service, volume } from "railway/iac";

export default defineRailway(() => {
  const willikinsVolume = volume("willikins-volume", {
    alerts: { usage: { "100": {}, "80": {}, "95": {} } },
    allowOnlineResize: true,
    region: "europe-west4-drams3a",
    sizeMB: 5000,
  });

  const willikins = service("willikins", {
    source: github("Lightless-Labs/willikins", { checkSuites: false }),
    replicas: { "europe-west4-drams3a": 1 },
    healthcheck: "/healthz",
    healthcheckTimeout: 30,
    volumeMounts: { "/data": willikinsVolume },
    env: {
      WILLIKINS_AGENT_TOKEN_HASHES: preserve(),
      WILLIKINS_ALLOWED_HOSTS: preserve(),
      WILLIKINS_APPROVER_TOKEN_HASH: preserve(),
      WILLIKINS_FAKE_CATALOG: preserve(),
      WILLIKINS_JOURNAL_PATH: preserve(),
    },
  });

  return project("Willikins", {
    resources: [willikins, willikinsVolume],
  });
});
