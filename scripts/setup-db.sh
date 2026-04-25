#!/usr/bin/env bash
# Initialise wxbtrv.db on the target Windows machine and import INT files.
#
# Usage:
#   BTR_DEPLOY_PASS='...' ./scripts/setup-db.sh [host]
#
# Environment variables:
#   BTR_DEPLOY_HOST           — target host                    (default: 10.0.10.10)
#   BTR_DEPLOY_USER           — SSH user                       (default: remote dos)
#   BTR_DEPLOY_PASS           — SSH password                   (required)
#   BTR_SQL_SERVER            — SQL Server IP/hostname         (default: 10.0.0.232)
#   BTR_SQL_DATABASE          — default session database       (optional)
#   BTR_SQL_SCHEMA            — default schema                 (optional, default: dbo)
#   BTR_SQL_USER              — SQL Server login               (e.g. adv)
#   BTR_SQL_PASS              — SQL Server password            (e.g. 11234)
#   BTR_SQL_TRUSTED           — use Windows auth (yes/no)      (optional, default: no)
#   BTR_SQL_ENCRYPT           — encrypt connection (yes/no)    (optional, default: no)
#   BTR_SQL_TRUST_CERT        — trust server TLS cert (yes/no) (optional, default: no)
#
# What this script does:
#   1. Creates a fresh wxbtrv.db via db_config init
#   2. Imports INT files per directory with their default db/schema
#   3. Sets SQL Server connection including all ODBC parameters

set -euo pipefail

HOST="${1:-${BTR_DEPLOY_HOST:-10.0.10.10}}"
USER="${BTR_DEPLOY_USER:-remote dos}"
PASS="${BTR_DEPLOY_PASS:?BTR_DEPLOY_PASS not set}"
SQL_SERVER="${BTR_SQL_SERVER:-10.0.0.232}"
SQL_USER="${BTR_SQL_USER:-}"
SQL_PASS="${BTR_SQL_PASS:-}"
SQL_DATABASE="${BTR_SQL_DATABASE:-}"
SQL_SCHEMA="${BTR_SQL_SCHEMA:-}"
SQL_TRUSTED="${BTR_SQL_TRUSTED:-no}"
SQL_ENCRYPT="${BTR_SQL_ENCRYPT:-no}"
SQL_TRUST_CERT="${BTR_SQL_TRUST_CERT:-yes}"

ssh_run() {
    sshpass -p "$PASS" ssh -o StrictHostKeyChecking=no "$USER@$HOST" "$@"
}

DB_CONFIG='C:\WatkinsX\bin\db_config.exe --db C:\WatkinsX\config\wxbtrv.db'

echo "=== Setting up wxbtrv.db on $HOST ==="

# ── Step 1: Init database ──────────────────────────────────────────────────────
echo ""
echo "[1/5] Clearing existing wxbtrv.db..."
ssh_run "powershell -Command \"Remove-Item -Force 'C:\WatkinsX\config\wxbtrv.db' -ErrorAction SilentlyContinue; Write-Host 'cleared'\""

echo "      Initialising fresh wxbtrv.db..."
ssh_run "powershell -Command \"$DB_CONFIG init\""

# ── Step 2: Import G:\pacific ─────────────────────────────────────────────────
echo ""
echo "[2/5] Importing G:\\pacific (db=GPacific schema=dbo)..."
ssh_run "powershell -Command \"& { $DB_CONFIG import-int --db-name GPacific --schema dbo 'G:\pacific'; Write-Host 'done' }\""

# ── Step 3: Import G:\canada ──────────────────────────────────────────────────
echo ""
echo "[3/5] Importing G:\\canada (db=GCanada schema=dbo)..."
ssh_run "powershell -Command \"& { $DB_CONFIG import-int --db-name GCanada --schema dbo 'G:\canada'; Write-Host 'done' }\""

# ── Step 4: Import J:\advdata ─────────────────────────────────────────────────
echo ""
echo "[4/5] Importing J:\\advdata (db=JAdvdata schema=dbo)..."
ssh_run "powershell -Command \"& { $DB_CONFIG import-int --db-name JAdvdata --schema dbo 'J:\advdata'; Write-Host 'done' }\""

# ── Step 5: Set SQL Server connection ─────────────────────────────────────────
echo ""
echo "[5/5] Setting SQL Server connection (server: $SQL_SERVER)..."

CONN_CMD="$DB_CONFIG set-connection --server '$SQL_SERVER'"
[[ -n "$SQL_DATABASE"  ]] && CONN_CMD="$CONN_CMD --database '$SQL_DATABASE'"
[[ -n "$SQL_SCHEMA"    ]] && CONN_CMD="$CONN_CMD --schema '$SQL_SCHEMA'"
[[ -n "$SQL_USER"      ]] && CONN_CMD="$CONN_CMD --user '$SQL_USER'"
[[ -n "$SQL_PASS"      ]] && CONN_CMD="$CONN_CMD --password '$SQL_PASS'"
[[ -n "$SQL_TRUSTED"   ]] && CONN_CMD="$CONN_CMD --trusted-connection $([ "$SQL_TRUSTED" = "yes" ] && echo true || echo false)"
[[ -n "$SQL_ENCRYPT"   ]] && CONN_CMD="$CONN_CMD --encrypt $([ "$SQL_ENCRYPT" = "yes" ] && echo true || echo false)"
[[ -n "$SQL_TRUST_CERT" ]] && CONN_CMD="$CONN_CMD --trust-server-certificate $([ "$SQL_TRUST_CERT" = "yes" ] && echo true || echo false)"

ssh_run "powershell -Command \"& { $CONN_CMD; Write-Host 'connection set' }\""

echo ""
echo "=== Database setup complete on $HOST ==="
echo "Run 'db_config.exe --db C:\\WatkinsX\\config\\wxbtrv.db list' on target to verify."
