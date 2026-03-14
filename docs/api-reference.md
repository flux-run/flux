# API Reference

This document describes the API surface at a route-group level.

The goal is to make the product surface legible to contributors, operators, and early adopters.

## Principles

- user traffic enters through the gateway
- operator traffic enters through the API
- debugging and execution-record routes are first-class
- route groups map cleanly to CLI workflows

## Route Groups

### System

- `GET /health`
- `GET /ready`
- `GET /metrics`

Purpose:

- health checks
- readiness probes
- operational metrics

### Functions And Deployments

- list functions
- inspect versions
- deploy new bundles
- compare or roll back versions

These routes power the deployment workflow and connect code versions to execution records.

### Traces And Debugging

- list traces
- inspect one trace
- generate `why` output
- query recent failures
- deep-dive debugging views

This is the heart of the operator API.

### Database And State

- inspect mutation history
- state blame and row history
- schema and migration status
- controlled database operations

These routes are what make the database part of the product rather than a hidden dependency.

### Queue, Schedule, And Events

- inspect queue state
- enqueue or publish work
- inspect retries and dead-letter history
- manage schedules

The goal is to preserve the same operator model for async work.

### Gateway And Routing

- inspect routes
- manage middleware
- inspect rate limits and policy state

This keeps the ingress surface visible to operators.

### Records And Monitoring

- request counts and summaries
- error and latency aggregations
- retention and pruning controls

These routes support ongoing operation of the system as a runtime, not just a deploy target.

## Documentation Rule

Every CLI workflow maps cleanly to an obvious API route group.
