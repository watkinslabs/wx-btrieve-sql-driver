#!/usr/bin/env bash
# Single entry point for all DEV A / DEV B operations.
#
# Usage:
#   ./scripts/btr.sh <server> <action> [options]
#
# Reads .env (gitignored) for credentials and hostnames; see .env.example.
# Required vars: BTR_DEPLOY_PASS, BTR_DEVA_HOST, BTR_DEVB_HOST.
# Optional: BTR_DEPLOY_USER (defaults to "remote dos").
#
# Servers:
#   deva          our wxbtrv stack (wxbtrv.dll → System32)
#   devb          Pervasive reference + btrv-tracer (w3btrv7.dll → System32)
#
# Actions:
#   deploy        build and deploy the DLL for this server
#                 deva: builds wxbtrv.dll (release by default)
#                 devb: builds btrv-tracer as w3btrv7.dll
#   deploy debug  (deva only) deploy debug build with tracing enabled
#   test          run G:\pacific\adv.bat remotely, wait 20s, kill ntvdm
#   pull          pull the trace log from this server
#                 deva: C:\WatkinsX\logs\<latest>.log
#                 devb: C:\PVSW\bin\btr_trace.log
#
# Examples:
#   ./scripts/btr.sh deva deploy
#   ./scripts/btr.sh deva deploy debug
#   ./scripts/btr.sh deva pull
#   ./scripts/btr.sh devb deploy
#   ./scripts/btr.sh devb pull

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"

# Auto-source .env if present (gitignored, holds creds + hostnames)
if [ -f "$REPO/.env" ]; then
    set -a; . "$REPO/.env"; set +a
fi

SSH_PASS="${BTR_DEPLOY_PASS:?BTR_DEPLOY_PASS not set (put it in $REPO/.env or export it)}"
SSH_USER="${BTR_DEPLOY_USER:-remote dos}"

SERVER="${1:-}"
ACTION="${2:-}"
OPT="${3:-}"

usage() {
    grep '^#' "$0" | sed 's/^# \?//'
    exit 1
}

[ -z "$SERVER" ] || [ -z "$ACTION" ] && usage

ssh_run() {
    sshpass -p "$SSH_PASS" ssh -o StrictHostKeyChecking=no "${SSH_USER}@${HOST}" "$@"
}
scp_put() {
    sshpass -p "$SSH_PASS" scp -o StrictHostKeyChecking=no "$1" "${SSH_USER}@${HOST}:$2"
}

deploy_dll() {
    local src="$1" dst_win="$2" label="$3"
    echo "Deploying $label ($(wc -c < "$src") bytes) → $dst_win on $HOST..."
    # We never blanket-kill ntvdm.exe during deploy. If the user has an
    # ADV.BAT session running that has the DLL loaded, scp will fail with
    # "file in use" and they can close that session themselves — that's
    # strictly preferable to us terminating their work.
    if ! scp_put "$src" "$dst_win"; then
        echo "ERROR: scp failed. If $dst_win is locked by a running ntvdm, close"
        echo "       your ADV.BAT session on $HOST and retry. This script no"
        echo "       longer auto-kills ntvdm."
        return 1
    fi
    ssh_run "powershell -Command \"\$f = Get-Item '$dst_win'; Write-Host ('Deployed ' + \$f.Length + ' bytes to $dst_win')\""
}

# ── Server config ──────────────────────────────────────────────────────────────

case "$SERVER" in
  deva)
    HOST="${BTR_DEVA_HOST:?BTR_DEVA_HOST not set (put it in $REPO/.env)}"
    ;;
  devb)
    HOST="${BTR_DEVB_HOST:?BTR_DEVB_HOST not set (put it in $REPO/.env)}"
    ;;
  *)
    echo "Unknown server '$SERVER' — use deva or devb"
    exit 1
    ;;
esac

cd "$REPO"

# ── Actions ────────────────────────────────────────────────────────────────────

