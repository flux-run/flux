# Example: Todo API

The Todo API is the simplest useful example of Flux.

It shows that Flux is not only for advanced AI or workflow-heavy systems. It also feels great for ordinary application backends.

## What The Example Covers

- HTTP routes through runtime request handling
- simple CRUD functions
- database schema and mutations
- execution records for normal requests
- row history and state blame on ordinary app data

## Why This Example Matters

If Flux cannot make a plain CRUD backend feel better to build and debug, the rest of the product story gets weaker.

The Todo API demonstrates:

- fast local setup
- clear function layout
- easy schema evolution
- request traces with linked state changes
- useful debugging for common mistakes

## Good Demo Flow

```bash
flux init todo-api
flux dev
flux function create create_todo
flux function create complete_todo
flux invoke create_todo --payload '{"title":"ship beta"}'
flux trace
flux why <request_id>
flux state history todos --id <todo_id>
```

## What A Reader Should Learn

This example shows:

- how a normal backend fits into Flux
- how database changes become part of debugging
- why the execution record matters even for simple product code
