// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY FACTS PROBE — NEWFRONT-118. NOT COMMITTED. Deleted after recon.
import { test } from "../fixtures";
import {
  serverIssues,
  serverListStates,
  serverProjectIdentifier,
  serverProjectUserProperties,
  signInSession,
} from "../helpers/api";

test("facts: seed states issues prefs", async ({ seed }) => {
  test.setTimeout(120_000);
  const apiBase = process.env["PARITY_API_URL"] ?? "http://localhost:18032";
  const session = await signInSession(seed.email, seed.password, apiBase);
  const states = await serverListStates(seed.workspaceSlug, seed.projectId, session, apiBase);
  console.log("FACT-STATES:" + JSON.stringify(states));
  const issues = await serverIssues(seed.workspaceSlug, seed.projectId, session, apiBase);
  console.log("FACT-ISSUES:" + JSON.stringify(issues));
  const ident = await serverProjectIdentifier(seed.workspaceSlug, seed.projectId, session, apiBase);
  console.log("FACT-IDENT:" + JSON.stringify(ident));
  const prefs = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session, apiBase);
  console.log("FACT-PREFS:" + JSON.stringify(prefs));
  const meRes = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie: session } });
  const me = (await meRes.json()) as Record<string, unknown>;
  console.log(
    "FACT-ME:" +
      JSON.stringify({ week_start_day: me["week_start_day"], role: me["role"], is_onboarded: me["is_onboarded"] })
  );
  const propsRes = await fetch(
    `${apiBase}/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/user-properties/`,
    { headers: { cookie: session } }
  );
  console.log("FACT-PROPS-RAW:" + JSON.stringify(await propsRes.json()));
});
