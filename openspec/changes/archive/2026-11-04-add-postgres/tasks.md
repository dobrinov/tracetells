# Tasks

## 1. Database layer

- [x] 1.1 Switch to sqlx's `Any` driver (`trace_core::Pool`) and pick SQLite or Postgres from `--db` / `TRACE_DB`
- [x] 1.2 Add `structure.sql`, mirror of `schema.sql`, with a test that both define the same columns
- [x] 1.3 Make migrations backend-aware (`has_table`, `has_column`, column types); keep the legacy SQLite-only upgrades for SQLite
- [x] 1.4 Bind nullable decimals as `db::Real`, which works around sqlx 0.8.6/0.9.0 sending a NULL f64 to Postgres as f32 (the statement cache stays on), with a single-connection regression test that fails if sqlx changes that mapping
- [x] 1.5 Take a Postgres advisory lock while creating or upgrading the schema, so several servers can start at once; retry SQLite setup while the file is busy
- [x] 1.6 Encode SQLite file paths for sqlx's URL parser; keep `:memory:` on one never-closed connection
- [x] 1.7 Fix `evaluations.insight_id` (a foreign key, not an identity column) in `structure.sql`, with a migration for existing Postgres databases and a test that only `id` columns are identities

## 2. Queries

- [x] 2.1 Rewrite `?` placeholders to `$1…`, keeping existing numbered ones
- [x] 2.2 Read and write yes/no columns as 0/1 integers through `db::Flag`
- [x] 2.3 Replace SQLite-only SQL: `INSERT OR IGNORE`, scalar `MAX`, `SUM(boolean)`, bare columns next to `MAX()`, unnamed subqueries, ambiguous upsert columns

## 3. Tests and docs

- [x] 3.1 Add `db::test_target` / `db::drop_test_target`, and run the whole suite on SQLite and on Postgres (`TRACE_TEST_POSTGRES`)
- [x] 3.2 Run the demo, analysis, evaluation and every dashboard API endpoint against a Postgres server
- [x] 3.3 README: a Database section; `--db` help mentions Postgres

## 4. Installation

- [x] 4.1 Add `trace init`: choose SQLite or Postgres (prompted, or `--database`/`--db`), check the connection, create the tables and save `TRACE_DB` to `trace.env` (mode 600), refusing to replace a different saved database without confirmation or `--force`
- [x] 4.2 Read `trace.env` (or `TRACE_ENV_FILE`) before parsing arguments, without overriding real environment variables
- [x] 4.3 Log the database in use with the password hidden (`db::describe`), in startup logs and connection errors; warn when SQLite backs a server reachable from outside; show the advice in the operator Admin area
- [x] 4.4 Tests for settings-file parsing and updating, the SQLite warning and password hiding; try `trace init` by hand (prompts, `--force`, wrong password) and `trace serve` reading `trace.env`
- [x] 4.5 README: an "Install on a server" section; pricing page copy
