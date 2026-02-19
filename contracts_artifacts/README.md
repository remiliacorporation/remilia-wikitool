# Contracts Artifacts (Rust Rewrite Bootstrap)

This directory tracks frozen command/data contracts and baseline parity artifacts.

Current cutover policy:

- Database migrations are disabled during early Rust cutover.
- Operators should delete local DB state and repull content when schema/runtime changes.

## Contents

- `contracts/v1.json`: frozen behavioral contracts for the cutover.
- `baselines/offline-fixture-baseline.json`: Bun-vs-Rust parity baseline generated from fixture scans.

## Commands

```bash
# Generate Bun snapshot for a fixture root
bun scripts/contracts/snapshot-bun.ts --project-root tests/fixtures/full-refresh

# Run Bun-vs-Rust differential harness against committed baseline
bun scripts/contracts/differential-harness.ts

# Refresh baseline after intentional contract updates
bun scripts/contracts/differential-harness.ts --write-baseline
```
