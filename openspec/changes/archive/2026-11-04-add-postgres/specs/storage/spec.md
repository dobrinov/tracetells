# Spec Delta

## Purpose

Where Trace keeps its data, and which databases a server can use.

## ADDED Requirements

### Requirement: SQLite by default, Postgres by URL
A server SHALL store its data in a SQLite file by default (`trace.db`, or the path given with `--db` / `TRACE_DB`). When `--db` / `TRACE_DB` is a `postgres://` or `postgresql://` URL, it SHALL store its data in that Postgres database instead. Every feature SHALL work the same on both.

#### Scenario: Self-hosting with no database server
- **WHEN** someone runs `trace serve` with no `--db`
- **THEN** Trace creates and uses `trace.db` next to it, and needs no other service

#### Scenario: A hosted server on Postgres
- **WHEN** an operator runs `trace serve` with `TRACE_DB=postgres://trace@localhost/trace` pointing at an empty database
- **THEN** Trace creates its tables there, and sign-in, collection, the dashboard, analysis, evaluation, MCP and the operator area all work

### Requirement: Schema upgrades on both databases
On start, Trace SHALL bring an existing database's schema up to date, on SQLite and on Postgres, without losing data.

#### Scenario: Restarting on an existing Postgres database
- **WHEN** a server restarts on a Postgres database that already has Trace's tables and data
- **THEN** the data is kept and nothing is created twice

### Requirement: Decimal values survive on Postgres
Values with decimals that may be empty (an event's value, a KPI's target, an evaluation's baseline, current value and p-value) SHALL be stored and read back exactly, whether or not earlier rows left them empty.

#### Scenario: An empty value, then a real one
- **WHEN** an event is stored without a value and another with the value 42.5
- **THEN** reading them back gives no value and 42.5

### Requirement: Choosing the database when installing
`trace init` SHALL ask where Trace keeps its data, explaining that SQLite suits trying Trace, development and small sites and that Postgres is recommended for production. It SHALL check the chosen database works, create Trace's tables and save the choice. It SHALL also run without prompts when given `--database` and `--db`.

#### Scenario: Installing for production
- **WHEN** someone runs `trace init`, chooses Postgres and gives a connection URL for an empty database
- **THEN** the tables are created, `trace.env` holds the URL, and `trace serve` in that folder uses Postgres without further flags

### Requirement: The settings file
`trace init` SHALL save the database as `TRACE_DB` in `trace.env`, readable only by its owner, and SHALL NOT replace a different saved database without confirmation or `--force`. Every `trace` command SHALL read `trace.env` from the folder it runs in, with real environment variables taking precedence.

#### Scenario: Environment variables win
- **WHEN** `trace.env` names a SQLite file and `TRACE_DB` is set to a Postgres URL in the environment
- **THEN** `trace serve` uses the Postgres URL

#### Scenario: A saved database isn't replaced by accident
- **WHEN** `trace.env` already names a SQLite file and someone runs `trace init --database postgres --db postgres://…` without `--force`
- **THEN** nothing is changed, no database is touched, and the message says how to replace it

### Requirement: Production advice and safe logging
`trace serve` SHALL log which database it uses without ever showing a password, and SHALL warn when a server reachable from outside runs on SQLite. The operator Admin area SHALL show the same advice when the server runs on SQLite. Errors about the database SHALL NOT include its password.

#### Scenario: SQLite on a public server
- **WHEN** `trace serve` runs on SQLite listening on `0.0.0.0` or behind a public URL
- **THEN** the log suggests moving to Postgres with `trace init --database postgres`

#### Scenario: A wrong password
- **WHEN** the Postgres server rejects the connection
- **THEN** the error names the server and database with the password replaced by `***`
