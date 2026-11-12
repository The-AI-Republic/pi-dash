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
unset PARITY_NS PARITY_PROJECT PARITY_CONTAINER_PREFIX PARITY_ALLOW_NAME_SKEW || true
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
exit "${STUB_DOWN_EXIT:-0}"
EOF
chmod +x "$STUB_BIN/docker"

stub_reset() {
  : > "$STUB_LOG"
  : > "$STUB_PS_FILE"
  unset STUB_PS_EXIT || true
  unset STUB_DOWN_EXIT || true
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

got="$(PARITY_PROJECT='' parity_resolve_project 2>/dev/null)" && [ "$got" = "parity19" ] \
  && pass "resolve treats empty env as unset" || fail "resolve treats empty env as unset (got '$got')"

if parity_resolve_project --allow-sudo-default >/dev/null 2>&1; then
  fail "resolve refuses flag-like project name"
else
  pass "resolve refuses flag-like project name"
fi

# --- require_identity_under_sudo() unit cases ---------------------------------
if parity_require_identity_under_sudo "parity-down.sh <project>" >/dev/null 2>&1; then
  pass "sudo-identity passes without sudo and without identity (default flow)"
else
  fail "sudo-identity passes without sudo and without identity (default flow)"
fi

if SUDO_USER=root parity_require_identity_under_sudo "parity-down.sh <project>" >/dev/null 2>&1; then
  fail "sudo-identity refuses under sudo with no identity"
else
  pass "sudo-identity refuses under sudo with no identity"
fi

if SUDO_USER=root PARITY_NS=257 parity_require_identity_under_sudo "parity-down.sh <project>" >/dev/null 2>&1; then
  pass "sudo-identity passes under sudo with PARITY_NS (sudo env form)"
else
  fail "sudo-identity passes under sudo with PARITY_NS (sudo env form)"
fi

if SUDO_USER=root PARITY_PROJECT=parity257 parity_require_identity_under_sudo "parity-down.sh <project>" >/dev/null 2>&1; then
  pass "sudo-identity passes under sudo with PARITY_PROJECT (env_keep form)"
else
  fail "sudo-identity passes under sudo with PARITY_PROJECT (env_keep form)"
fi

if SUDO_UID=0 parity_require_identity_under_sudo "parity-down.sh <project>" >/dev/null 2>&1; then
  fail "sudo-identity refuses under sudo with no identity (SUDO_UID)"
else
  pass "sudo-identity refuses under sudo with no identity (SUDO_UID)"
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

stub_reset
printf 'parity19s2-pg\n' > "$STUB_PS_FILE"
PARITY_CONTAINER_PREFIX=parity19s2 parity_assert_project parity19s2 >/dev/null 2>&1 || true
if grep -q "compose -p parity19s2 ps --all" "$STUB_LOG"; then
  pass "assert lists stopped containers too (ps --all)"
else
  fail "assert lists stopped containers too (ps --all) (log: $(cat "$STUB_LOG"))"
fi

stub_reset
printf 'parity19s2-pg\n\nparity19s2-api\n' > "$STUB_PS_FILE"
if PARITY_CONTAINER_PREFIX=parity19s2 parity_assert_project parity19s2 >/dev/null 2>&1; then
  pass "assert skips blank lines in the listing"
else
  fail "assert skips blank lines in the listing"
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

# Namespaced teardown: identity surviving under sudo resolves the namespace.
if run_down 'parity257-pg
' SUDO_USER=root SUDO_UID=0 PARITY_NS=257 --; then
  if grep -q "compose -p parity257 down -v" "$STUB_LOG" && ! grep -q "compose -p parity19 " "$STUB_LOG"; then
    pass "down.sh sudo + PARITY_NS targets the namespace"
  else
    fail "down.sh sudo + PARITY_NS invoked wrong project (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "down.sh sudo + PARITY_NS should succeed"
fi

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

# Explicit argument disagreeing with the environment: refuse, destroy nothing.
if run_down '' PARITY_PROJECT=parity19s4 PARITY_CONTAINER_PREFIX=parity19s4 -- parity19s2; then
  fail "down.sh refuses explicit argument disagreeing with env"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "down.sh refuses disagreeing argument but still called down"
  else
    pass "down.sh refuses explicit argument disagreeing with env, no down issued"
  fi
fi

# Resolver failure (invalid namespace): exit before touching anything.
if run_down '' PARITY_NS=-bad --; then
  fail "down.sh exits when the resolver fails"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "down.sh exits on resolver failure but still called down"
  else
    pass "down.sh exits when the resolver fails, no down issued"
  fi
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

# Extra arguments are refused: a destructive script must not guess.
if run_down '' PARITY_PROJECT=parity19s2 PARITY_CONTAINER_PREFIX=parity19s2 -- parity19s2 --dry-run; then
  fail "down.sh refuses extra arguments"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "down.sh refuses extra arguments but still called down"
  else
    pass "down.sh refuses extra arguments, no down issued"
  fi
fi

# reset.sh refuses extra arguments before touching anything (bounded: a
# refusal returns at once instead of running the bring-up).
stub_reset
if timeout 20 env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
    "$STACK_DIR/parity-reset.sh" parity19s2 --dry-run >/dev/null 2>&1; then
  fail "reset.sh refuses extra arguments"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "reset.sh refuses extra arguments but still called down"
  else
    pass "reset.sh refuses extra arguments, no down issued"
  fi
fi

# reset.sh refuses the incident shape (sudo, no identity) before resolving.
stub_reset
if timeout 20 env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
    SUDO_USER=root SUDO_UID=0 \
    "$STACK_DIR/parity-reset.sh" >/dev/null 2>&1; then
  fail "reset.sh refuses incident shape (sudo, no project)"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "reset.sh refuses incident shape but still called down"
  else
    pass "reset.sh refuses incident shape (sudo, no project), no down issued"
  fi
fi

# reset.sh exits when the resolver fails, before destroying anything.
stub_reset
if timeout 20 env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
    PARITY_NS=-bad \
    "$STACK_DIR/parity-reset.sh" >/dev/null 2>&1; then
  fail "reset.sh exits when the resolver fails"
else
  if grep -q " down " "$STUB_LOG"; then
    fail "reset.sh exits on resolver failure but still called down"
  else
    pass "reset.sh exits when the resolver fails, no down issued"
  fi
fi

# --- parity-reset.sh success paths (fake stack dir, stub docker) --------------
# reset.sh ends by running the real bring-up, so its success paths run
# against a fake STACK_DIR: the real reset.sh + guard lib, with stubbed
# resolver/guard/bring-up. The stub docker still records every down.
FAKE_RESET="$STUB_BIN/fakestack"
mkdir -p "$FAKE_RESET/lib"
cp "$STACK_DIR/parity-reset.sh" "$FAKE_RESET/"
cp "$STACK_DIR/lib/parity-project.sh" "$FAKE_RESET/lib/"
cat > "$FAKE_RESET/parity-env.sh" <<'EOF'
#!/usr/bin/env bash
echo "env resolver called" >> "${STUB_LOG:?}"
printf 'export PARITY_NS=%s\n' "'fakens'"
printf 'export PARITY_PROJECT=%s\n' "'parityfake'"
printf 'export PARITY_CONTAINER_PREFIX=%s\n' "'parityfake'"
EOF
chmod +x "$FAKE_RESET/parity-env.sh"
cat > "$FAKE_RESET/parity-guard.sh" <<'EOF'
#!/usr/bin/env bash
echo "guard $*" >> "${STUB_LOG:?}"
exit "${STUB_GUARD_EXIT:-0}"
EOF
chmod +x "$FAKE_RESET/parity-guard.sh"
cat > "$FAKE_RESET/parity-up.sh" <<'EOF'
#!/usr/bin/env bash
echo "up called prefix=${PARITY_CONTAINER_PREFIX:-<unset>} project=${PARITY_PROJECT:-<unset>}" >> "${STUB_LOG:?}"
exit 0
EOF
chmod +x "$FAKE_RESET/parity-up.sh"

run_reset() {
  # run_reset <ps-file-content> <env...> -- [args...]
  stub_reset
  printf '%s' "$1" > "$STUB_PS_FILE"
  shift
  local env_args=()
  while [ "${1:-}" != "--" ]; do env_args+=("$1"); shift; done
  shift
  env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" "${env_args[@]}" \
    "$FAKE_RESET/parity-reset.sh" "$@" >/dev/null 2>&1
}

# Explicit argument, prefix unset: the prefix defaults to the project, the
# bring-up half runs, and only that project is torn down.
if run_reset 'parity19s2-pg
parity19s2-api
' -- parity19s2; then
  if grep -q "compose -p parity19s2 -f .* down -v" "$STUB_LOG" \
     && grep -q "up called prefix=parity19s2 project=parity19s2" "$STUB_LOG" \
     && ! grep -q "compose -p parity19 " "$STUB_LOG"; then
    pass "reset.sh explicit argument resets the slot and brings it up"
  else
    fail "reset.sh explicit argument hit the wrong target (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "reset.sh explicit argument should succeed"
fi

# Explicit argument with an explicit prefix: the prefix is honored, not reset.
if run_reset 'custom-pg
' PARITY_CONTAINER_PREFIX=custom -- parity19s2; then
  if grep -q "up called prefix=custom project=parity19s2" "$STUB_LOG"; then
    pass "reset.sh honors an explicit container prefix"
  else
    fail "reset.sh lost the explicit prefix (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "reset.sh with explicit prefix should succeed"
fi

# Skewed slot is refused before any down, and the bring-up never runs.
if run_reset 'parity19s2-pg
parity19-api
' PARITY_CONTAINER_PREFIX=parity19s2 -- parity19s2; then
  fail "reset.sh refuses name-skewed project"
else
  if grep -q " down " "$STUB_LOG" || grep -q "^up called" "$STUB_LOG"; then
    fail "reset.sh refuses skewed project but still acted"
  else
    pass "reset.sh refuses name-skewed project, no down or bring-up"
  fi
fi

# No argument: identity resolves through the resolver, then guard, down, up.
if run_reset 'parityfake-pg
' --; then
  if grep -q "compose -p parityfake -f .* down -v" "$STUB_LOG" \
     && grep -q "^guard parityfake " "$STUB_LOG" \
     && grep -q "up called prefix=parityfake project=parityfake" "$STUB_LOG"; then
    pass "reset.sh default path resolves, guards, downs and brings up"
  else
    fail "reset.sh default path missed a step (log: $(cat "$STUB_LOG"))"
  fi
else
  fail "reset.sh default path should succeed"
fi

# A guard refusal still gates the reset after the rewire: no down, no up.
if run_reset '' STUB_GUARD_EXIT=3 -- parity19s2; then
  fail "reset.sh proceeds despite a guard refusal"
else
  if grep -q " down " "$STUB_LOG" || grep -q "^up called" "$STUB_LOG"; then
    fail "reset.sh guard refusal still acted"
  else
    pass "reset.sh guard refusal stops before down and bring-up"
  fi
fi

# --- parity-reaper.sh watch (stub docker) ------------------------------------
dead_pid_for_reaper() {
  : & _dead=$!
  wait "$_dead" 2>/dev/null || true
  printf '%s' "$_dead"
}

# A guard refusal must never be overridden by the reaper fallback.
stub_reset
printf 'parity19s2-pg\nparity19-api\n' > "$STUB_PS_FILE"
REAPER_DEAD_PID="$(dead_pid_for_reaper)"
env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
  PARITY_PROJECT=parity19s2 PARITY_CONTAINER_PREFIX=parity19s2 TMPDIR="$STUB_BIN" \
  "$STACK_DIR/parity-reaper.sh" watch "$REAPER_DEAD_PID" >/dev/null 2>&1 || true
if grep -q " down " "$STUB_LOG"; then
  fail "reaper watch still reaps after a guard refusal"
else
  pass "reaper watch does not override a guard refusal"
fi

# The success path: down.sh tears down once, no fallback fires, MARK is gone.
stub_reset
printf 'parity19s2-pg\nparity19s2-api\n' > "$STUB_PS_FILE"
REAPER_DEAD_PID="$(dead_pid_for_reaper)"
REAPER_MARK="$STUB_BIN/parity-reaper.parity19s2.$REAPER_DEAD_PID.pid"
env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
  PARITY_PROJECT=parity19s2 PARITY_CONTAINER_PREFIX=parity19s2 TMPDIR="$STUB_BIN" \
  "$STACK_DIR/parity-reaper.sh" watch "$REAPER_DEAD_PID" >/dev/null 2>&1 || true
if [ "$(grep -c " down -v" "$STUB_LOG")" = "1" ] && [ ! -e "$REAPER_MARK" ]; then
  pass "reaper watch tears down once on success and removes MARK"
else
  fail "reaper watch success path misbehaved (log: $(cat "$STUB_LOG"))"
fi

# The fallback still reaps when the checkout (and parity-down.sh) is gone.
stub_reset
FAKE_STACK="$STUB_BIN/nostack"
mkdir -p "$FAKE_STACK"
cp "$STACK_DIR/parity-reaper.sh" "$FAKE_STACK/"
REAPER_DEAD_PID="$(dead_pid_for_reaper)"
env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" \
  PARITY_PROJECT=parity19gone TMPDIR="$STUB_BIN" \
  "$FAKE_STACK/parity-reaper.sh" watch "$REAPER_DEAD_PID" >/dev/null 2>&1 || true
if grep -q "compose -p parity19gone down -v" "$STUB_LOG"; then
  pass "reaper watch falls back when parity-down.sh is gone"
else
  fail "reaper watch falls back when parity-down.sh is gone (log: $(cat "$STUB_LOG"))"
fi

# The fallback tolerates a failing docker: the watcher still exits cleanly.
stub_reset
mkdir -p "$FAKE_STACK"
cp "$STACK_DIR/parity-reaper.sh" "$FAKE_STACK/"
REAPER_DEAD_PID="$(dead_pid_for_reaper)"
REAPER_MARK2="$STUB_BIN/parity-reaper.parity19gone.$REAPER_DEAD_PID.pid"
reaper_rc=0
env -i PATH="$STUB_BIN:/usr/bin:/bin" STUB_LOG="$STUB_LOG" STUB_PS_FILE="$STUB_PS_FILE" STUB_DOWN_EXIT=1 \
  PARITY_PROJECT=parity19gone TMPDIR="$STUB_BIN" \
  "$FAKE_STACK/parity-reaper.sh" watch "$REAPER_DEAD_PID" >/dev/null 2>&1 || reaper_rc=$?
if [ "$reaper_rc" = "0" ] && grep -q "compose -p parity19gone down -v" "$STUB_LOG" && [ ! -e "$REAPER_MARK2" ]; then
  pass "reaper watch survives a failing fallback down"
else
  fail "reaper watch mishandles a failing fallback down (rc=$reaper_rc, log: $(cat "$STUB_LOG"))"
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
