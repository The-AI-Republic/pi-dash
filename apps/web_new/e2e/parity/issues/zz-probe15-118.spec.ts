// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4b. Replicate ISS-039 step 2, longer
// (target_date sub-group): fetches, errors, DOM. Deleted before PR.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverDeleteProject,
  serverListStates,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe15: target_date subgroup render", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KQ${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  const a = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe a ${suffix}`, home.id);
  const b = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe b ${suffix}`, home.id);
  const c = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe c ${suffix}`, home.id);
  await serverPatchIssue(seed.workspaceSlug, projectId, a, { target_date: "2026-12-01" }, owner.cookie);
  await serverPatchIssue(seed.workspaceSlug, projectId, b, { target_date: "2026-12-02" }, owner.cookie);
  await serverPatchIssue(seed.workspaceSlug, projectId, c, { target_date: "2026-12-03" }, owner.cookie);
  page.on("response", (res) => {
    const url = res.url();
    if (!url.includes("/issues/") || !url.includes("sub_group_by")) return;
    void res
      .json()
      .then((body: unknown) => {
        const results = (body as { results?: Record<string, { results?: Record<string, { results?: unknown[] }> }> })
          .results;
        const deep: Record<string, unknown> = {};
        for (const [gk, gv] of Object.entries(results ?? {})) {
          deep[gk.slice(0, 6)] = {};
          for (const [sk, sv] of Object.entries(gv.results ?? {})) {
            (deep[gk.slice(0, 6)] as Record<string, unknown>)[sk.slice(0, 12)] = Array.isArray(sv.results)
              ? sv.results.map((r) => (r as { name?: string }).name ?? "?")
              : typeof sv;
          }
        }
        console.log(`P14-FETCH:${url.slice(-60)} => ${JSON.stringify(deep).slice(0, 400)}`);
      })
      .catch(() => console.log(`P14-FETCH:non-json ${res.status()}`));
  });
  page.on("pageerror", (err) => console.log(`P14-PAGEERROR:${String(err).slice(0, 300)}`));
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      group_by: "state",
      sub_group_by: "target_date",
      show_empty_groups: true,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  await page.waitForTimeout(90_000);
  console.log(`P14-URL:${page.url()}`);
  await page.screenshot({ path: "/tmp/p14_board.png" });
  console.log(`P14-ACTIVE:${await driver.boardActiveLayout().catch((e: unknown) => String(e).slice(0, 100))}`);
  console.log(`P14-VIS:${await driver.kanbanBoardVisible().catch(() => false)}`);
  console.log(
    `P14-LANES:${JSON.stringify(await driver.kanbanSwimlanes().catch((e: unknown) => String(e).slice(0, 120)))}`
  );
  const bars = await page.locator('main div[class*="top-[50px]"]').count();
  const bodies = await page.locator('main div[id*="__"]').count();
  const shells = await page.locator("main div.group.relative.flex.flex-shrink-0.flex-col").count();
  console.log(`P14-STRUCT:bars=${bars} bodies=${bodies} shells=${shells}`);
  const mainText = await page
    .locator("main")
    .first()
    .innerText()
    .catch((e: unknown) => String(e).slice(0, 100));
  console.log(`P14-TEXT:${mainText.slice(0, 500).replace(/\s+/g, " ")}`);
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P14-DONE");
});
