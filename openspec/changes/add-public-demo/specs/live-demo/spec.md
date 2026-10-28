# Spec Delta

## Purpose

A public, live demo on the marketing site: visitors use the Stride demo shop, which has deliberate UX problems, and then look at what Trace recorded about the shop and about the marketing site itself.

## ADDED Requirements

### Requirement: The demo shop on the marketing site
The marketing site SHALL serve the Stride demo shop under `/demo/` as static files, built from the same shop source that `trace serve --demo-site` uses. Every page of the shop SHALL work when opened directly by its address, including product pages, the cart, checkout, pricing and sign-up. The shop SHALL say it is a fictional demo instrumented with Trace and that nothing is for sale.

#### Scenario: A deep link opens the right page
- **WHEN** a visitor opens `/demo/products/shoe-3` directly
- **THEN** the Cloud Daily product page shows

#### Scenario: The local demo still works
- **WHEN** someone runs `trace serve --demo-site`
- **THEN** the shop works at the root of that address as before

### Requirement: The planted problems still happen
The static shop SHALL keep every deliberate problem:
- product images that look clickable but do nothing;
- a "Pay now" button that takes 3 seconds with no feedback;
- an "Annual" toggle that throws an error;
- a sign-up form that wipes every field when the phone format is wrong;
- reviews that fail to load about 30% of the time as a failed network request with a console error.

#### Scenario: Reviews sometimes fail
- **WHEN** visitors open product pages many times
- **THEN** about 3 in 10 show "Reviews are unavailable right now", and Trace records a failed request and a console error for each

### Requirement: Both sites are instrumented
The marketing site and the demo shop SHALL each be a separate website in Trace, each with its own write key and `trace.json` in the repository. The marketing site's KPIs SHALL cover visits that reach the demo and clicks on "Start free". Instrumenting them SHALL NOT add anything beyond the standard collector snippet and `data-trace-*` attributes.

#### Scenario: Shop events go to the shop's website
- **WHEN** a visitor clicks around the demo shop
- **THEN** the events appear under the shop's website in Trace, not under the marketing site

### Requirement: The demo links to what Trace recorded
The marketing site SHALL link to the demo shop. The demo bar at the top of every shop page SHALL link to the shop's public view. The marketing site SHALL also link to its own public view. Until the hosted server address and public links are configured, the links SHALL point at visible placeholders and the collector SHALL NOT be loaded.

#### Scenario: From shop to insights
- **WHEN** a visitor clicks "see what Trace recorded" in the shop's demo bar
- **THEN** the shop's public view opens in a new tab

#### Scenario: Not configured yet
- **WHEN** the site is deployed without a Trace server address
- **THEN** no collector script is loaded and the demo links show placeholders instead of broken links
