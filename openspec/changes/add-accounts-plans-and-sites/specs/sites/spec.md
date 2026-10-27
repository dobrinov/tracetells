# Spec Delta

## Purpose

An account can monitor several websites. Each website is collected, configured, analysed and evaluated on its own, and every way of working with Diagnostic acts on one website at a time.

## ADDED Requirements

### Requirement: Websites are separate
Each website SHALL have its own collector write key, KPIs and event catalogue, privacy rules, behaviour data, ideas, evaluations and collection status. Data collected for one website SHALL never appear in another website's results.

#### Scenario: Two websites in one account
- **WHEN** Acme has the websites "shop" and "docs" and visitors use both
- **THEN** the shop's behaviour summary, KPIs and ideas contain only events collected with the shop's write key

#### Scenario: A privacy rule on one website
- **WHEN** an admin ignores `/admin/*` on "shop"
- **THEN** events on `/admin/*` are still collected for "docs"

### Requirement: Adding and removing websites
Admins SHALL be able to add a website by giving it a name, which produces a short identifier (slug) unique within the account and a new write key. Adding a website SHALL be refused when it would exceed the account's website limit. Admins SHALL be able to remove a website, which deletes its data after a confirmation that names the website.

#### Scenario: Adding a website
- **WHEN** an admin adds a website named "Docs site"
- **THEN** it appears in the website switcher with the identifier `docs-site` and an install snippet with its own write key

#### Scenario: At the website limit
- **WHEN** the account's plan allows 1 website and it already has one
- **THEN** adding another is refused with a message stating the limit

### Requirement: Choosing the website
The dashboard SHALL show which website is selected and let the person switch between the account's websites. API requests, `diagnostic push` and MCP tools that read or change website data SHALL name the website, and SHALL use the account's only website when the account has exactly one.

#### Scenario: Pushing configuration
- **WHEN** someone runs `diagnostic push --site docs` with a valid token
- **THEN** the KPIs and events in the file apply to "docs" only

#### Scenario: Ambiguous website
- **WHEN** an account has two websites and an MCP tool is called without a `site` argument
- **THEN** the tool returns an error that lists the account's websites so the agent can choose one

#### Scenario: Only one website
- **WHEN** an account has exactly one website and an MCP tool is called without a `site` argument
- **THEN** the tool acts on that website

### Requirement: Listing websites to agents
MCP SHALL offer a way to list the account's websites with their identifiers, addresses they collect from, and whether they are currently collecting.

#### Scenario: An agent starting work
- **WHEN** an agent lists websites with a token for Acme
- **THEN** it gets "shop" and "docs" with their identifiers and collection status, and nothing from other accounts
