# CLI Roadmap

This document exists to keep the CLI aligned with the product instead of letting it drift into a collection of unrelated commands.

## Core Rule

The CLI should make this loop feel inevitable:

```bash
flux init
flux dev
flux function create
flux invoke --gateway
flux trace
flux why
```

If that loop is confusing, the CLI is not aligned with the product.

## Design Principles

### One Obvious Path

New users should not have to guess:

- which command is current
- which ports matter
- which concepts are historical leftovers
- whether the local and remote workflows are different products

### Project-First

The current directory should be the primary context.

Remote contexts should exist, but they should feel like an extension of the product, not the main product.

### Debugging Commands Are The Hero Surface

The best commands in Flux should be:

- `trace`
- `why`
- `debug`
- replay and diff
- state and mutation inspection

Administrative commands are necessary, but they are not the reason the product exists.

### Calm Output

The CLI should prefer:

- short readable output
- actionable next-step hints
- useful defaults
- machine output only when requested

### Stable Nouns

Command naming should reinforce the current product model:

- current directory is the project
- Flux is self-hosted by default
- functions, queue, schedules, agents, and debugging are parts of one system

## Intended Command Groups

### Bootstrap

- `flux init`
- `flux doctor`
- `flux dev`

### Build And Ship

- `flux function create`
- `flux db push`
- `flux generate`
- `flux deploy`
- `flux invoke`

### Debug And Operate

- `flux tail`
- `flux errors`
- `flux trace`
- `flux why`
- `flux debug`
- `flux state`
- `flux incident`
- `flux bug bisect`

### Platform Surfaces

- `flux secrets`
- `flux config`
- `flux gateway`
- `flux queue`
- `flux schedule`
- `flux agent`

## Cleanup Priorities

The CLI should keep moving toward:

1. removing cloud-era terms from the main path
2. eliminating command aliases that no longer reflect the product
3. making config resolution obvious
4. making `flux dev` and `flux invoke` feel like the same local product
5. keeping help text and tests aligned with the current story

## Success Condition

The CLI is aligned when a developer spends a few minutes with it and comes away with this impression:

- the system feels complete
- the local workflow is obvious
- the debugging workflow is the center of gravity
- the command set feels designed, not accumulated
