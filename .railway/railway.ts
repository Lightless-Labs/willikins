// Railway Infrastructure as Code for the `willikins` service.
//
// Task 12, step C of
// docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md, authored
// from Railway's own IaC reference, fetched verbatim 2026-09-15:
//   https://docs.railway.com/infrastructure-as-code.md
//   https://docs.railway.com/infrastructure-as-code/reference.md
//
// *** THIS FILE IS AUTHORED, NOT YET APPLIED. ***
//
// The Railway CLI installed for this task (4.37.4) has no `railway
// config` subcommand at all, so nothing here has ever been run through
// `railway config apply` (or `railway config pull`/`plan`, which the
// same CLI also lacks). Applying it is the coordinator's and the
// operator's step, and only after re-reading it against the live
// project: Railway's Infrastructure as Code treats a field's *omission*
// as a deletion instruction, not as "leave it alone" -- applying a
// project definition that is missing something the live environment
// currently has will remove that thing. In particular, this file
// declares no `source` (see below) and no `domains`; applying it must
// not be allowed to interpret either omission as "remove the GitHub
// connection" or "remove a domain" if the live environment has grown
// one since this file was written.
//
// The volume: this task's own brief permitted running `railway volume
// add --mount-path /data` at most once, and only if `railway volume
// list` showed none. It showed none, so that command ran (2026-09-15)
// and Railway auto-named the result -- the installed CLI's `volume add`
// has no `--name` flag, so the name was never this task's to choose.
// `railway volume list` afterwards confirmed:
//   Volume: willikins-volume
//   Attached to: willikins
//   Mount path: /data
// The `volume(...)` resource below uses that exact name, per this
// task's own rule: a volume the CLI already created must be mirrored
// here by name, never declared as if it did not exist yet (which would
// read, on apply, as "create a second one" -- Railway does not de-
// duplicate by mount path).
//
// No `source` field: the `willikins` service is already a GitHub-
// sourced service the operator connected (repository
// `Lightless-Labs/willikins`, branch `main`, auto-deploy on push)
// through the dashboard, before this file existed. The reference's own
// words for this: "Omit `source` when `.railway/railway.ts` should
// manage service settings but not declare a GitHub repository or Docker
// image." This file manages exactly the three things task 12 asks it
// to (healthcheck, replicas, the volume mount) and leaves the
// dashboard's own GitHub connection alone.
//
// No `build`/`start` commands: those are the buildpack-style fields
// (`pnpm build` / `pnpm start`) the reference's own examples use for a
// service with no Dockerfile; this service builds from the repository
// root `Dockerfile` (task 12, step B) instead, which is not a field
// this version of the IaC reference documents at all -- there is
// nothing to author here without inventing a field the schema does not
// have. `railway status --json` showed the service's stored builder as
// `RAILPACK` as of 2026-09-15 (every deploy before this task's
// Dockerfile existed failed at Railpack's own "no start command
// detected" step, for a workspace with more than one binary target);
// Railway is documented to prefer a root `Dockerfile` automatically
// once one exists, so `railway up --ci` (task 12, step D) is the actual
// test of whether that holds here. If it does not, switching the
// service to the Dockerfile builder is a dashboard action outside this
// task's permitted commands, for the coordinator or the operator.
//
// No `domains`: the operator deleted the service's public domain on
// purpose (docs/HANDOFF.md, "RESUME HERE") and the milestone 2 plan's
// 2026-09-15 addendum decided the service gets none until a stronger
// auth path lands in milestone 3. The reference's own words: "Generated
// Railway service domains are not included in `.railway/railway.ts`."
// Omitting the field entirely (never `domains: []`) is what keeps this
// file from ever being read as "remove whatever domain exists" the one
// time that stops being true.
import { defineRailway, project, service, volume } from "railway/iac";

export default defineRailway(() => {
  // 5000 MB mirrors what `railway volume list` showed after `volume add`
  // created this volume on 2026-09-15 (the plan-tiered default; this
  // task's brief authorized creating the volume, not choosing its
  // size). The IaC reference says applying a *smaller* `sizeMB` than the
  // live volume is destructive -- re-check `railway volume list` before
  // ever changing this number, never just edit it to a round figure.
  const journal = volume("willikins-volume", {
    sizeMB: 5000,
  });

  const willikins = service("willikins", {
    healthcheck: "/healthz",
    healthcheckTimeout: 30,
    replicas: 1,
    volumeMounts: {
      "/data": journal,
    },
  });

  return project("Willikins", {
    resources: [willikins, journal],
  });
});
