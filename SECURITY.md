# Security policy

## Supported versions

`icmd` is pre-1.0. Only the latest published `0.x` release receives security
fixes; older `0.x` versions are supported only while a migration path to the
current release is being completed.

## Reporting a vulnerability

Do not open a public issue for a security problem. Report it privately through
the repository's security advisory form (GitHub "Report a vulnerability") or by
email to the maintainer address listed in `Cargo.toml`.

Please include:

- the affected version or revision;
- the smallest input that reproduces the problem;
- whether the issue is reachable from a public API, and which one;
- any observed memory, resource, or terminal-state impact.

You will receive an acknowledgement within a few working days. Please allow time
for a fix and a coordinated release before public disclosure.

## Scope

The project treats the following as security-relevant, matching the guarantees
the framework makes:

- memory-safety defects in the native raster backend, including any `unsafe`
  use or lifetime error around the Chafa FFI boundary;
- resource exhaustion reachable from ordinary public input — trees, images,
  caches, in-flight work, or emitted output exceeding their configured budgets;
- escape-sequence or control-character injection through cell text, image
  paths, or terminal protocol payloads;
- silent failure modes that lose data or strand the terminal, such as a worker
  that panics without reporting, or shutdown that leaves raw mode or the
  alternate screen active.

## Hardening posture

The following controls are in place and are covered by tests in this repository:

- Frame validation is transactional: an invalid operation batch is rejected
  before any renderer state is mutated.
- Every potentially unbounded unit of work has a validated ceiling in
  `ResourceLimits`, checked before allocation or recursion.
- Pipeline stages are named, joinable threads; a stage panic or a shutdown
  timeout is reported as a typed error.
- The runtime uses no `unsafe impl Send`; the only `unsafe` blocks are in the
  `native-raster` feature, which can be disabled entirely.
- CRC, dependency, packaging-content, and feature-matrix gates run in CI, with
  Miri and ThreadSanitizer on a schedule.
