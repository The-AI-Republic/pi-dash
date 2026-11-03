// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings navigation model (PSET-001/PSET-003/PSET-004, from
// parity/specs/project-settings-shell.md). Pure: role-filtered groups,
// suffix resolution both slash forms, the slash-sensitive active marker
// and the per-page gate matrix.
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/permissions.js";
import {
  admitsStanding,
  decidePageGate,
  effectiveProjectRole,
  findTabByKey,
  findTabBySuffix,
  findVisibleTab,
  isAdminStanding,
  isMemberStanding,
  isTabActive,
  PROJECT_SETTINGS_TABS,
  projectSettingsRoot,
  projectSettingsTabHref,
  resolveSettingsAddress,
  selectSettingsNav,
} from "./tabs.js";

describe("effectiveProjectRole", () => {
  it("lifts a workspace admin to project admin standing", () => {
    expect(effectiveProjectRole(ROLE_GUEST, ROLE_ADMIN)).toBe(ROLE_ADMIN);
    expect(effectiveProjectRole(ROLE_MEMBER, ROLE_ADMIN)).toBe(ROLE_ADMIN);
    expect(effectiveProjectRole(ROLE_ADMIN, ROLE_MEMBER)).toBe(ROLE_ADMIN);
  });

  it("keeps the project role without a workspace admin", () => {
    expect(effectiveProjectRole(ROLE_GUEST, ROLE_GUEST)).toBe(ROLE_GUEST);
    expect(effectiveProjectRole(ROLE_MEMBER, null)).toBe(ROLE_MEMBER);
  });

  it("stays unresolved without a project role", () => {
    expect(effectiveProjectRole(null, ROLE_ADMIN)).toBeNull();
    expect(effectiveProjectRole(null, null)).toBeNull();
  });
});

const WS = "acme";
const PID = "223e4567-e89b-12d3-a456-426614174001";

function labels(role: number | null): string[] {
  return selectSettingsNav(role).flatMap((group) => group.tabs.map((tab) => tab.label));
}

describe("selectSettingsNav", () => {
  it("shows admins all fourteen tabs in four groups", () => {
    const groups = selectSettingsNav(ROLE_ADMIN);
    expect(groups.map((group) => group.label)).toEqual(["General", "Features", "Work structure", "Execution"]);
    expect(labels(ROLE_ADMIN)).toEqual([
      "General",
      "Members",
      "Cycles",
      "Modules",
      "Views",
      "Pages",
      "Intake",
      "States",
      "Labels",
      "Estimates",
      "Automations",
      "Repository",
      "Schedulers",
      "AI Workers",
    ]);
  });

  it("shows members general, work structure and AI workers", () => {
    expect(selectSettingsNav(ROLE_MEMBER).map((group) => group.label)).toEqual([
      "General",
      "Work structure",
      "Execution",
    ]);
    expect(labels(ROLE_MEMBER)).toEqual(["General", "Members", "States", "Labels", "AI Workers"]);
  });

  it("shows guests general and members, with the general group only", () => {
    expect(selectSettingsNav(ROLE_GUEST).map((group) => group.key)).toEqual(["general"]);
    expect(labels(ROLE_GUEST)).toEqual(["General", "Members"]);
  });

  it("hides everything for outsiders", () => {
    expect(selectSettingsNav(null)).toEqual([]);
  });
});

describe("standing helpers", () => {
  it("rank project roles", () => {
    expect(isMemberStanding(ROLE_MEMBER)).toBe(true);
    expect(isMemberStanding(ROLE_ADMIN)).toBe(true);
    expect(isMemberStanding(ROLE_GUEST)).toBe(false);
    expect(isMemberStanding(null)).toBe(false);
    expect(isAdminStanding(ROLE_ADMIN)).toBe(true);
    expect(isAdminStanding(ROLE_MEMBER)).toBe(false);
    expect(isAdminStanding(null)).toBe(false);
  });

  it("admit every standing", () => {
    expect(admitsStanding(ROLE_GUEST, "everyone")).toBe(true);
    expect(admitsStanding(ROLE_GUEST, "member")).toBe(false);
    expect(admitsStanding(ROLE_MEMBER, "member")).toBe(true);
    expect(admitsStanding(ROLE_MEMBER, "admin")).toBe(false);
    expect(admitsStanding(ROLE_ADMIN, "admin")).toBe(true);
    expect(admitsStanding(null, "everyone")).toBe(false);
  });
});

