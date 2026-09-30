// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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

  const visible = await driver.visibleIssueNames();

  await test.step("every seeded issue is listed", async () => {
    for (const name of seed.issueNames) expect(visible).toContain(name);
  });

  await test.step("the server agrees with the screen", async () => {
    const session = await signInSession(seed.email, seed.password);
    const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
    expect(new Set(server)).toEqual(new Set(seed.issueNames));
    expect(new Set(visible)).toEqual(new Set(server));
  });
});
