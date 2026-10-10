#!/usr/bin/env bash
# Regression test for the NEWFRONT-174 parity-stack guard. Uses a stub
# `docker` on PATH, so it never touches a daemon and runs anywhere bash is.
#   apps/web_new/e2e/parity/stack/tests/test-parity-project.sh
set -euo pipefail

TEST_DIR="$(cd "$(dirname "$0")" && pwd)"
STACK_DIR="$(cd "$TEST_DIR/.." && pwd)"
# shellcheck source=../lib/parity-project.sh
. "$STACK_DIR/lib/parity-project.sh"

# Deterministic ambient env: the guard reads these, so start clean.
unset PARITY_PROJECT PARITY_CONTAINER_PREFIX PARITY_ALLOW_NAME_SKEW || true
unset SUDO_USER SUDO_UID || true

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL + 1)); }
pass() { echo "ok: $1"; PASS=$((PASS + 1)); }

# --- stub docker -----------------------------------------------------------
STUB_BIN="$(mktemp -d)"
STUB_LOG="$STUB_BIN/calls.log"
STUB_PS_FILE="$STUB_BIN/ps.txt"
export STUB_LOG STUB_PS_FILE
trap 'rm -rf "$STUB_BIN"' EXIT
cat > "$STUB_BIN/docker" <<'EOF'
#!/usr/bin/env bash
echo "docker $*" >> "${STUB_LOG:?}"
if [ "${1:-}" = "compose" ] && [ "${2:-}" = "-p" ]; then
  shift 3 # compose -p <project>
  if [ "${1:-}" = "ps" ]; then
    cat "${STUB_PS_FILE:-/dev/null}" 2>/dev/null || true
    exit "${STUB_PS_EXIT:-0}"
  fi
fi
exit 0
EOF
chmod +x "$STUB_BIN/docker"

stub_reset() {
  : > "$STUB_LOG"
  : > "$STUB_PS_FILE"
  unset STUB_PS_EXIT || true
}
PATH="$STUB_BIN:$PATH"
export PATH

# --- resolve() unit cases ---------------------------------------------------
got="$(parity_resolve_project 2>/dev/null)" && [ "$got" = "parity19" ] \
  && pass "resolve defaults to parity19" || fail "resolve defaults to parity19 (got '$got')"

got="$(PARITY_PROJECT=parity19s2 parity_resolve_project 2>/dev/null)" && [ "$got" = "parity19s2" ] \
  && pass "resolve honors PARITY_PROJECT" || fail "resolve honors PARITY_PROJECT (got '$got')"

got="$(parity_resolve_project parity19s2 2>/dev/null)" && [ "$got" = "parity19s2" ] \
  && pass "resolve honors explicit argument" || fail "resolve honors explicit argument (got '$got')"

if PARITY_PROJECT=parity19s2 parity_resolve_project parity19s2 >/dev/null 2>&1; then
  pass "resolve accepts argument agreeing with env"
else
  fail "resolve accepts argument agreeing with env"
fi

if PARITY_PROJECT=parity19s2 parity_resolve_project parity19s4 >/dev/null 2>&1; then
  fail "resolve refuses argument disagreeing with env"
else
  pass "resolve refuses argument disagreeing with env"
fi

if SUDO_USER=root parity_resolve_project >/dev/null 2>&1; then
  fail "resolve refuses silent default under sudo"
else
  pass "resolve refuses silent default under sudo"
fi

if SUDO_UID=0 parity_resolve_project >/dev/null 2>&1; then
  fail "resolve refuses silent default under sudo (SUDO_UID)"
else
  pass "resolve refuses silent default under sudo (SUDO_UID)"
fi

got="$(SUDO_USER=root parity_resolve_project parity19s2 2>/dev/null)" && [ "$got" = "parity19s2" ] \
  && pass "resolve accepts explicit argument under sudo" || fail "resolve accepts explicit argument under sudo (got '$got')"

got="$(SUDO_USER=root PARITY_PROJECT=parity19s2 parity_resolve_project 2>/dev/null)" && [ "$got" = "parity19s2" ] \
  && pass "resolve honors env passed explicitly through sudo" || fail "resolve honors env passed explicitly through sudo (got '$got')"

got="$(SUDO_USER=root parity_resolve_project_allow_sudo 2>/dev/null)" && [ "$got" = "parity19" ] \
  && pass "resolve_allow_sudo keeps default (bring-up)" || fail "resolve_allow_sudo keeps default (got '$got')"

got="$(PARITY_PROJECT='' parity_resolve_project 2>/dev/null)" && [ "$got" = "parity19" ] \
  && pass "resolve treats empty env as unset" || fail "resolve treats empty env as unset (got '$got')"

if parity_resolve_project --allow-sudo-default >/dev/null 2>&1; then
  fail "resolve refuses flag-like project name"
else
  pass "resolve refuses flag-like project name"
fi

# --- assert() unit cases ----------------------------------------------------
stub_reset
if parity_assert_project parity19s2 >/dev/null 2>&1; then
  pass "assert passes on empty project"
else
  fail "assert passes on empty project"
fi

stub_reset
printf 'parity19s2-pg\nparity19s2-api\n' > "$STUB_PS_FILE"
if PARITY_CONTAINER_PREFIX=parity19s2 parity_assert_project parity19s2 >/dev/null 2>&1; then
  pass "assert passes when names match prefix"
