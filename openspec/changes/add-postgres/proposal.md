# Proposal

This is a feature.

## Why

The hosted edition will run on a server that already has Postgres. SQLite serialises every write behind one writer and has no built-in replication, which limits a hosted server with many busy websites. Postgres removes both limits and brings standard backups and replication.

## What Changes

- `--db` / `TRACE_DB` accepts a `postgres://` URL as well as a SQLite file. SQLite stays the default for trying Trace and development; Postgres is recommended for production.
- `trace init` makes the choice part of installing: it asks, checks the connection, creates the tables and saves `TRACE_DB` to `trace.env`, which every command reads.
- Trace creates its tables on a fresh Postgres database (`structure.sql`, kept column-for-column in step with `schema.sql`) and adds new columns on upgrade, as it does for SQLite.
- Every query is written once, in SQL both databases understand, and runs through sqlx's `Any` driver.
- The test suite runs on either database; `TRACE_TEST_POSTGRES` gives each test a throwaway Postgres database.

## Capabilities

### New Capabilities
- `storage`: where Trace keeps its data and which databases it supports.

### Modified Capabilities
None.

## Privacy impact

None. The same data is stored; only the database engine can differ. Operators running Postgres are responsible for its access control and backups, as with the SQLite file.

## Site impact

None: the collector is unchanged.

## Impact

- `crates/trace-core/src/db.rs`: `connect` picks the database from the target; Postgres schema and backend-aware migration helpers; test-database helpers.
- All crates: `sqlx::SqlitePool` becomes `trace_core::Pool` (`AnyPool`), `?` placeholders become `$1…`, yes/no columns go through `db::Flag` (0/1 integers) and nullable decimals through `db::Real` (a workaround for an sqlx bug, see its comment).
- A few SQLite-only constructs are rewritten portably: `INSERT OR IGNORE`, scalar `MAX(a, b)`, `SUM(boolean)`, bare columns next to `MAX()`, unnamed subqueries, ambiguous upsert columns.
- `Cargo.toml`: sqlx (pinned to 0.8.6) gains the `any`, `postgres` and `tls-rustls` features; `sqlx-core` at the same version for `db::Real`.
- README: a Database section.
