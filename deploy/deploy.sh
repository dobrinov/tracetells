#!/bin/bash
# Redeploy Trace from GitHub. Usage: /opt/apps/trace/deploy.sh [branch]
# The server brings its database schema up to date when it starts.
set -euo pipefail

cd /opt/apps/trace
BRANCH="${1:-main}"

echo "==> fetching $BRANCH"
git -C src fetch --prune origin
git -C src checkout -q -B "$BRANCH" "origin/$BRANCH"
echo "    $(git -C src log --oneline -1)"

echo "==> building"
docker compose build

echo "==> starting"
docker compose up -d

echo "==> waiting for health"
for i in $(seq 1 60); do
  status=$(docker inspect -f '{{.State.Health.Status}}' trace-web 2>/dev/null || echo starting)
  [ "$status" = "healthy" ] && { echo "    healthy"; break; }
  [ "$status" = "unhealthy" ] && { echo "    UNHEALTHY — check: docker compose logs web"; exit 1; }
  sleep 3
done
[ "$status" = "healthy" ] || { echo "    not healthy after 3 minutes — check: docker compose logs web"; exit 1; }

docker image prune -f >/dev/null
echo "==> done"
