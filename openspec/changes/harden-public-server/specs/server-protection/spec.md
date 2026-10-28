# Spec Delta

## Purpose

Keep a Trace server that anyone on the internet can reach usable and safe: limit abuse, see real client addresses behind a proxy, protect the owner, and send security headers.

## ADDED Requirements

### Requirement: Collector rate limits
The server SHALL accept at most 600 collector batches a minute from one visitor address and 6,000 for one website, answering the rest with 429 without storing them. A batch the server rejects SHALL NOT count towards the account's monthly events.

#### Scenario: A flood from one address
- **WHEN** one address sends its 601st batch within a minute
- **THEN** that batch is refused with 429 before it is parsed

### Requirement: Sign-in limits
The server SHALL refuse sign-in for 15 minutes after 10 wrong passwords for one email from one address, 50 for one email from anywhere, or 30 from one address across emails. Other people SHALL still be able to sign in.

#### Scenario: Guessing one person's password
- **WHEN** 10 wrong passwords are tried for vic@acme.example
- **THEN** vic's next sign-in, even with the right password, is refused for 15 minutes, and eve can still sign in

#### Scenario: Spraying many emails
- **WHEN** one address tries wrong passwords for 30 different emails
- **THEN** sign-in from that address is refused for 15 minutes

### Requirement: Client addresses behind a proxy
The server SHALL use the last address in `X-Forwarded-For` as the client's address only when the connection comes from a private or loopback address, and the connection's own address otherwise.

#### Scenario: A client claims another address
- **WHEN** a client on the internet connects directly and sends `X-Forwarded-For: 6.6.6.6`
- **THEN** limits count the client's real address

### Requirement: The owner is protected
Only the owner SHALL be able to reset the owner's two-step verification, end the owner's sessions, change the owner's role or remove the owner.

#### Scenario: An admin tries to reset the owner
- **WHEN** an admin resets the owner's two-step verification
- **THEN** the request is refused and the owner stays signed in

### Requirement: Security headers
Every response SHALL carry `X-Content-Type-Options: nosniff`. Dashboard responses SHALL also forbid framing and send `Referrer-Policy: same-origin`, and dashboard pages a Content-Security-Policy that allows scripts only from the server and the page's own inline script by hash. The collector's responses SHALL NOT forbid being used from other sites.

#### Scenario: The dashboard can't be framed
- **WHEN** another site puts the dashboard in a frame
- **THEN** the browser refuses, because of `X-Frame-Options: DENY` and `frame-ancestors 'none'`
