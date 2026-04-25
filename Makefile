##
## wlbtr — WatkinsX Btrieve Replacement Stack
## Build targets for all components.
##
## Requires:
##   rustup target add i686-pc-windows-gnu
##   apt install gcc-mingw-w64-i686 nasm
##

WIN_TARGET  := i686-pc-windows-gnu
RELEASE_DIR := target/release
WIN_DIR     := target/$(WIN_TARGET)/release

# ── Install paths (overridable at build time) ──────────────────────────────────
# Override: make installer WXBTRV_INSTALL_DIR='C:\MyApp\bin' WXBTRV_CONFIG_DIR='C:\MyApp\config'
WXBTRV_INSTALL_DIR ?= C:\WatkinsX\bin
WXBTRV_CONFIG_DIR  ?= C:\WatkinsX\config

export WXBTRV_INSTALL_DIR
export WXBTRV_CONFIG_DIR

# ── Default: build everything ──────────────────────────────────────────────────

.PHONY: all
all: dll sys db-config btr-import installer

# ── wxbtrv.dll  (32-bit Windows DLL — the Btrieve engine) ─────────────────────

.PHONY: dll
dll:
	cargo build -p wxbtrv --release --target $(WIN_TARGET)

# ── wxbtrv.sys  (16-bit DOS device driver) ────────────────────────────────────
# NTVDM only honors LOAD_LIBRARY_SEARCH_SYSTEM32 for VDDs, so the .sys file
# always references the bare filename. Override at build time if needed.

WXBTRV_DLL_PATH ?= "wxbtrv.dll"

.PHONY: sys
sys:
	mkdir -p $(WIN_DIR)
	nasm -f bin -DWXBTRV_DLL_PATH='$(WXBTRV_DLL_PATH)' -o $(WIN_DIR)/wxbtrv.sys crates/wxbtrv-sys/src/wxbtrv.asm

# ── db_config  (schema/config manager CLI) ────────────────────────────────────

.PHONY: db-config
db-config: db-config-native db-config-win

.PHONY: db-config-native
db-config-native:
	cargo build -p db-config --release

.PHONY: db-config-win
db-config-win:
	cargo build -p db-config --release --target $(WIN_TARGET)

# ── btr-import  (Btrieve .B → SQL Server migration tool) ──────────────────────

.PHONY: btr-import
btr-import: btr-import-native btr-import-win

.PHONY: btr-import-native
btr-import-native:
	cargo build -p btr-import --release

.PHONY: btr-import-win
btr-import-win:
	cargo build -p btr-import --release --target $(WIN_TARGET)

# ── installer  (single-file bundle — embeds all binaries) ─────────────────────

.PHONY: installer
installer: dll sys db-config-win btr-import-win
	WXBTRV_DLL=$(CURDIR)/$(WIN_DIR)/wxbtrv.dll \
	WXBTRV_SYS=$(CURDIR)/$(WIN_DIR)/wxbtrv.sys \
	INT_TOOL_EXE=$(CURDIR)/$(WIN_DIR)/db_config.exe \
	BTR_IMPORT_EXE=$(CURDIR)/$(WIN_DIR)/btr-import.exe \
	cargo build -p installer --release --target $(WIN_TARGET)

# ── Deploy targets ─────────────────────────────────────────────────────────────
# Reads .env for credentials and hostnames (see .env.example).

.PHONY: deploy
deploy: dll sys
	./scripts/btr.sh deva deploy

.PHONY: pull-trace
pull-trace:
	./scripts/btr.sh deva pull

# ── Check (no output, just verify everything compiles) ────────────────────────

.PHONY: check
check:
	cargo check --workspace
	cargo check --workspace --target $(WIN_TARGET)

# ── Clean ──────────────────────────────────────────────────────────────────────

.PHONY: clean
clean:
	cargo clean

# ── Show build artifacts ───────────────────────────────────────────────────────

.PHONY: artifacts
artifacts:
	@echo "=== Windows ($(WIN_TARGET)) ==="
	@ls -lh $(WIN_DIR)/wxbtrv.dll        2>/dev/null || echo "  wxbtrv.dll        — not built"
	@ls -lh $(WIN_DIR)/wxbtrv.sys        2>/dev/null || echo "  wxbtrv.sys        — not built"
	@ls -lh $(WIN_DIR)/db_config.exe     2>/dev/null || echo "  db_config.exe     — not built"
	@ls -lh $(WIN_DIR)/btr-import.exe    2>/dev/null || echo "  btr-import.exe    — not built"
	@ls -lh $(WIN_DIR)/installer.exe     2>/dev/null || echo "  installer.exe     — not built"
	@echo "=== Native (Linux) ==="
	@ls -lh $(RELEASE_DIR)/db_config     2>/dev/null || echo "  db_config         — not built"
	@ls -lh $(RELEASE_DIR)/btr-import    2>/dev/null || echo "  btr-import        — not built"
