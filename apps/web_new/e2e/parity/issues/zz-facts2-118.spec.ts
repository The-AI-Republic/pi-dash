// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY FACTS PROBE 2 — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
import { test } from "../fixtures";
import {
  seedProjectUserProperties,
  serverIssueDetails,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  signInSession,
} from "../helpers/api";

test("facts: profile grouped counts priorities", async ({ seed }) => {
  test.setTimeout(120_000);
  const apiBase = process.env["PARITY_API_URL"] ?? "http://localhost:18032";
  const session = await signInSession(seed.email, seed.password, apiBase);
  const base = `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}`;
  const meRes = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie: session } });
  console.log("FACT-ME-FULL:" + JSON.stringify(await meRes.json()));
  const profRes = await fetch(`${apiBase}/api/users/me/profile/`, { headers: { cookie: session } });
  console.log("FACT-PROFILE-FULL:" + JSON.stringify(await profRes.json()));
  const groupedRes = await fetch(`${base}/issues/?group_by=state&order_by=sort_order`, {
    headers: { cookie: session },
  });
  const grouped = (await groupedRes.json()) as Record<string, unknown>;
  console.log("FACT-GROUPED-KEYS:" + JSON.stringify(Object.keys(grouped)) + " STATUS:" + String(groupedRes.status));
  console.log("FACT-RESULTS:" + JSON.stringify(grouped["results"]).slice(0, 1200));
  console.log("FACT-TOTALCOUNT:" + JSON.stringify(grouped["total_count"]));
  const first = await serverIssueDetails(
    seed.workspaceSlug,
    seed.projectId,
    "9d0f068b-d5a4-4d83-b352-6b7ed47013ce",
    session,
    apiBase
  );
  console.log("FACT-FIRST:" + JSON.stringify(first));
});

test("restore: seed prefs and null seed dates", async ({ seed }) => {
  test.setTimeout(120_000);
  const apiBase = process.env["PARITY_API_URL"] ?? "http://localhost:18032";
  const session = await signInSession(seed.email, seed.password, apiBase);
  const prefs = seedProjectUserProperties();
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
    display_filters: { ...prefs.display_filters, sub_group_by: null, show_empty_groups: true },
    display_properties: prefs.display_properties,
  });
  for (const id of [
    "9d0f068b-d5a4-4d83-b352-6b7ed47013ce",
    "ad403aa2-4f02-40d8-9f3e-3b257d1fc3cf",
    "400681dc-b77b-4f6a-8e36-8135beb5d5b0",
  ]) {
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      id,
      { start_date: null, target_date: null },
      session,
      apiBase
    );
  }
  console.log("RESTORE-DONE");
});
