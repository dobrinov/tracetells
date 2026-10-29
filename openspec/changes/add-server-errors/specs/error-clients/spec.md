# Spec Delta

## Purpose
Client libraries for Rust, Node.js and Ruby report a server's unhandled and reported errors to Trace, sending only error facts, scrubbed, without ever slowing down or breaking the application they run in.

## ADDED Requirements

### Requirement: Each client is configured the same way
Each client SHALL read its server key, endpoint, environment and release from explicit configuration or from `TRACE_SERVER_KEY`, `TRACE_ENDPOINT` (default `https://app.tracetells.com`), `TRACE_ENVIRONMENT` and `TRACE_RELEASE`. Without a server key it SHALL do nothing and send nothing. When no release is configured, it SHALL use a release the platform exposes (for example a git commit environment variable) or leave it empty.

#### Scenario: No key in development
- **WHEN** an application starts with the client installed and no `TRACE_SERVER_KEY`
- **THEN** errors are not sent anywhere, and the application behaves as without the client

### Requirement: Unhandled errors are captured automatically
Once installed, the Rust client SHALL capture panics and errors logged through `tracing` at error level, and errors raised in HTTP handlers wrapped by its middleware. The Node.js client SHALL capture uncaught exceptions and unhandled rejections, and errors raised in Express, Fastify and plain `http` handlers wrapped by its helpers, keeping the platform's default behaviour on crashes. The Ruby client SHALL capture exceptions raised through its Rack middleware and, in Rails, controller and background job errors through the framework's error reporter. Each client SHALL also offer a call to report a handled error.

#### Scenario: An exception in a Rails controller
- **WHEN** a Rails action raises `NoMethodError` and the request returns 500
- **THEN** one error is sent with the class, scrubbed message, backtrace, route `/orders/:id`, method `GET` and status 500

#### Scenario: A Node.js crash
- **WHEN** an uncaught exception reaches the Node.js process
- **THEN** the client tries to send it within two seconds, then lets the process exit as it would without the client

### Requirement: Clients send only error facts
Each client SHALL send only: class, message, backtrace frames (file relative to the project root, line, function, in-app flag), the route template (never the raw URL or query string), HTTP method and status, environment, release, runtime name and version, occurrence time and repeat count. Clients SHALL NOT send request or response bodies, parameters, headers, cookies, sessions, environment variables, local variables, source code, host names or user identifiers, and SHALL offer no option to add them. Clients SHALL scrub messages and frames with the same patterns as Trace's server before sending.

#### Scenario: A message with a token
- **WHEN** an error message contains `Bearer eyJhbGciOi...`
- **THEN** the client sends `Bearer [token]`

#### Scenario: A route with an id
- **WHEN** a request to `/users/42/settings` fails in a framework that knows the route `/users/:id/settings`
- **THEN** the route sent is `/users/:id/settings`; in one that doesn't, ids in the path are replaced before sending

### Requirement: Clients never harm the host application
Each client SHALL send from the background in batches, with a bounded queue and timeouts, and SHALL NOT block request handling. It SHALL swallow its own failures, drop errors when the queue is full or Trace is unreachable, and back off on 429 and 5xx responses. Repeats of the same error within a minute SHALL be sent as one error with a count. On graceful shutdown it SHALL try to flush within two seconds.

#### Scenario: Trace is down
- **WHEN** Trace answers 503 or the network is unreachable
- **THEN** requests are served as usual, the client drops what it can't send, and retries later with backoff

#### Scenario: An error storm
- **WHEN** the same error is raised 10,000 times in a minute
- **THEN** the client sends it as one error with a count of 10,000

### Requirement: Clients are documented and versioned with Trace
Each client SHALL live in this repository under `clients/`, under the MIT licence, with a README covering install, framework setup, configuration and exactly what it sends. Each SHALL have tests that fail if a forbidden field is sent.

#### Scenario: A forbidden field
- **WHEN** a test reports an error raised while handling a request with a body, cookies and an authorisation header
- **THEN** the test fails if the payload contains any of them
