// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): desktop sign-in, row CLOUD-018
// (sign-in completes through the system browser and a deep link), green
// on the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): the desktop marker switches
// the sign-in endpoint to the desktop OIDC client branch (the scratch
// stack has no desktop client configured, so it fails loudly with a
// redirect instead of hanging); the web branch is unaffected. The
// webview exchange endpoint refuses incomplete or unknown flows with an
// error redirect, never a hang or a 500. The login card in a desktop
// shell never runs the handoff inside the embedded view. The native
// halves — the system-browser open and the deep-link return into the
// webview — need the desktop build and have no web-observable side
// beyond these contracts (same precedent as the DESK oracle rows).
import { test, expect } from "../fixtures";
import { cloudDesktopExchangeStatus, cloudSignInRedirect } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-018"], "desktop marker switches the sign-in branch"),
  { tag: specTags(["CLOUD-018"]) },
  async () => {
    const desktop = await cloudSignInRedirect({ desktop: "1", return_to: "/" });
    expect(desktop.status).toBe(302);
    expect(desktop.location).toBe("/sign-in?error=desktop_not_configured");
    const web = await cloudSignInRedirect({ return_to: "/" });
    expect(web.status).toBe(302);
    expect(new URL(web.location).pathname).toBe("/auth/authorize");
  }
);

test(
  specTitle(["CLOUD-018"], "exchange refuses incomplete or unknown flows"),
  { tag: specTags(["CLOUD-018"]) },
  async () => {
    const missing = await cloudDesktopExchangeStatus({});
    expect(missing.status).toBe(302);
    expect(missing.location).toBe("/sign-in?error=missing_params");
    const bogus = await cloudDesktopExchangeStatus({ code: "x", state: "y" });
    expect(bogus.status).toBe(302);
    expect(bogus.location).toBe("/sign-in?error=invalid_state");
  }
);

test(
  specTitle(["CLOUD-018"], "desktop card click never runs an embedded flow"),
  { tag: specTags(["CLOUD-018"]) },
  async ({ driver }) => {
    await driver.cloudStubTauri();
    await driver.cloudArmConsoleErrors();
    await driver.cloudArmSignInCapture();
    await driver.openPath("/login");
    await driver.cloudClickLoginContinue();
    await driver.page.waitForTimeout(3_000);
    expect(await driver.currentPath()).toBe("/login");
    expect(await driver.cloudSignInRequests()).toHaveLength(0);
    expect((await driver.cloudConsoleErrors()).some((line) => line.includes("VITE_API_BASE_URL"))).toBe(true);
  }
);
