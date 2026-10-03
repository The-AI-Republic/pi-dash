// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4. Does a UI drag into In Progress
// land, or fall back to Backlog like the PATCH path? Deleted before PR.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverDeleteProject,
  serverIssueDetails,
  serverListStates,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe16: drag into In Progress", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KQ${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  const name = `KB probe ${suffix} 1`;
  const id = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, name, home.id);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", order_by: "sort_order", group_by: "state" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(owner));
  await driver.kanbanOpenBoard();
  const cols = await driver.kanbanColumns();
  console.log(`P16-COLS:${JSON.stringify(cols.map((c) => c.name))}`);
  const target = states.find((s) => s.name === "In Progress");
  console.log(`P16-TARGET:${target?.name} ${target?.id.slice(0, 8)}`);
  await driver.kanbanDragCardToColumnEnd(name, "In Progress");
  await page.waitForTimeout(5_000);
  const details = await serverIssueDetails(seed.workspaceSlug, projectId, id, owner.cookie);
  const landed = states.find((s) => s.id === details.stateId)?.name ?? details.stateId.slice(0, 8);
  console.log(`P16-LANDED:${landed} (want In Progress)`);
  console.log(`P16-COLS-AFTER:${JSON.stringify((await driver.kanbanColumns()).map((c) => `${c.name}:${c.count}`))}`);
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P16-DONE");
});
