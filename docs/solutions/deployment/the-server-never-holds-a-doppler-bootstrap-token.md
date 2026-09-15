---
title: "The willikins-server process never holds a Doppler bootstrap token"
category: deployment
tags: [railway, doppler, credentials, secrets, blast-radius, task-12]
module: willikins-server
symptom: "A naive deployment reads WILLIKINS_GITHUB_TOKEN and WILLIKINS_DOPPLER_TOKEN from a Doppler bootstrap token baked into the image or set by hand, which is one more long-lived secret to rotate and one more place a leak can start from"
root_cause: "Doppler's own Railway integration writes both values straight into the service's environment; the server reads them as plain environment variables and never talks to Doppler's own API to fetch them, so no bootstrap credential exists to leak"
date: 2026-09-15
---

# The server never holds a Doppler bootstrap token

## Symptom

A common pattern for a service that reads secrets from Doppler is: give the service one
Doppler token (a "bootstrap" token), and have the service call Doppler's API at startup to
fetch its real secrets. This pattern adds a credential that is not one of the service's own
two provisioning credentials. Someone must store it, rotate it, and answer for it if it
leaks. It also makes the service a live Doppler client, with its own retry and error-
handling surface, before it has done any real work.

Willikins does not do this. `willikins-server` never calls Doppler to fetch its own
configuration. It has no code path that could.

## Root cause

The Railway service (`willikins`) never holds a Doppler bootstrap token, because Doppler's
own Railway integration writes `WILLIKINS_GITHUB_TOKEN` and `WILLIKINS_DOPPLER_TOKEN`
directly into the service's environment. From the server's point of view, both values
arrive exactly like `PORT` or `WILLIKINS_JOURNAL_PATH`: a plain environment variable, read
once at startup by `ServerConfig::from_vars` (`WILLIKINS_JOURNAL_PATH` and friends) and each
provider crate's own `credential_from_env` (`WILLIKINS_GITHUB_TOKEN`,
`WILLIKINS_DOPPLER_TOKEN`). Neither crate makes an HTTP request to Doppler to obtain either
value.

## How the two credentials actually arrive

1. The operator sets up Doppler's Railway integration once, in the Railway dashboard,
   pointed at the Doppler config that holds the real `WILLIKINS_GITHUB_TOKEN` and
   `WILLIKINS_DOPPLER_TOKEN` values.
2. Doppler pushes both values into the `willikins` service's Railway environment. Railway,
   not the server, holds the connection to Doppler.
3. `willikins-server` starts and reads both values with `std::env::var`, the same call it
   uses for every other configuration variable.
4. When either credential changes in Doppler, the integration updates the Railway
   environment and Railway redeploys the service. The server never re-fetches anything; it
   is simply started again with new values.

Rotating a credential is: change it in Doppler. Nothing on the willikins side, and no
bootstrap token, is ever touched.

## What a compromised host reaches

The design plan's "Credential blast radius, recorded" already states the ceiling: the
server holds one GitHub credential and one Doppler credential for one org and one
workplace, so a compromise of the process reaches everything those two credentials reach.
This entry adds the one fact that ceiling depends on: there is no *third* credential (a
Doppler bootstrap token, an API key for the integration itself) sitting in the same
process for an attacker to find. The blast radius is exactly the two provisioning
credentials the plan already accounts for, never one credential more.

## Where this would break

If `willikins-server` ever grows a feature that calls Doppler's own API to read its *own*
configuration (as opposed to provisioning a project on a caller's behalf, which is what
`willikins-providers-doppler` already does with `WILLIKINS_DOPPLER_TOKEN`), this note is
the one to update, and the new bootstrap credential's blast radius needs its own line in
the design plan's trust boundaries.
