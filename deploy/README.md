# Deploying the hosted Trace server

How `app.tracetells.com` runs: one Docker container behind a shared Caddy,
on a shared Postgres. The host's own layout (`/opt/infra/README.md` on the
server) is the reference; this follows it.

    /opt/infra/caddy/conf.d/trace.caddy    routing + TLS for app.tracetells.com
    /opt/infra/postgres/credentials/trace.env
    /opt/apps/trace/src/                   this repository
    /opt/apps/trace/.env                   TRACE_* settings (mode 600)
    /opt/apps/trace/compose.yaml           copy of deploy/compose.yaml
    /opt/apps/trace/deploy.sh              copy of deploy/deploy.sh

## First time

1. Database: a role and a database it owns.

       /opt/infra/new-project-db.sh trace    # role trace, database trace_production

2. App directory:

       mkdir -p /opt/apps/trace && cd /opt/apps/trace
       git clone https://github.com/dobrinov/tracetells src
       cp src/deploy/compose.yaml src/deploy/deploy.sh .
       . /opt/infra/postgres/credentials/trace.env
       umask 077
       cat > .env <<ENV
       TRACE_DB=postgres://$POSTGRES_USER:$POSTGRES_PASSWORD@$DB_HOST:$DB_PORT/$POSTGRES_DB
       TRACE_PUBLIC_URL=https://app.tracetells.com
       TRACE_CONTACT_URL=mailto:hello@tracetells.com
       TRACE_DEFAULT_PLAN=free
       ENV
       ./deploy.sh

   Add `ANTHROPIC_API_KEY=...` to turn on the Claude analyst (heuristics run
   without it), and `TRACE_ALLOW_SIGNUP=1` to let people create accounts from
   the sign-in page.

3. Routing:

       cp src/deploy/trace.caddy /opt/infra/caddy/conf.d/trace.caddy
       docker exec caddy caddy reload --config /etc/caddy/Caddyfile

   Caddy gets the certificate itself once `app.tracetells.com` points here.

4. First account: with no accounts yet, the server logs a one-time link.

       docker compose -f /opt/apps/trace/compose.yaml logs web | grep "first account"

   Open it and create the account; it becomes the owner. Operator rights
   (the Admin area) can only be granted here, and need two-step verification
   in the dashboard before the Admin area opens:

       docker exec trace-web trace operator grant you@example.com

## Day to day

    /opt/apps/trace/deploy.sh [branch]                       # redeploy (default: main)
    docker compose -f /opt/apps/trace/compose.yaml logs -f web
    docker exec trace-web trace --site SITE insights         # any CLI command; TRACE_DB comes from .env
    docker exec trace-web trace user reset-mfa --email E

The schema is created and upgraded when the server starts; several starting at
once take turns. Backups come from the host's nightly `backup.timer`, which
dumps every database (see `/opt/infra/README.md`).

## Before opening sign-ups

The hosted Free plan stores other people's visitor data, which makes the
operator their processor: publish terms, a privacy policy and a data
processing agreement (listing the host and, with the Claude analyst,
Anthropic as subprocessors) before setting `TRACE_ALLOW_SIGNUP=1`.
