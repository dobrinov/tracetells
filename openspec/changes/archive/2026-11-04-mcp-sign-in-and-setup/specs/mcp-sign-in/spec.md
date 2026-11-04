# Spec Delta

## Purpose

Let a person connect an MCP client to Trace by signing in and approving it, with no token to copy.

## ADDED Requirements

### Requirement: Discoverable sign-in
A request to `/mcp` without a valid token SHALL get 401 with a `WWW-Authenticate` header naming the protected-resource metadata, which SHALL name this server as the authorization server. The server metadata SHALL offer dynamic registration, the authorization-code grant and PKCE with S256 only.

#### Scenario: Claude Code adds the server
- **WHEN** Claude Code calls `/mcp` without a token
- **THEN** it finds the authorization server through the metadata, registers, and opens the authorize URL in the browser

### Requirement: Approval by a signed-in person
Authorizing SHALL show the person, once signed in, which app is asking and where approval sends them back, and SHALL issue nothing until they approve. An unknown client or unregistered redirect address SHALL be shown as an error and never redirected to. Only a signed-in person, not a token, SHALL approve.

#### Scenario: A wrong redirect address
- **WHEN** the authorize URL names a redirect address the client didn't register
- **THEN** Trace shows an error and doesn't redirect

### Requirement: One-time, bound codes
A code SHALL be usable once, within 10 minutes, by the client it was issued to, with the same redirect address and a verifier that matches the PKCE challenge. Any failed attempt SHALL use the code up. A redeemed code SHALL become an API token for the approving person, read-only if they chose so, and named after the app.

#### Scenario: A replayed code
- **WHEN** a code that was already traded for a token is sent again
- **THEN** the token endpoint answers `invalid_grant`
