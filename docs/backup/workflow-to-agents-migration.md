# Workflow To Agents Migration

Flux supports step-based orchestration, but the product does not treat "workflow" as the headline.

The stronger framing is:

- deterministic runtime for backend work
- queue and schedules for durable async execution
- agents for reasoning-heavy or tool-heavy flows

## Product Stance

Workflows are useful as an implementation pattern.

They do not dominate the product message because Flux is not trying to be only a workflow engine. It is a complete backend runtime with a strong debugging model.

## When To Use Queue And Schedules

Use queue and schedules when the work is:

- durable
- retryable
- mostly deterministic
- important to operate with normal backend tooling

Examples:

- webhook processing
- email sending
- invoice generation
- nightly maintenance jobs

## When To Use Agents

Use agents when the work is:

- reasoning-heavy
- tool-driven
- partially open-ended
- still important to trace and audit

Examples:

- support triage
- retrieval plus tool invocation
- operator copilots
- AI-assisted backoffice flows

## Migration Principle

When an existing workflow system is brought into Flux, the migration preserves:

- execution identity
- state attribution
- retry visibility
- operator debugging surfaces

The important question is not whether something is called a workflow or an agent. The important question is whether Flux can explain it as part of the same execution record.
