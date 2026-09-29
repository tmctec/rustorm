# ============================================================================
# rustorm Makefile
# ============================================================================
# 2-Layer System: This Makefile defines WHAT to do.
# Complex logic (>10 lines, OS branching) goes in makehelp.sh (HOW to do it).
# The Cargo workspace lives under crates/: rustorm (CLI), rustorm-core (library),
# rustorm-tui and rustorm-gui.
# ============================================================================

.DEFAULT_GOAL := help

# ---------------------------------------------------------------------------
# Variables (Rust toolchain)
# ---------------------------------------------------------------------------

BUILD_CMD := cargo build
TEST_CMD := cargo test
LINT_CMD := cargo clippy
FMT_CMD := cargo fmt

BINARY_DIR := ./target
COVERAGE_FILE := tarpaulin-report.html
DOCS := docs/about.md docs/features.md docs/cli.md docs/functionality.md docs/prd.md docs/USERGUIDE.md docs/tui.md docs/gui.md

VERSION := $(shell git describe --tags 2>/dev/null || echo "0.0.0-dev")

# ---------------------------------------------------------------------------
# Targets
# ---------------------------------------------------------------------------

##@ General

.PHONY: help
help:  ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} \
		/^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 } \
		/^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)

.PHONY: prereqs
prereqs:  ## Check and install prerequisites (rustup toolchain)
	@./makehelp.sh prereqs

##@ Build

.PHONY: all
all: build  ## Alias for build — builds everything

.PHONY: build
build:  ## Build every workspace crate (debug): rustorm, rustorm-core, rustorm-tui, rustorm-gui
	@./makehelp.sh build

.PHONY: build-production
build-production:  ## Build optimized release binaries for the whole workspace
	@./makehelp.sh build-production $(VERSION)

##@ Test

.PHONY: test
test:  ## Run every crate's tests (core, cli examples, tui snapshots, gui kittest, tui/gui parity)
	@./makehelp.sh test

.PHONY: test-unit
test-unit:  ## Run rustorm-core's library tests only (fast)
	@./makehelp.sh test -p rustorm-core --lib

.PHONY: test-core
test-core:  ## Run every rustorm-core test (model, ops, keyspec, combine, include, workspace, io, lexer)
	@./makehelp.sh test -p rustorm-core

.PHONY: test-cli
test-cli:  ## Run the rustorm CLI tests, including every docs/cli.md example
	@./makehelp.sh test -p rustorm

.PHONY: test-tui
test-tui:  ## Run the rustorm-tui TestBackend and pty tests
	@./makehelp.sh test -p rustorm-tui

.PHONY: test-gui
test-gui:  ## Run the rustorm-gui kittest suite
	@./makehelp.sh test -p rustorm-gui

GUI_WALKTHROUGH ?= target/gui-walkthrough.txt

.PHONY: test-parity
test-parity:  ## Run the same 13 operations in the TUI and the GUI and compare the files they write
	@./makehelp.sh test -p rustorm-parity

.PHONY: test-gui-walkthrough
test-gui-walkthrough:  ## Drive the GUI follow, settings (filled/all, Add setting) and editor completion flows; write what they show to GUI_WALKTHROUGH
	@RUSTORM_GUI_EVIDENCE=$(abspath $(GUI_WALKTHROUGH)) ./makehelp.sh test -p rustorm-gui --test settings gui_walkthrough -- --ignored
	@echo "walkthrough written to $(GUI_WALKTHROUGH)"

##@ Quality

.PHONY: lint
lint:  ## Run clippy on the whole workspace, warnings as errors (slow cold: egui tree, ~10 min)
	@./makehelp.sh lint

.PHONY: lint-core
lint-core:  ## Run clippy on rustorm-core only
	@./makehelp.sh lint rustorm-core

.PHONY: lint-cli
lint-cli:  ## Run clippy on the rustorm CLI crate only
	@./makehelp.sh lint rustorm

.PHONY: lint-tui
lint-tui:  ## Run clippy on rustorm-tui only
	@./makehelp.sh lint rustorm-tui

.PHONY: lint-gui
lint-gui:  ## Run clippy on rustorm-gui only
	@./makehelp.sh lint rustorm-gui

.PHONY: lint-parity
lint-parity:  ## Run clippy on the rustorm-parity test crate only
	@./makehelp.sh lint rustorm-parity

.PHONY: fmt
fmt:  ## Format code with cargo fmt
	@./makehelp.sh fmt

.PHONY: docs-check
docs-check:  ## Verify every product doc exists and carries no change-history phrasing
	@./makehelp.sh docs-check $(DOCS)

.PHONY: licenses
licenses:  ## Fail on any copyleft crate in the dependency tree (cargo deny)
	@./makehelp.sh licenses

.PHONY: check
check: lint test docs-check licenses  ## Run lint + test + docs-check + licenses together

##@ Install

.PHONY: install
install: install-dev  ## Alias for install-dev

.PHONY: install-dev
install-dev: build  ## Symlink the three debug binaries into ~/.local/bin
	@./makehelp.sh install-dev

.PHONY: install-production
install-production: build-production  ## Copy the three release binaries into ~/.local/bin
	@./makehelp.sh install-production

.PHONY: uninstall
uninstall:  ## Remove the installed rustorm, rustorm-tui and rustorm-gui binaries
	@./makehelp.sh uninstall

##@ Development

.PHONY: dev
dev: fmt test build  ## Format, test, and build

.PHONY: cycle
cycle: uninstall clean build install  ## Full clean rebuild and install

.PHONY: run
run: build  ## Run `rustorm list` on ~/.ssh/config (ARGS="..." runs any other rustorm command)
	@./makehelp.sh run $(ARGS)

.PHONY: run-tui
run-tui: build  ## Launch the rustorm-tui terminal UI on ~/.ssh/config (ARGS="--config FILE" for another file)
	@./makehelp.sh run-tui $(ARGS)

.PHONY: run-gui
run-gui: build  ## Launch the rustorm-gui egui window on ~/.ssh/config (ARGS="--config FILE" for another file)
	@./makehelp.sh run-gui $(ARGS)

##@ Cleanup

.PHONY: clean
clean:  ## Remove build artifacts
	@rm -rf ./target ./tarpaulin-report.html

.PHONY: clean-all
clean-all: clean  ## Deep clean including the reference clones under .claude/iterate/research
	@rm -rf ./.claude/iterate/research/storm ./.claude/iterate/research/ssh-config
