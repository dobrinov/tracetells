# Spec Delta

## MODIFIED Requirements

### Requirement: Old links keep working
Links to the pages before the redesign SHALL open the page that replaced them. This holds for both earlier layouts: the flat pages (`#/kpis`, `#/behaviour`) and the four areas (`#/home`, `#/understand/…`, `#/improve/…`, `#/setup/…`).

#### Scenario: A bookmarked KPIs page
- **WHEN** someone opens `#/kpis`
- **THEN** Goals opens, at `#/goals`

#### Scenario: A link to a page's story
- **WHEN** someone opens `#/understand/pages/%2Fcheckout`
- **THEN** the story of `/checkout` opens, at `#/map/pages/%2Fcheckout`

#### Scenario: Old areas
- **WHEN** someone opens `#/home`, `#/understand/audience`, `#/understand/errors`, `#/improve/releases` or `#/setup/privacy`
- **THEN** Pulse, Pulse, Problems, Work and Settings › Privacy open, each at its new address
