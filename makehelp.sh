#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# makehelp.sh — Complex logic extracted from Makefile
# ============================================================================
# The Makefile defines WHAT; this script implements HOW for anything too
# complex for inline make recipes (>10 lines, OS branching, multi-step ops).
# ============================================================================

BINARY_DIR="./target"
INSTALL_DIR="${HOME:?}/.local/bin"
BINARY_NAME="rustorm"
BINARIES=(rustorm rustorm-tui rustorm-gui)
HISTORY_PHRASES='recently|previously|as of|we now|has been changed'

# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------

check_command() {
    if ! command -v "$1" &>/dev/null; then
        echo "Error: $1 is required but not installed" >&2
        return 1
    fi
}

have_cargo_project() {
    [[ -f Cargo.toml ]]
}

no_cargo_yet() {
    echo "no Cargo project yet ($1 skipped); the project is in its definition stage — see docs/cli.md"
}

# ---------------------------------------------------------------------------
# Command functions
# ---------------------------------------------------------------------------

cmd_prereqs() {
    echo "Checking prerequisites..."
    case "$(uname -s)" in
        Darwin)
            echo "macOS detected"
            check_command brew || { echo "Install Homebrew first: https://brew.sh" >&2; exit 1; }
            command -v cargo &>/dev/null || brew install rustup-init
            ;;
        Linux)
            echo "Linux detected"
            command -v cargo &>/dev/null || echo "Install Rust with: curl https://sh.rustup.rs -sSf | sh"
            ;;
        *)
            echo "Unsupported OS: $(uname -s)" >&2
            exit 1
            ;;
    esac
    command -v cargo-deny &>/dev/null || echo "Install cargo-deny for 'make licenses': cargo install cargo-deny"
    echo "Prerequisite check complete"
}

cmd_build() {
    have_cargo_project || { no_cargo_yet build; return 0; }
    cargo build --workspace
}

cmd_build_production() {
    have_cargo_project || { no_cargo_yet build-production; return 0; }
    cargo build --release --workspace
}

cmd_test() {
    have_cargo_project || { no_cargo_yet test; return 0; }
    cargo test "$@"
}

cmd_lint() {
    # No argument: whole workspace. With a crate name: that crate only (fast).
    have_cargo_project || { no_cargo_yet lint; return 0; }
    if [[ $# -gt 0 ]]; then
        cargo clippy -p "$1" --all-targets -- -D warnings
    else
        cargo clippy --workspace --all-targets -- -D warnings
    fi
}

cmd_fmt() {
    have_cargo_project || { no_cargo_yet fmt; return 0; }
    cargo fmt
}

cmd_docs_check() {
    # Every doc must exist and describe the system as it is: no change history.
    local failed=0 doc
    for doc in "$@"; do
        if [[ ! -f "$doc" ]]; then
            echo "MISSING  $doc"
            failed=1
            continue
        fi
        if grep -niE "$HISTORY_PHRASES" "$doc" >/dev/null; then
            echo "HISTORY  $doc"
            grep -niE "$HISTORY_PHRASES" "$doc" | sed 's/^/           /'
            failed=1
            continue
        fi
        echo "ok       $doc"
    done
    if [[ $failed -ne 0 ]]; then
        echo "docs-check failed" >&2
        exit 1
    fi
    echo "docs-check passed ($# docs)"
}

cmd_licenses() {
    # D20: no copyleft anywhere in the dependency tree, direct or transitive.
    have_cargo_project || { no_cargo_yet licenses; return 0; }
    check_command cargo-deny || { echo "Install it with: cargo install cargo-deny" >&2; exit 1; }
    [[ -f deny.toml ]] || { echo "Error: deny.toml missing — add one with a copyleft deny list (GPL, LGPL, AGPL, SSPL, EUPL, CC-BY-SA)" >&2; exit 1; }
    cargo deny check licenses
}

cmd_run() {
    # run = the most common thing rustorm does: list the hosts in ~/.ssh/config.
    if [[ $# -gt 0 ]]; then
        exec cargo run --quiet -p "$BINARY_NAME" -- "$@"
    fi
    exec cargo run --quiet -p "$BINARY_NAME" -- list
}

cmd_run_tui() {
    # The terminal UI needs a real tty; say so instead of dumping a backtrace.
    if [[ ! -t 0 || ! -t 1 ]]; then
        echo "Error: rustorm-tui needs an interactive terminal (stdin and stdout must be a tty)" >&2
        exit 1
    fi
    exec cargo run --quiet -p rustorm-tui -- "$@"
}

cmd_run_gui() {
    exec cargo run --quiet -p rustorm-gui -- "$@"
}

cmd_install_dev() {
    # install-dev creates SYMLINKS so rebuilds work without reinstalling
    have_cargo_project || { no_cargo_yet install-dev; return 0; }
    mkdir -p "$INSTALL_DIR"
    local b binary
    for b in "${BINARIES[@]}"; do
        binary="${BINARY_DIR}/debug/${b}"
        [[ -x "$binary" ]] || { echo "Error: $binary not found (run 'make build' first)" >&2; exit 1; }
        ln -sf "$(pwd)/${binary}" "${INSTALL_DIR}/${b}"
        echo "Installed: ${INSTALL_DIR}/${b} -> $(pwd)/${binary} (symlink)"
    done
}

cmd_install_production() {
    # install-production COPIES binaries (self-contained, works after source removal)
    have_cargo_project || { no_cargo_yet install-production; return 0; }
    mkdir -p "$INSTALL_DIR"
    local b binary
    for b in "${BINARIES[@]}"; do
        binary="${BINARY_DIR}/release/${b}"
        [[ -x "$binary" ]] || { echo "Error: $binary not found (run 'make build-production' first)" >&2; exit 1; }
        cp -f "$binary" "${INSTALL_DIR}/${b}"
        chmod +x "${INSTALL_DIR}/${b}"
        echo "Installed: ${INSTALL_DIR}/${b} (copy)"
    done
}

cmd_uninstall() {
    local b
    for b in "${BINARIES[@]}"; do
        rm -f "${INSTALL_DIR:?}/${b:?}"
        echo "Uninstalled: ${INSTALL_DIR}/${b}"
    done
}

# ---------------------------------------------------------------------------
# Dispatcher — one entry per makehelp.sh-delegated target
# ---------------------------------------------------------------------------

case "${1:-}" in
    prereqs)            cmd_prereqs ;;
    build)              cmd_build ;;
    build-production)   cmd_build_production "${2:-}" ;;
    test)               shift; cmd_test "$@" ;;
    lint)               shift; cmd_lint "$@" ;;
    fmt)                cmd_fmt ;;
    docs-check)         shift; cmd_docs_check "$@" ;;
    licenses)           cmd_licenses ;;
    run)                shift; cmd_run "$@" ;;
    run-tui)            shift; cmd_run_tui "$@" ;;
    run-gui)            shift; cmd_run_gui "$@" ;;
    install-dev)        cmd_install_dev ;;
    install-production) cmd_install_production ;;
    uninstall)          cmd_uninstall ;;
    *)
        echo "Usage: $0 {prereqs|build|build-production|test|lint [crate]|fmt|docs-check|licenses|run|run-tui|run-gui|install-dev|install-production|uninstall}" >&2
        exit 1
        ;;
esac
