// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Example parity scenario (NEWFRONT-19): sign in with email plus password
// and list a project's issues. Green on apps/web first (the oracle); the
// same file must go green on apps/web_new once the auth and issues areas
// land. Rows: AUTH-001 (password sign-in), ISS-007 (flat issue list).
import { test, expect } from "../fixtures";
import { serverIssueNames, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["AUTH-001", "ISS-007"];

test(specTitle(ROWS, "sign in and list a project's issues"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  await test.step("sign in through the UI", async () => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
  });

  await test.step("open the seeded project issues list", async () => {
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  // The list populates asynchronously after navigation; poll the
  // user-visible read until every seeded title shows up.
  await test.step("every seeded issue is listed", async () => {
    await expect
      .poll(() => driver.visibleIssueNames(), { timeout: 60_000 })
      .toEqual(expect.arrayContaining([...seed.issueNames]));
  });

  await test.step("the server agrees with the screen", async () => {
    const session = await signInSession(seed.email, seed.password);
    const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
    expect(new Set(server)).toEqual(new Set(seed.issueNames));
    // The read also contains surrounding chrome text, so this is a subset
    // check: every issue the server reports for the project is on screen.
    const visible = await driver.visibleIssueNames();
    for (const name of server) expect(visible).toContain(name);
  });
});
