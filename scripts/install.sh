#!/usr/bin/env bash
# Full installation of wxbtrv stack on a target Windows machine.
#
# Usage:
#   BTR_DEPLOY_PASS='...' ./scripts/install.sh [host]
#
# Environment variables:
#   BTR_DEPLOY_HOST  — target host       (default: 10.0.10.10)
#   BTR_DEPLOY_USER  — SSH user          (default: remote dos)
#   BTR_DEPLOY_PASS  — SSH password      (required)
#
# What this script does:
#   1. Creates C:\WatkinsX\bin and C:\WatkinsX\config on the target
#   2. Copies wxbtrv.dll, wxbtrv.sys, db_config.exe, btr-import.exe
#   3. Adds C:\WatkinsX\bin to the system PATH
#   4. Patches config.nt: comments out old Btrieve/Pervasive DEVICE= lines, adds ours
#   5. Kills ntvdm.exe so the new driver is picked up on next launch

set -euo pipefail

HOST="${1:-${BTR_DEPLOY_HOST:-10.0.10.10}}"
USER="${BTR_DEPLOY_USER:-remote dos}"
PASS="${BTR_DEPLOY_PASS:?BTR_DEPLOY_PASS not set}"

WIN_DIR="target/i686-pc-windows-gnu/release"
NATIVE_DIR="target/release"

SSH="sshpass -p '$PASS' ssh -o StrictHostKeyChecking=no"
SCP="sshpass -p '$PASS' scp -o StrictHostKeyChecking=no"

ssh_run() {
    sshpass -p "$PASS" ssh -o StrictHostKeyChecking=no "$USER@$HOST" "$@"
}
scp_file() {
    sshpass -p "$PASS" scp -o StrictHostKeyChecking=no "$1" "$USER@$HOST:$2"
}
transfer_binary() {
    local src="$1" dst="$2"
    echo "  copying $(basename $src) → $dst"
    base64 -w0 "$src" > /tmp/_xfer.b64
    scp_file /tmp/_xfer.b64 'C:/Windows/Temp/_xfer.b64'
    ssh_run "powershell -Command \"\$b=[Convert]::FromBase64String([IO.File]::ReadAllText('C:\\Windows\\Temp\\_xfer.b64')); [IO.File]::WriteAllBytes('$dst', \$b); Write-Host 'wrote' \$b.Length 'bytes'\""
    rm -f /tmp/_xfer.b64
}

echo "=== Installing wxbtrv stack on $HOST ==="

# ── Step 1: Create install directories ────────────────────────────────────────
echo ""
echo "[1/5] Creating C:\\WatkinsX\\bin and C:\\WatkinsX\\config..."
ssh_run "powershell -Command \"
  New-Item -ItemType Directory -Force 'C:\\WatkinsX\\bin'    | Out-Null
  New-Item -ItemType Directory -Force 'C:\\WatkinsX\\config' | Out-Null
  Write-Host 'OK'
\""

# ── Step 2: Copy binaries ──────────────────────────────────────────────────────
echo ""
echo "[2/5] Copying binaries..."
transfer_binary "$WIN_DIR/wxbtrv.dll"       'C:/WatkinsX/bin/wxbtrv.dll'
transfer_binary "$WIN_DIR/wxbtrv.sys"       'C:/WatkinsX/bin/wxbtrv.sys'
transfer_binary "$WIN_DIR/db_config.exe"     'C:/WatkinsX/bin/db_config.exe'
transfer_binary "$WIN_DIR/btr-import.exe"   'C:/WatkinsX/bin/btr-import.exe'
transfer_binary "$WIN_DIR/installer.exe"    'C:/WatkinsX/bin/installer.exe'

# ── Step 3: Add to system PATH ────────────────────────────────────────────────
echo ""
echo "[3/5] Adding C:\\WatkinsX\\bin to system PATH..."
ssh_run "powershell -Command \"
  \$p = [Environment]::GetEnvironmentVariable('Path','Machine');
  if (\$p -notlike '*WatkinsX*') {
    [Environment]::SetEnvironmentVariable('Path', 'C:\\WatkinsX\\bin;' + \$p, 'Machine');
    Write-Host 'PATH updated';
  } else {
    Write-Host 'PATH already contains WatkinsX\\bin';
  }
\""

# ── Step 4: Patch config.nt ───────────────────────────────────────────────────
echo ""
echo "[4/5] Patching C:\\Windows\\System32\\config.nt..."

# Comment out any existing Btrieve/Pervasive DEVICE= lines, add ours.
# Three separate commands to avoid multi-line escaping failures.
ssh_run "powershell -Command \"(Get-Content 'C:\\Windows\\System32\\config.nt') | ForEach-Object { if (\$_ -match '(?i)^DEVICE=.*btrv') { 'REM [replaced by WatkinsX] ' + \$_ } else { \$_ } } | Set-Content 'C:\\Windows\\System32\\config.nt'\""
ssh_run "powershell -Command \"(Get-Content 'C:\\Windows\\System32\\config.nt') | Where-Object { \$_ -notmatch '(?i)wxbtrv\\.sys' } | Set-Content 'C:\\Windows\\System32\\config.nt'\""
ssh_run "powershell -Command \"Add-Content 'C:\\Windows\\System32\\config.nt' 'DEVICE=C:\\WatkinsX\\bin\\wxbtrv.sys'\""
echo "      Result:"
ssh_run "powershell -Command \"Get-Content 'C:\\Windows\\System32\\config.nt' | Select-String '(?i)DEVICE|REM.*replaced'\" " | sed 's/^/        /'

# ── Step 5: Kill ntvdm ────────────────────────────────────────────────────────
echo ""
echo "[5/5] Killing ntvdm.exe..."
ssh_run "powershell -Command \"taskkill /F /IM ntvdm.exe 2>\$null; \$true | Out-Null; Write-Host 'ntvdm stopped (or was not running)'\""

echo ""
echo "=== Installation complete on $HOST ==="
echo "Next: run scripts/setup-db.sh to initialise wxbtrv.db and import schemas."
