#!/usr/bin/env bash
# scripts/secret-scan.sh — local secret scanning with gitleaks (Issue #1533)
#
# Single local entry point for secret scanning. Wraps the gitleaks binary
# with the repo's .gitleaks.toml config so contributors, the husky
# pre-commit hook, and CI all scan the same way.
#
# Default mode scans the working tree only (--no-git): fast, and it skips
# gitignored files (node_modules, .env, .next, ...). Pass --history to
# scan the full git history instead.
#
# Exit codes:
#   0 — no secrets found
#   1 — secrets found (do not commit/push; rotate anything real)
#   2 — usage error, missing config, or gitleaks not installed

set -euo pipefail

CONFIG=".gitleaks.toml"
SOURCE=""
HISTORY=false
REPORT=""
# Overridable so tests can simulate a missing/alternate binary.
GITLEAKS_BIN="${GITLEAKS_BIN:-gitleaks}"

usage() {
  cat <<'EOF'
Usage: scripts/secret-scan.sh [options]

Scan for accidentally committed secrets using gitleaks and the repo's
.gitleaks.toml config.

Options:
  -s, --source <path>   File or directory to scan (default: .)
  -c, --config <path>   Gitleaks config file (default: .gitleaks.toml)
  -r, --report <path>   Write a JSON findings report to <path>
      --history         Scan full git history instead of the working tree
  -h, --help            Show this help and exit

Exit codes:
  0 — no secrets found
  1 — secrets found
  2 — usage error, missing config, or gitleaks not installed
EOF
}

die() {
  echo "secret-scan: error: $*" >&2
  echo "Run 'scripts/secret-scan.sh --help' for usage." >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -s|--source)
      [[ $# -ge 2 ]] || die "$1 requires a path"
      SOURCE="$2"
      shift 2
      ;;
    -c|--config)
      [[ $# -ge 2 ]] || die "$1 requires a path"
      CONFIG="$2"
      shift 2
      ;;
    -r|--report)
      [[ $# -ge 2 ]] || die "$1 requires a path"
      REPORT="$2"
      shift 2
      ;;
    --history)
      HISTORY=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

[[ -n "$SOURCE" ]] || SOURCE="."

if [[ ! -f "$CONFIG" ]]; then
  echo "secret-scan: error: gitleaks config not found: $CONFIG" >&2
  exit 2
fi

if ! command -v "$GITLEAKS_BIN" >/dev/null 2>&1; then
  cat >&2 <<EOF
secret-scan: error: gitleaks not found (looked for '$GITLEAKS_BIN').
Install: brew install gitleaks (macOS) | apt-get install gitleaks (Debian/Ubuntu)
         Or: go install github.com/gitleaks/gitleaks/v8@latest
EOF
  exit 2
fi

args=(detect --source "$SOURCE" --config "$CONFIG" --no-banner)
if [[ "$HISTORY" == false ]]; then
  args+=(--no-git)
fi
if [[ -n "$REPORT" ]]; then
  args+=(--report-format json --report-path "$REPORT")
fi

set +e
"$GITLEAKS_BIN" "${args[@]}"
status=$?
set -e

if [[ $status -eq 0 ]]; then
  echo "secret-scan: no secrets found ($SOURCE)"
  exit 0
elif [[ $status -eq 1 ]]; then
  echo "secret-scan: secrets found in $SOURCE — commit blocked. Fix the findings or rotate anything real." >&2
  exit 1
else
  echo "secret-scan: gitleaks failed with exit code $status" >&2
  exit 2
fi