else
  fail "assert passes when names match prefix"
fi

stub_reset
printf 'parity19s2-pg\nparity19-api\n' > "$STUB_PS_FILE"
if PARITY_CONTAINER_PREFIX=parity19s2 parity_assert_project parity19s2 >/dev/null 2>&1; then
  fail "assert refuses foreign-named container"
else
  pass "assert refuses foreign-named container"
fi

stub_reset
printf 'parity19s2-pg\nparity19-api\n' > "$STUB_PS_FILE"
if PARITY_CONTAINER_PREFIX=parity19s2 PARITY_ALLOW_NAME_SKEW=1 parity_assert_project parity19s2 >/dev/null 2>&1; then
  pass "assert override PARITY_ALLOW_NAME_SKEW=1 proceeds"
else
  fail "assert override PARITY_ALLOW_NAME_SKEW=1 proceeds"
fi

stub_reset
printf 'parity19s2-pg\n' > "$STUB_PS_FILE"
export STUB_PS_EXIT=1
if parity_assert_project parity19s2 >/dev/null 2>&1; then
  fail "assert fails closed when docker listing fails"
else
  pass "assert fails closed when docker listing fails"
fi
unset STUB_PS_EXIT || true

stub_reset
printf 'parity19-pg\nparity19-api\n' > "$STUB_PS_FILE"
if parity_assert_project parity19 >/dev/null 2>&1; then
  pass "assert passes on default project with default prefix"
else
  fail "assert passes on default project with default prefix"
fi

# --- parity-down.sh end to end (stub docker) --------------------------------
run_down() {
  # run_down <ps-file-content> <env...> -- [args...]
  stub_reset
  printf '%s' "$1" > "$STUB_PS_FILE"
  shift
  local env_args=()
  while [ "${1:-}" != "--" ]; do env_args+=("$1"); shift; done
  shift
  env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" "${env_args[@]}" \
    "$STACK_DIR/parity-down.sh" "$@" >/dev/null 2>&1
}

# The incident shape: slot intent lost (no PARITY_PROJECT), under sudo.
if run_down '' SUDO_USER=root SUDO_UID=0 --; then
  fail "down.sh refuses incident shape (sudo, no project)"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "down.sh refuses incident shape but still called down"
  else
    pass "down.sh refuses incident shape (sudo, no project), no down issued"
  fi
fi

# Slot teardown targets the slot only, never the default project.
if run_down 'parity19s2-pg
parity19s2-api
' PARITY_PROJECT=parity19s2 PARITY_CONTAINER_PREFIX=parity19s2 --; then
  if grep -q "compose -p parity19s2 down -v" "$STUB_LOG" && ! grep -q "compose -p parity19 " "$STUB_LOG"; then
    pass "down.sh slot teardown targets slot project only"
  else
    fail "down.sh slot teardown invoked wrong project (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "down.sh slot teardown should succeed"
fi

# Documented sudo form: explicit argument survives env loss.
if run_down '' SUDO_USER=root SUDO_UID=0 -- parity19s2; then
  if grep -q "compose -p parity19s2 down -v" "$STUB_LOG"; then
    pass "down.sh sudo + explicit argument targets the slot"
  else
    fail "down.sh sudo + explicit argument invoked wrong project (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "down.sh sudo + explicit argument should succeed"
fi

# Default teardown is unchanged without sudo.
if run_down 'parity19-pg
' --; then
  if grep -q "compose -p parity19 down -v" "$STUB_LOG"; then
    pass "down.sh default teardown unchanged without sudo"
  else
    fail "down.sh default teardown invoked wrong project (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "down.sh default teardown should succeed"
fi

# Skewed slot is refused before any down.
if run_down 'parity19s2-pg
parity19-api
' PARITY_PROJECT=parity19s2 PARITY_CONTAINER_PREFIX=parity19s2 --; then
  fail "down.sh refuses name-skewed project"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "down.sh refuses skewed project but still called down"
  else
    pass "down.sh refuses name-skewed project, no down issued"
  fi
fi

# --- real sudo (skipped when unavailable) -----------------------------------
if sudo -n true >/dev/null 2>&1; then
  stub_reset
  : > "$STUB_PS_FILE"
  if sudo -n env PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
      "$STACK_DIR/parity-down.sh" >/dev/null 2>&1; then
    fail "real sudo: down.sh refuses with env dropped"
  else
    if grep -q " down " "$STUB_LOG"; then
      fail "real sudo: down.sh refused but still called down"
    else
      pass "real sudo: down.sh refuses with env dropped, no down issued"
    fi
  fi
  stub_reset
  if sudo -n env PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
      "$STACK_DIR/parity-down.sh" parity19s2 >/dev/null 2>&1 \
      && grep -q "compose -p parity19s2 down -v" "$STUB_LOG"; then
    pass "real sudo: down.sh with explicit argument targets the slot"
  else
    fail "real sudo: down.sh with explicit argument targets the slot"
  fi
else
  echo "skip: real-sudo cases (passwordless sudo unavailable)"
fi

echo "---"
echo "$PASS passed, $FAIL failed"
[ "$FAIL" = "0" ]
