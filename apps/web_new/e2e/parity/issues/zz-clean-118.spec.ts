// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY CLEANUP — NEWFRONT-118. NOT COMMITTED. Deleted before PR.
// Removes orphans left by failed scenarios: scratch projects, seed-project
// states/labels/cycles/modules not in the seed, restores seed prefs/dates.
import { test } from "../fixtures";
import {
  seedProjectUserProperties,
  serverIssueDetails,
  serverIssues,
  serverListStates,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjects,
  signInFreshUser,
} from "../helpers/api";

test("cleanup: drop 118 orphans", async ({ seed }) => {
  test.setTimeout(300_000);
  const apiBase = process.env["PARITY_API_URL"] ?? "http://localhost:18032";
  const owner = await signInFreshUser(seed.email, seed.password, apiBase);
  const projects = await serverProjects(seed.workspaceSlug, owner.cookie, apiBase);
  console.log("CLEAN-PROJECTS:" + JSON.stringify(projects.map((p) => ({ id: p.id, name: p.name }))));
  for (const project of projects) {
    if (project.id === seed.projectId) continue;
    const res = await fetch(`${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${project.id}/`, {
      method: "DELETE",
      headers: { cookie: owner.cookie },
    });
    console.log(`CLEAN-PROJECT-DEL:${project.name ?? project.id} status=${res.status}`);
  }
  const states = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie, apiBase);
  console.log("CLEAN-STATES:" + JSON.stringify(states.map((s) => s.name)));
  for (const state of states) {
    if (state.name === "Todo") continue;
    const res = await fetch(
      `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/states/${state.id}/`,
      { method: "DELETE", headers: { cookie: owner.cookie } }
    );
    console.log(`CLEAN-STATE-DEL:${state.name} status=${res.status}`);
  }
  const labelsRes = await fetch(
    `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issue-labels/`,
    { headers: { cookie: owner.cookie } }
  );
  const labels = (await labelsRes.json()) as Array<{ id: string; name: string }>;
  console.log("CLEAN-LABELS:" + JSON.stringify(labels.map((l) => l.name)));
  for (const label of labels) {
    const res = await fetch(
      `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issue-labels/${label.id}/`,
      { method: "DELETE", headers: { cookie: owner.cookie } }
    );
    console.log(`CLEAN-LABEL-DEL:${label.name} status=${res.status}`);
  }
  const cyclesRes = await fetch(`${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/`, {
    headers: { cookie: owner.cookie },
  });
  const cycles = (await cyclesRes.json()) as
    | { results?: Array<{ id: string; name: string }> }
    | Array<{
        id: string;
        name: string;
      }>;
  const cycleRows = Array.isArray(cycles) ? cycles : (cycles.results ?? []);
  for (const cycle of cycleRows) {
    const res = await fetch(
      `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}/`,
      { method: "DELETE", headers: { cookie: owner.cookie } }
    );
    console.log(`CLEAN-CYCLE-DEL:${cycle.name} status=${res.status}`);
  }
  const modulesRes = await fetch(
    `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/modules/`,
    {
      headers: { cookie: owner.cookie },
    }
  );
  const modules = (await modulesRes.json()) as
    | { results?: Array<{ id: string; name: string }> }
    | Array<{
        id: string;
        name: string;
      }>;
  const moduleRows = Array.isArray(modules) ? modules : (modules.results ?? []);
  for (const module of moduleRows) {
    const res = await fetch(
      `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${module.id}/`,
      { method: "DELETE", headers: { cookie: owner.cookie } }
    );
    console.log(`CLEAN-MODULE-DEL:${module.name} status=${res.status}`);
  }
  // Restore every seed issue to pristine shape (Todo, no labels/dates).
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie, apiBase);
  const todo = states.find((s) => s.name === "Todo");
  for (const row of rows) {
    if (!seed.issueNames.includes(row.name)) {
      const res = await fetch(
        `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${row.id}/`,
        {
          method: "DELETE",
          headers: { cookie: owner.cookie },
        }
      );
      console.log(`CLEAN-ISSUE-DEL:${row.name} status=${res.status}`);
      continue;
    }
    const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, row.id, owner.cookie, apiBase);
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      row.id,
      {
        state_id: todo?.id,
        label_ids: [],
        assignee_ids: [],
        priority: "none",
        start_date: null,
        target_date: null,
        parent_id: null,
        sort_order: details.sequenceId * 10000 + 5000,
      },
      owner.cookie,
      apiBase
    );
  }
  const prefs = seedProjectUserProperties();
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, owner.cookie, {
    display_filters: { ...prefs.display_filters, sub_group_by: null, show_empty_groups: true },
    display_properties: prefs.display_properties,
  });
  console.log("CLEAN-DONE");
});
