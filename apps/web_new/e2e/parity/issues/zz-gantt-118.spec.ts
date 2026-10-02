// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY GANTT PROBE — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
// Scratch project only: never touches the seed project (chunk A runs there).
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverDefaultStateId,
  serverDeleteProject,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProfileStartOfWeek,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe: gantt initial zoom rows reorder-toast", async ({ driver, seed }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `GT probe ${suffix}`, `GP${suffix}`);
  const home = await serverDefaultStateId(seed.workspaceSlug, projectId, owner.cookie);
  const aId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `GT a ${suffix}`, home);
  await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `GT b ${suffix}`, home);
  await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `GT c ${suffix}`, home);
  await serverPatchIssue(
    seed.workspaceSlug,
    projectId,
    aId,
    { start_date: "2026-09-28", target_date: "2026-10-05" },
    owner.cookie
  );
  console.log("PROBE-SOW:" + String(await serverProfileStartOfWeek(owner.cookie)));
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: { ...before.displayFilters, layout: "gantt_chart", group_by: null, order_by: "sort_order" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  await driver.ganttOpenTimeline();
  console.log("PROBE-ZOOM0:" + String(await driver.ganttActiveZoom()));
  console.log("PROBE-HEADER:" + JSON.stringify(await driver.ganttHeader()));
  console.log("PROBE-ROWS:" + JSON.stringify(await driver.ganttSidebarRows()));
  console.log("PROBE-BARA:" + String(await driver.ganttBarExists(`GT a ${suffix}`)));
  console.log("PROBE-BARB:" + String(await driver.ganttBarExists(`GT b ${suffix}`)));
  console.log("PROBE-ADDVIS:" + String(await driver.ganttRowAddVisible(`GT b ${suffix}`)));
  console.log("PROBE-TODAYVIS:" + String(await driver.ganttTodayVisible()));
  console.log("PROBE-WKND:" + String(await driver.ganttWeekendTinted()));
  console.log("PROBE-WEEKSTARTS:" + JSON.stringify(await driver.ganttWeekRowStarts()));
  console.log("PROBE-DAYW:" + String(await driver.ganttDayWidth()));
  // Non-manual reorder attempt: toast or silent?
  const current = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: { ...current.displayFilters, order_by: "priority" },
  });
  await driver.boardReloadIssues();
  await driver.ganttOpenTimeline();
  const orderBefore = await driver.ganttSidebarOrder();
  await driver.ganttAttemptRowBefore(`GT c ${suffix}`, `GT a ${suffix}`);
  const orderAfter = await driver.ganttSidebarOrder();
  console.log("PROBE-ORDER-BEFORE:" + JSON.stringify(orderBefore));
  console.log("PROBE-ORDER-AFTER:" + JSON.stringify(orderAfter));
  console.log("PROBE-TOAST:" + JSON.stringify(await driver.boardLastToast()));
  console.log("PROBE-LOADING:" + String(await driver.ganttLoadingObservedOnReload()));
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("PROBE-DONE");
});