case "$ACTION" in

  deploy)
    case "$SERVER" in

      deva)
        MODE="${OPT:-release}"
        if [ "$MODE" = "debug" ]; then
            echo "Building wxbtrv debug..."
            touch crates/wxbtrv/src/lib.rs
            cargo build -p wxbtrv --target i686-pc-windows-gnu
            DLL="target/i686-pc-windows-gnu/debug/wxbtrv.dll"
        else
            echo "Building wxbtrv release..."
            touch crates/wxbtrv/src/lib.rs
            cargo build -p wxbtrv --release --target i686-pc-windows-gnu
            DLL="target/i686-pc-windows-gnu/release/wxbtrv.dll"
        fi
        deploy_dll "$DLL" "C:/Windows/System32/wxbtrv.dll" "wxbtrv.dll ($MODE)"
        echo "Done. Run ADV.BAT on DEV A to test."
        ;;

      devb)
        echo "Building btrv-tracer..."
        cargo build -p btrv-tracer --target i686-pc-windows-gnu
        DLL="target/i686-pc-windows-gnu/debug/btrv_tracer.dll"
        BUILD=$(grep -oP 'BUILD: u32 = \K[0-9]+' crates/btrv-tracer/src/log.rs 2>/dev/null || echo "?")
        deploy_dll "$DLL" "C:/Windows/System32/w3btrv7.dll" "btrv-tracer BUILD ${BUILD}"

        # Ensure w3btrv7_real.dll is in place
        ssh_run "powershell -Command \"
          if (-not (Test-Path 'C:\\PVSW\\bin\\w3btrv7_real.dll')) {
            if (Test-Path 'C:\\PVSW\\bin\\w3btrv7.dll.per') {
              Copy-Item 'C:\\PVSW\\bin\\w3btrv7.dll.per' 'C:\\PVSW\\bin\\w3btrv7_real.dll';
              Write-Host 'Copied w3btrv7.dll.per -> w3btrv7_real.dll';
            } else {
              Write-Host 'WARNING: w3btrv7.dll.per not found';
            }
          } else {
            Write-Host 'w3btrv7_real.dll already in place';
          }
          [IO.File]::WriteAllText('C:\\PVSW\\bin\\btr_trace.ini', \"TraceMode=mertech\`r\`n\");
          Write-Host 'btr_trace.ini written';
        \""
        echo "Done. Run ADV.BAT on DEV B, then: ./scripts/btr.sh devb pull"
        ;;
    esac
    ;;

  test)
    # This action is disabled by design — it used to blanket-kill ntvdm on
    # the host, which stomps on any work the user has running. Use the MCP
    # server's session.start / session.stop flow via scripts/mcp_client.py
    # instead, which only terminates the specific cmd+ntvdm it spawned.
    echo "btr.sh test is disabled. Use:"
    echo "  python3 scripts/mcp_client.py --host $HOST start 'G:\\\\pacific\\\\adv.bat'"
    echo "  python3 scripts/mcp_client.py --host $HOST stop  <session_id>"
    exit 1
    ;;

  pull)
    mkdir -p logs
    DEST="logs/${SERVER}-trace-$(date +%Y%m%d-%H%M%S).log"

    case "$SERVER" in
      deva)
        echo "Finding latest log on DEV A ($HOST)..."
        LOGFILE=$(ssh_run "powershell -Command \"Get-ChildItem 'C:\\WatkinsX\\logs\\*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName\"" 2>/dev/null | tr -d '\r')
        if [ -z "$LOGFILE" ]; then
            echo "No log files found in C:\\WatkinsX\\logs\\"
            exit 1
        fi
        echo "Pulling $LOGFILE..."
        ssh_run "powershell -Command \"Get-Content '$LOGFILE'\"" > "$DEST" 2>/dev/null
        ;;
      devb)
        LOGFILE='C:\PVSW\bin\btr_trace.log'
        echo "Pulling $LOGFILE from DEV B ($HOST)..."
        ssh_run "powershell -Command \"Get-Content '$LOGFILE'\"" > "$DEST" 2>/dev/null
        ;;
    esac

    LINES=$(wc -l < "$DEST")
    echo "Saved to $DEST ($LINES lines)"
    tail -80 "$DEST"
    ;;

  *)
    echo "Unknown action '$ACTION' — use deploy or pull"
    exit 1
    ;;
esac
