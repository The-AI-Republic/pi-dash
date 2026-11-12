// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): public entry redirects, rows CLOUD-001
// (signed-out root forwards to the marketing surface preserving a deep
// target) and CLOUD-002 (legacy sign-in URLs resolve to the cloud login
// experience), green on the cloud-edition old app first. The same file
// must go green on apps/web_new once the cloud marketing area lands there.
//
// Observed behavior notes (cloud oracle): the root route redirects
// client-side — a bare visit lands on the marketing home, while a
// next_path value routes straight to login carrying it as a parameter.
// The older /sign-in and /signin aliases redirect to the root (which
// then applies the root rule), and /login renders its own screen in
// place. The login card itself belongs to NEWFRONT-438; here only the
// URL behavior is proven.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-001"], "signed-out root lands on the marketing home"),
  { tag: specTags(["CLOUD-001"]) },
  async ({ driver }) => {
    await driver.openPath("/");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/home");
    // The landing is the real marketing home, not a dead redirect target.
    expect(await driver.cloudHomeHeroActionHrefs()).toEqual(["/login", "/downloads", "/docs"]);
  }
);

test(
  specTitle(["CLOUD-001"], "root preserves a deep target as a login parameter"),
  { tag: specTags(["CLOUD-001"]) },
  async ({ driver }) => {
    // The login page auto-resumes the provider flow when a return target
    // exists (CLOUD-012, owned by NEWFRONT-438), which would navigate away
    // before the redirect target can be read. The explicit-logout marker
    // (whose semantics belong to CLOUD-014) suppresses only that resume,
    // so this stays a pure redirect assertion: root must still route to
    // login carrying the target.
    await driver.page.addInitScript(() => {
      window.sessionStorage.setItem("pidash:explicitly-logged-out", "1");
    });
    await driver.openPath("/?next_path=%2Fdocs");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toMatch(/^\/login\?next_path=%2Fdocs$/);
  }
);

test(
  specTitle(["CLOUD-002"], "login renders in place while legacy aliases bounce through root"),
  { tag: specTags(["CLOUD-002"]) },
  async ({ driver }) => {
    await driver.openPath("/login");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/login");

    await driver.openPath("/sign-in");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/home");
    expect(await driver.cloudHomeHeroActionHrefs()).toEqual(["/login", "/downloads", "/docs"]);

    await driver.openPath("/signin");
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/home");
  }
);
