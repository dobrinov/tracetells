# Tasks

## 1. Storage and step states

- [x] 1.1 Add `privacy_reviewed_at` and `privacy_reviewed_by` to `projects` (schema and `db::migrate`)
- [x] 1.2 Add `onboarding::checklist(pool, site)` in `trace-core`, deriving website, install, privacy, KPIs, agent and team, with a unit test covering a fresh website, each signal turning its step done, and the team step not blocking completion

## 2. API

- [x] 2.1 Add `GET /api/onboarding` and `POST /api/onboarding/privacy-reviewed` (site-scoped; POST needs admin and writes `onboarding.privacy_reviewed` to the activity log)
- [x] 2.2 Test the routes: a user can read but not confirm, an admin can confirm, another account's website gives 404, and confirming one website leaves the others unchanged

## 3. Dashboard

- [x] 3.1 Add the Get started page (`#/start`) with the six steps, sharing Setup's snippet builders, the live install status, and "copy as a prompt for my agent"
- [x] 3.2 Show "Get started" with its progress in the nav until complete; land on it for new accounts and websites without data; link to it from Setup
- [x] 3.3 Verify in the browser with a fresh sign-up: each step ticks off as its signal appears, and the nav item disappears when the required steps are done

## 4. Docs

- [x] 4.1 Add a "First run" section to the README describing the checklist
