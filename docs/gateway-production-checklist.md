# Gateway Production Checklist

Use this checklist before exposing a Flux deployment to real traffic.

## Network Exposure

- expose only the public gateway and operator surface
- keep internal execution and worker surfaces private
- terminate TLS at the edge or directly in front of Flux

## Authentication And Secrets

- set strong production secrets and API keys
- remove development defaults
- rotate credentials before beta onboarding
- review which routes require operator versus user auth

## Rate Limits And Abuse Controls

- enable sensible per-route rate limits
- protect expensive routes and agent/tool paths
- test error behavior under abusive traffic

## Execution Safety

- confirm timeout and memory limits are configured
- verify queue retry and dead-letter policies
- verify replay and debugging actions are operator-gated

## Observability

- confirm traces and mutation history are being recorded
- confirm logs are correlated with execution records
- verify that recent failures are visible through CLI or dashboard workflows

## Storage And Retention

- back up Postgres
- back up bundle artifacts if stored externally
- choose retention settings that still support debugging

## Deploy And Recovery

- document the deploy path
- verify rollback or re-deploy procedures
- rehearse database restore and service restart steps

## Product Readiness Check

Before onboarding real beta users, make sure a failed request can be:

1. found quickly
2. traced end to end
3. explained with linked state changes
4. tied back to a code version or deployment
