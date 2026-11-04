# Spec Delta

## Purpose
Browser errors tell a team what broke for visitors on its website. Trace records the errors the website can fix and leaves out noise from outside it: browser extensions, ad and privacy blockers, and errors the browser hides.

## ADDED Requirements

### Requirement: Blocked requests to other sites are not errors
Trace SHALL NOT record a request, or a script, image, stylesheet or other resource load, that fails without an HTTP response when it goes to a different site than the website's (a host that is neither one of the website's hosts nor under the same domain). A failure with an HTTP status, and any failure on the website's own site and its subdomains, SHALL be recorded as before.

#### Scenario: Another site's analytics, refused by an ad blocker
- **WHEN** a visitor's ad blocker refuses `POST stats.example.org/api/event` on a page of `shop.example.com`
- **THEN** no error is recorded

#### Scenario: The website's own API on a subdomain
- **WHEN** `POST api.example.com/orders` fails without a response on a page of `shop.example.com`
- **THEN** the network error is recorded

#### Scenario: Another site answers with an error
- **WHEN** `GET payments.example.org/session` answers 500 on a page of `shop.example.com`
- **THEN** the network error is recorded

### Requirement: Errors from browser extensions are not recorded
Trace SHALL NOT record an error whose source, or the first frame of whose stack that names a file, is in a browser extension (`chrome-extension:`, `moz-extension:`, `safari-extension:` or `safari-web-extension:`). The collector SHALL NOT send an extension's address, and the server SHALL drop one that arrives.

#### Scenario: An extension's script throws
- **WHEN** a content script of an installed extension throws an uncaught exception on the page
- **THEN** no error is recorded, and nothing naming the extension leaves the browser

#### Scenario: An extension's promise is rejected
- **WHEN** a promise created in an extension's script is rejected and nothing handles it
- **THEN** no error is recorded

#### Scenario: The page's own error
- **WHEN** a script of the website throws an uncaught exception
- **THEN** the error is recorded with the script's file and line, as before

### Requirement: Errors the browser hides are not recorded
Trace SHALL NOT record an exception whose message is `Script error.` and which has no source, because the browser has hidden everything that would locate it.

#### Scenario: A cross-origin script without crossorigin
- **WHEN** a script loaded from another origin without the `crossorigin` attribute throws
- **THEN** no error is recorded
