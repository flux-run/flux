# `flux why`

`flux why` is the product in one command.

It is the clearest expression of why Flux exists and why the system is intentionally complete.

## What `flux why` Does

Given a request, job, or execution ID, `flux why` produces a compact root-cause view that connects:

- what ran
- what failed or slowed down
- which version handled it
- what state changed
- what the likely causal chain is
- what the operator does next

This is the center of the product.

## Why It Matters

Most backend debugging starts with raw logs and many open tabs.

`flux why` feels different:

- one command
- one execution
- one explanation surface

That is what makes it the most memorable command in the product.

## Why It Is Shareable

`flux why` is inherently communicable because it compresses a debugging story into something small enough to paste into a ticket, issue, or chat message.

The command is shareable because it produces output that feels:

- surprisingly useful
- obviously better than grepping logs
- easy to share with another engineer

## Dependencies

`flux why` only works if the rest of Flux does its job:

- runtime request handling creates stable request identities
- runtime records execution metadata
- deployments are linked to requests
- database dispatch records mutations
- queue and async work remain attributable

That dependency chain is why Flux includes more than just functions.

## Success Condition

Users think:

- "I would rather start here than in logs"
- "I immediately understand what changed"
- "This is a different category of debugging tool"