describe("addresses", () => {
  it("build slash-terminated tab addresses", () => {
    const general = findTabByKey("general");
    const members = findTabByKey("members");
    if (general === undefined || members === undefined) throw new Error("registry lost a core tab");
    expect(projectSettingsRoot(WS, PID)).toBe(`/${WS}/settings/projects/${PID}`);
    expect(projectSettingsTabHref(WS, PID, general)).toBe(`/${WS}/settings/projects/${PID}/`);
    expect(projectSettingsTabHref(WS, PID, members)).toBe(`/${WS}/settings/projects/${PID}/members/`);
  });

  it("find tabs by key and suffix", () => {
    expect(findTabByKey("github")?.label).toBe("Repository");
    expect(findTabByKey("nope" as never)).toBeUndefined();
    expect(findTabBySuffix("")?.key).toBe("general");
    expect(findTabBySuffix("/")?.key).toBe("general");
    expect(findTabBySuffix("/members/")?.key).toBe("members");
    expect(findTabBySuffix("/features/cycles")?.key).toBe("cycles");
    expect(findTabBySuffix("/features")?.key).toBeUndefined();
  });

  it("resolve both slash forms and reject foreign or unknown addresses", () => {
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}`, WS)).toMatchObject({
      projectId: PID,
      tab: { key: "general" },
    });
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}/`, WS)).toMatchObject({
      projectId: PID,
      tab: { key: "general" },
    });
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}/members`, WS)).toMatchObject({
      tab: { key: "members" },
    });
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}/members/`, WS)).toMatchObject({
      tab: { key: "members" },
    });
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}/features/views/`, WS)).toMatchObject({
      tab: { key: "views" },
    });
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}//members`, WS)).toMatchObject({
      tab: { key: "members" },
    });
    expect(resolveSettingsAddress(`/${WS}/settings/projects`, WS)).toBeUndefined();
    expect(resolveSettingsAddress(`/${WS}/settings/projects/`, WS)).toBeUndefined();
    expect(resolveSettingsAddress(`/${WS}/settings/projects//members`, WS)).toBeUndefined();
    expect(resolveSettingsAddress(`/${WS}/settings/projects/${PID}/nope`, WS)).toBeUndefined();
    expect(resolveSettingsAddress(`/other/settings/projects/${PID}/members`, WS)).toBeUndefined();
  });

  it("rejects an address with an empty project segment", () => {
    expect(resolveSettingsAddress(`/${WS}/settings/projects/`, WS)).toBeUndefined();
    expect(resolveSettingsAddress(`/${WS}/settings/projects`, WS)).toBeUndefined();
    expect(resolveSettingsAddress(`/${WS}/settings/projects//`, WS)).toBeUndefined();
  });

  it("mark only the canonical slash form active", () => {
    const general = findTabByKey("general");
    const members = findTabByKey("members");
    if (general === undefined || members === undefined) throw new Error("registry lost a core tab");
    expect(isTabActive(`/${WS}/settings/projects/${PID}/`, WS, PID, general)).toBe(true);
    expect(isTabActive(`/${WS}/settings/projects/${PID}`, WS, PID, general)).toBe(false);
    expect(isTabActive(`/${WS}/settings/projects/${PID}/members/`, WS, PID, members)).toBe(true);
    expect(isTabActive(`/${WS}/settings/projects/${PID}/members`, WS, PID, members)).toBe(false);
  });

  it("does not mark a sub-address or an unknown path as its tab", () => {
    const members = findTabBySuffix("/members");
    if (members === undefined) throw new Error("registry lost the members tab");
    expect(isTabActive(`/${WS}/settings/projects/${PID}/members/details/`, WS, PID, members)).toBe(false);
    expect(isTabActive(`/${WS}/projects/${PID}/settings/members/`, WS, PID, members)).toBe(false);
  });

  it("find a visible tab for the role", () => {
    expect(findVisibleTab("nope" as never, ROLE_ADMIN)).toBeUndefined();
    expect(findVisibleTab("states", ROLE_MEMBER)?.label).toBe("States");
    expect(findVisibleTab("states", ROLE_GUEST)).toBeUndefined();
    expect(findVisibleTab("github", ROLE_MEMBER)).toBeUndefined();
    expect(findVisibleTab("general", ROLE_GUEST)?.label).toBe("General");
  });
});

describe("decidePageGate", () => {
  function gate(key: string, role: number | null): string {
    const tab = findTabByKey(key as never);
    if (tab === undefined) throw new Error(`no tab ${key}`);
    return decidePageGate(tab, role);
  }

  it("refuses outsiders everywhere", () => {
    for (const tab of PROJECT_SETTINGS_TABS) expect(decidePageGate(tab, null)).toBe("denied");
    expect(gate("general", 0)).toBe("denied");
  });

  it("never refuses General", () => {
    expect(gate("general", ROLE_GUEST)).toBe("allowed");
    expect(gate("general", ROLE_MEMBER)).toBe("allowed");
    expect(gate("general", ROLE_ADMIN)).toBe("allowed");
  });

  it("keeps members stricter than their entry for guests", () => {
    expect(gate("members", ROLE_GUEST)).toBe("denied");
    expect(gate("members", ROLE_MEMBER)).toBe("allowed");
    expect(gate("members", ROLE_ADMIN)).toBe("allowed");
  });

  it("admits members to states, labels and the wider repository page", () => {
    for (const key of ["states", "labels", "github", "ai_workers"]) {
      expect(gate(key, ROLE_GUEST)).toBe("denied");
      expect(gate(key, ROLE_MEMBER)).toBe("allowed");
      expect(gate(key, ROLE_ADMIN)).toBe("allowed");
    }
    expect(gate("states", ROLE_GUEST)).toBe("denied");
  });

  it("keeps the remaining tabs admin-only", () => {
    for (const key of ["cycles", "modules", "views", "pages", "intake", "estimates", "automations", "schedulers"]) {
      expect(gate(key, ROLE_GUEST)).toBe("denied");
      expect(gate(key, ROLE_MEMBER)).toBe("denied");
      expect(gate(key, ROLE_ADMIN)).toBe("allowed");
    }
  });
});
