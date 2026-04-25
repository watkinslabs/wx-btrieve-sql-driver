#!/usr/bin/env bash
# Bring up the Postgres test container the integration harness expects
# when run with BTR_TEST_BACKEND=postgres.
#
# Default: 127.0.0.1:15432, db=wxbtrv_test, user=postgres, password=WxTest2024
# Override via BTR_PG_HOST / BTR_PG_PORT / BTR_PG_USER / BTR_PG_PASS / BTR_PG_DB.
set -euo pipefail

NAME=wxbtrv-test-pg
PORT="${BTR_PG_PORT:-15432}"
PASS="${BTR_PG_PASS:-WxTest2024}"
DB="${BTR_PG_DB:-wxbtrv_test}"

if ! docker ps --format '{{.Names}}' | grep -q "^${NAME}$"; then
    if docker ps -a --format '{{.Names}}' | grep -q "^${NAME}$"; then
        docker start "$NAME" >/dev/null
    else
        docker run -d --name "$NAME" \
            -e POSTGRES_PASSWORD="$PASS" \
            -e POSTGRES_DB="$DB" \
            -p "$PORT:5432" \
            postgres:16-alpine >/dev/null
    fi
fi

echo "Waiting for Postgres on $PORT..."
for _ in $(seq 1 30); do
    if docker exec "$NAME" pg_isready -U postgres >/dev/null 2>&1; then
        echo "Postgres test container ready: $NAME on port $PORT, db $DB"
        exit 0
    fi
    sleep 1
done
echo "Postgres test container failed to become ready" >&2
exit 1
