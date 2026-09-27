/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { AGENT_RUNTIME_REASON_MESSAGES } from "@/pi-dash-web/components/desktop/agent-runtime-edition";

// Guards against the desktop copy map drifting from the reason codes the
// backend actually emits. Two things go wrong when they drift: a backend code
// gains no copy (the user sees a raw slug), or the map keeps a key for a code
// the server never sends (dead copy that hides a typo). This test fails on
// either, so the two sides have to be reconciled in the same change.

// Resolve from the test file's own path rather than a `new URL(...)` relative
// to `import.meta.url`: Vite rewrites any path outside the web root (apps/web)
// to an `http://.../@fs/...` URL that `readFileSync` cannot open.
const thisDir = path.dirname(fileURLToPath(import.meta.url));
const errorsPy = path.resolve(thisDir, "../../../../apps/api/pi_dash/managed_runner/errors.py");

/** String values declared on the `ManagedRunnerReason` class in errors.py. */
function managedRunnerReasonCodes(): string[] {
  let source: string;
  try {
    source = readFileSync(errorsPy, "utf8");
  } catch (cause) {
    throw new Error(
      `Cannot read ${errorsPy}. This test reads the Python source of truth and expects ` +
        "the full monorepo checkout (apps/api alongside apps/web).",
      { cause }
    );
  }
  const classStart = source.indexOf("class ManagedRunnerReason");
  expect(classStart, "ManagedRunnerReason class not found in errors.py").toBeGreaterThan(-1);
  const nextClass = source.indexOf("\nclass ", classStart + 1);
  const body = source.slice(classStart, nextClass === -1 ? undefined : nextClass);
  const codes = [...body.matchAll(/^\s+[A-Z_]+\s*=\s*"([a-z_]+)"/gm)].map((match) => match[1]);
  expect(codes.length, "no reason codes parsed from ManagedRunnerReason").toBeGreaterThan(0);
  return codes;
}

// Codes the desktop can receive that are NOT ManagedRunnerReason members —
// emitted as literals by the credential and permission layers. Kept explicit
// (with source pointers) because they live outside the enum:
//   - assistant/views/agent_profile.py::_classify_credential_error
//   - managed_runner/permissions.py::IsDesktopSession.message
const NON_ENUM_DESKTOP_CODES = ["gateway_session_revoked", "gateway_unavailable", "desktop_session_required"];

// The desktop resolves this one silently (it enrolls a runner and retries), so
// it is never shown to the user and deliberately carries no copy.
const NOT_SURFACED_TO_USER = new Set(["no_managed_runner_for_project"]);

describe("agent runtime reason-code copy", () => {
  const backendCodes = new Set([...managedRunnerReasonCodes(), ...NON_ENUM_DESKTOP_CODES]);
  const mapKeys = new Set(Object.keys(AGENT_RUNTIME_REASON_MESSAGES));

  it("has copy for every backend code the desktop shows the user", () => {
    const missing = [...backendCodes].filter((code) => !NOT_SURFACED_TO_USER.has(code) && !mapKeys.has(code));
    expect(missing, `add copy for these reason codes: ${missing.join(", ")}`).toEqual([]);
  });

  it("has no copy keyed to a code the backend never emits", () => {
    const orphans = [...mapKeys].filter((code) => !backendCodes.has(code));
    expect(orphans, `these keys match no backend reason code: ${orphans.join(", ")}`).toEqual([]);
  });
});
