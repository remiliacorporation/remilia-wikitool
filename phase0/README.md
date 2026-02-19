# Phase 0 (Rust Rewrite Bootstrap)

This directory tracks the frozen contracts and baseline artifacts for the Rust rewrite bootstrap.

## Contents

- `contracts/v1.json`: frozen behavioral contracts for Phase 0.
- `baselines/offline-fixture-baseline.json`: Bun-vs-Rust parity baseline generated from fixture scans.

## Commands

```bash
# Generate Bun snapshot for a fixture root
bun scripts/phase0/snapshot-bun.ts --project-root tests/fixtures/full-refresh

# Run Bun-vs-Rust differential harness against committed baseline
bun scripts/phase0/differential-harness.ts

# Refresh baseline after intentional contract updates
bun scripts/phase0/differential-harness.ts --write-baseline
```
