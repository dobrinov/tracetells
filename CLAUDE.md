# Working on Trace

- **Don't run `cargo fmt`.** The code is formatted by hand, with lines up to about 160 characters, and rustfmt would rewrite most files. Match the style of the code around your change.
- **Tests run on both databases:** `cargo test --workspace` (SQLite), and again with `TRACE_TEST_POSTGRES=postgres://you@localhost/postgres` set (each test makes and drops its own database). CI runs both on every push, plus `cargo clippy --workspace --all-targets -- -D warnings -A clippy::type_complexity -A clippy::result_large_err`.
- **The dashboard and the collector are compiled into the binary** (`crates/trace-server/assets/`): rebuild before trying a change in a browser.
- **The collector is promised to stay small.** `the_collector_stays_small` in `crates/trace-server/src/api.rs` fails when it grows past the size the homepage and README state; raise the budget only together with those claims.
- **Migrations are additive.** `db::migrate` adds columns and tables on start and there are no down migrations, so the only rollback is a backup. Keep `schema.sql` (SQLite) and `structure.sql` (Postgres) in step; a test compares them.
- **Changes are OpenSpec changes** (`openspec/changes/`); `openspec/config.yaml` lists the constraints every change must respect, privacy first.
