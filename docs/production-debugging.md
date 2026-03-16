# Production Debugging

## 1) Triage

```bash
flux status
flux logs --status error --limit 100
flux tail
```

## 2) Deep Inspect

```bash
flux trace <execution_id> --verbose
flux why <execution_id>
```

## 3) Safe Reproduction

```bash
flux replay <execution_id>
flux replay <execution_id> --diff
```

Use `--diff` to compare original and replay output fields.

## 4) Continue From Checkpoint

```bash
flux resume <execution_id>
flux resume <execution_id> --from 2
```

## 5) One-Off Local Probe

```bash
flux exec index.ts --payload '{"test":true}'
```

## Incident Outcome Standard

A good incident workflow yields:

- exact failing execution
- request/response visibility
- call-level checkpoint trail
- deterministic replay evidence
- clear next action
