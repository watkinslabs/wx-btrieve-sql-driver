#!/usr/bin/env bash
# Repeatable test harness bootstrap.
# 1. Ensure docker'd SQL Server is running
# 2. Wait for it to accept connections
# 3. Drop and recreate WXBTRV_TEST from fixtures/schema.sql
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
sqlcmd=/opt/mssql-tools/bin/sqlcmd
pass='WxTest!2024'

if ! docker ps --format '{{.Names}}' | grep -q '^wxbtrv-test-sql$'; then
    echo "Starting wxbtrv-test-sql container..."
    docker start wxbtrv-test-sql >/dev/null 2>&1 || {
        echo "Container 'wxbtrv-test-sql' does not exist. Create it first with:"
        echo "  docker run -d --name wxbtrv-test-sql \\"
        echo "    -e 'ACCEPT_EULA=Y' -e 'MSSQL_SA_PASSWORD=$pass' \\"
        echo "    -p 1433:1433 mcr.microsoft.com/mssql/server:2022-latest"
        exit 1
    }
fi

echo "Waiting for SQL Server to accept connections..."
for _ in $(seq 1 30); do
    if "$sqlcmd" -S localhost -U sa -P "$pass" -C -Q "SELECT 1" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

echo "Applying fixtures/schema.sql..."
"$sqlcmd" -S localhost -U sa -P "$pass" -C \
    -i "$here/crates/btr-test-harness/fixtures/schema.sql"

echo "Fixture ready: WXBTRV_TEST.TEST_CUST (10 seed rows)"
