// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

// The generator is plain ESM with no type declarations; this suite is its
// only in-tree consumer.
// @ts-expect-error -- plain JS module
import { entryRegex, loadEntries, parseEntry } from "../../../checks/coexistence-routes.mjs";

type GeneratedEntry = {
  entry: string;
  catchAll: boolean;
  segments: ReadonlyArray<{ kind: string; name?: string; value?: string }>;
};
type GeneratedHandler = { kind: "catchAll" } | { kind: "literal"; base: string } | { kind: "matcher"; regex: RegExp };

import {
  compileEntry,
  isMigratedPath,
  MIGRATED_RESERVED_TOP_LEVEL,
  MIGRATED_ROUTE_ENTRIES,
  normalizePathPrefix,
  reservedTopLevelOf,
  resolveLinkKind,
} from "./migrated-routes.js";

describe("normalizePathPrefix", () => {
  it("adds a leading slash and strips trailing slashes, queries and hashes", () => {
    expect(normalizePathPrefix("sign-in")).toBe("/sign-in");
    expect(normalizePathPrefix("/sign-in/")).toBe("/sign-in");
    expect(normalizePathPrefix("/sign-in?next=/")).toBe("/sign-in");
    expect(normalizePathPrefix("/sign-in#main")).toBe("/sign-in");
    expect(normalizePathPrefix("/")).toBe("/");
  });
});

describe("literal entries", () => {
  const entries = ["/sign-in", "/god-mode/users"];

  it("match exactly and below, but never a longer sibling segment", () => {
    expect(isMigratedPath("/sign-in", entries)).toBe(true);
    expect(isMigratedPath("/sign-in/", entries)).toBe(true);
    expect(isMigratedPath("/sign-in?next=/", entries)).toBe(true);
    expect(isMigratedPath("/god-mode/users/42", entries)).toBe(true);
    expect(isMigratedPath("/sign-invites", entries)).toBe(false);
    expect(isMigratedPath("/god-mode", entries)).toBe(false);
    expect(isMigratedPath("/", entries)).toBe(false);
  });

  it("ignore blank entries and treat / as everything", () => {
    expect(isMigratedPath("/anything", ["", "   "])).toBe(false);
    expect(isMigratedPath("/anything/deep", ["/"])).toBe(true);
    expect(isMigratedPath("/", ["/"])).toBe(true);
  });

  it("reject a double slash so a stray empty segment never matches", () => {
    expect(isMigratedPath("/sign-in//x", entries)).toBe(false);
  });
});

describe("workspace-scoped entries", () => {
  const entries = ["/sign-in", "/{ws}", "/{ws}/projects", "/{ws}/projects/*"];
  const reserved = ["api", "god-mode", "settings", "spaces"];

  it("match one workspace slug at their exact depth", () => {
    expect(isMigratedPath("/acme", entries, reserved)).toBe(true);
    expect(isMigratedPath("/acme/", entries, reserved)).toBe(true);
    expect(isMigratedPath("/acme/projects", entries, reserved)).toBe(true);
    expect(isMigratedPath("/acme/projects/", entries, reserved)).toBe(true);
    expect(isMigratedPath("/acme/projects?sort=name", entries, reserved)).toBe(true);
  });

  it("stay exact without a trailing *, so unbuilt pages stay on the old app", () => {
    const exact = ["/sign-in", "/{ws}", "/{ws}/projects"];
    expect(isMigratedPath("/acme/projects/1/issues", exact, reserved)).toBe(false);
    expect(isMigratedPath("/acme/browse/ACME-1", exact, reserved)).toBe(false);
    expect(isMigratedPath("/acme/active-cycles", exact, reserved)).toBe(false);
  });

  it("claim the whole subtree with a trailing *", () => {
    expect(isMigratedPath("/acme/projects/1/issues", ["/{ws}/projects/*"], reserved)).toBe(true);
    expect(isMigratedPath("/acme/projects/1/issues/relations", ["/{ws}/projects/*"], reserved)).toBe(true);
    expect(isMigratedPath("/acme/projects", ["/{ws}/projects/*"], reserved)).toBe(true);
  });

  it("never match an empty segment or a segment containing a dot", () => {
    expect(isMigratedPath("/", entries, reserved)).toBe(false);
    expect(isMigratedPath("//projects", entries, reserved)).toBe(false);
    expect(isMigratedPath("/acme.dev", entries, reserved)).toBe(false);
    expect(isMigratedPath("/acme.dev/projects", entries, reserved)).toBe(false);
    expect(isMigratedPath("/favicon.ico/projects", entries, reserved)).toBe(false);
  });
});

describe("reserved-path collisions", () => {
  const entries = ["/sign-in", "/{ws}", "/{ws}/projects"];

  it("keeps reserved top-level areas on the old app even though they are one segment", () => {
    for (const path of ["/sign-up", "/api", "/god-mode", "/onboarding", "/profile", "/settings", "/spaces"]) {
      expect(isMigratedPath(path, entries)).toBe(false);
    }
    expect(MIGRATED_RESERVED_TOP_LEVEL).toContain("sign-in");
    expect(isMigratedPath("/sign-in", ["/{ws}"])).toBe(false);
  });

  it("keeps reserved child paths on the old app too", () => {
    expect(isMigratedPath("/settings/acme/projects", entries)).toBe(false);
    expect(isMigratedPath("/api/projects", entries)).toBe(false);
    expect(isMigratedPath("/god-mode/projects", entries)).toBe(false);
    expect(isMigratedPath("/spaces/projects", entries)).toBe(false);
  });

  it("matches reserved names case-insensitively", () => {
    expect(isMigratedPath("/API", entries, MIGRATED_RESERVED_TOP_LEVEL)).toBe(false);
    expect(isMigratedPath("/Settings/acme/projects", entries, MIGRATED_RESERVED_TOP_LEVEL)).toBe(false);
    expect(isMigratedPath("/ACME/PROJECTS", entries, MIGRATED_RESERVED_TOP_LEVEL)).toBe(false);
    expect(isMigratedPath("/acme/Projects", entries, MIGRATED_RESERVED_TOP_LEVEL)).toBe(false);
  });

  it("excludes every literal entry's own first segment from variable matching", () => {
    const names = reservedTopLevelOf(["/sign-in", "/codex"], ["api"]);
    expect(names).toEqual(["api", "codex", "sign-in"]);
    expect(isMigratedPath("/codex", ["/codex", "/{ws}"], ["api"])).toBe(true);
    expect(isMigratedPath("/codex/projects", ["/codex", "/{ws}/projects"], ["api"])).toBe(true);
    expect(isMigratedPath("/codex/projects", ["/{ws}/projects"], ["codex"])).toBe(false);
  });

  it("only applies the exclusion to a variable first segment", () => {
    const literalFirst = ["/acme"];
    expect(isMigratedPath("/acme/projects", literalFirst, ["acme"])).toBe(true);
  });
});

describe("bundled list", () => {
  it("carries the auth entries, the workspace home and the Shell routes", () => {
    expect(MIGRATED_ROUTE_ENTRIES).toEqual(
      expect.arrayContaining([
        "/sign-in",
        "/sign-up",
        "/accounts",
        "/auth/device",
        "/invitations",
        "/workspace-invitations",
        "/onboarding",
        "/create-workspace",
        "/signin",
        "/login",
        "/register",
        "/settings/profile",
        "/profile",
        "/{ws}",
        "/{ws}/projects",
        "/{ws}/browse/{workItem}",
      ])
    );
    expect(isMigratedPath("/sign-in")).toBe(true);
    expect(isMigratedPath("/settings/profile/security")).toBe(true);
    expect(isMigratedPath("/profile/general")).toBe(true);
    expect(isMigratedPath("/acme")).toBe(true);
    expect(isMigratedPath("/acme/projects")).toBe(true);
    // Project issues moved with the Issues area gate (NEWFRONT-658).
    expect(isMigratedPath("/acme/projects/1/issues")).toBe(true);
    expect(isMigratedPath("/acme/browse/ACME-1")).toBe(true);
    expect(isMigratedPath("/acme/browse/ACME-1/")).toBe(true);
    expect(isMigratedPath("/acme/browse")).toBe(false);
  });

  it("serves every workspace-settings section and the webhook detail subtree", () => {
    for (const path of [
      "/acme/settings",
      "/acme/settings/members",
      "/acme/settings/billing",
      "/acme/settings/exports",
      "/acme/settings/integrations",
      "/acme/settings/webhooks",
      "/acme/settings/webhooks/00000000-0000-0000-0000-000000000000",
    ]) {
      expect(isMigratedPath(path), path).toBe(true);
    }
    // Project settings moved with their own area gate (NEWFRONT-606); the
    // profile legacy forwards plus any unknown section have no web_new page
    // at this address.
    for (const path of ["/acme/settings/api-tokens", "/acme/settings/account", "/acme/settings/unknown"]) {
      expect(isMigratedPath(path), path).toBe(false);
    }
    expect(resolveLinkKind("/acme/settings/members")).toBe("router");
    expect(resolveLinkKind("/acme/settings/unknown")).toBe("anchor");
  });

  it("serves the runner area and AI dev machines from web_new", () => {
    for (const path of [
      "/acme/runners",
      "/acme/runners/",
      "/acme/runners/approvals",
      "/acme/runners/runs",
      "/acme/runners/runs/00000000-0000-0000-0000-000000000000",
      "/acme/runners/chat/00000000-0000-0000-0000-000000000000",
      "/acme/runners/detail/00000000-0000-0000-0000-000000000000",
      "/acme/ai-dev-machines",
      "/acme/ai-dev-machines/00000000-0000-0000-0000-000000000000",
      "/acme/projects/00000000-0000-0000-0000-000000000000/runners",
      "/acme/projects/00000000-0000-0000-0000-000000000000/runners/runs",
      "/acme/projects/00000000-0000-0000-0000-000000000000/runners/detail/x",
    ]) {
      expect(isMigratedPath(path), path).toBe(true);
    }
    // The project runner subtree is scoped to the runners segment: sibling
    // project pages and other workspaces' runner-looking segments stay on
    // the old app. (Project issues moved with their own area gate,
    // NEWFRONT-658, and are asserted in the issues block below.)
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000",
      "/acme/projects/00000000-0000-0000-0000-000000000000/cycles",
      "/acme/runner",
      "/acme/runs",
      "/acme/ai-dev-machine",
    ]) {
      expect(isMigratedPath(path), path).toBe(false);
    }
    expect(resolveLinkKind("/acme/runners/approvals")).toBe("router");
    expect(resolveLinkKind("/acme/runner")).toBe("anchor");
  });

  it("serves project issues and the issue detail subtree from web_new", () => {
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000/issues",
      "/acme/projects/00000000-0000-0000-0000-000000000000/issues/",
      "/acme/projects/00000000-0000-0000-0000-000000000000/issues/00000000-0000-0000-0000-000000000000",
    ]) {
      expect(isMigratedPath(path), path).toBe(true);
    }
    // Sibling project areas stay on the old app until their own gates land.
    // (Project intake moved with its own area gate, NEWFRONT-695, and is
    // asserted in the intake block below.)
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000",
      "/acme/projects/00000000-0000-0000-0000-000000000000/cycles",
      "/acme/projects/00000000-0000-0000-0000-000000000000/modules",
      // (Project views moved with their own area gate, NEWFRONT-745, and are
      // asserted in the views block below.)
      "/acme/projects/00000000-0000-0000-0000-000000000000/issues-archive",
    ]) {
      expect(isMigratedPath(path), path).toBe(false);
    }
    expect(resolveLinkKind("/acme/projects/00000000-0000-0000-0000-000000000000/issues")).toBe("router");
    expect(resolveLinkKind("/acme/projects/00000000-0000-0000-0000-000000000000/cycles")).toBe("anchor");
  });

  it("serves project intake and the retired inbox redirect from web_new", () => {
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000/intake",
      "/acme/projects/00000000-0000-0000-0000-000000000000/intake/",
      "/acme/projects/00000000-0000-0000-0000-000000000000/inbox",
      "/acme/projects/00000000-0000-0000-0000-000000000000/inbox/",
    ]) {
      expect(isMigratedPath(path), path).toBe(true);
    }
    // Near-miss spellings stay on the old app.
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000/intakes",
      "/acme/projects/00000000-0000-0000-0000-000000000000/inboxes",
    ]) {
      expect(isMigratedPath(path), path).toBe(false);
    }
    expect(resolveLinkKind("/acme/projects/00000000-0000-0000-0000-000000000000/intake")).toBe("router");
    expect(resolveLinkKind("/acme/projects/00000000-0000-0000-0000-000000000000/cycles")).toBe("anchor");
  });

  it("serves workspace analytics and the tab subtree from web_new", () => {
    for (const path of [
      "/acme/analytics",
      "/acme/analytics/",
      "/acme/analytics/overview",
      "/acme/analytics/work-items",
      "/acme/analytics/work-items/",
    ]) {
      expect(isMigratedPath(path), path).toBe(true);
    }
    // Near-miss spellings stay on the old app.
    for (const path of ["/acme/analytics-archive", "/acme/analyticsx"]) {
      expect(isMigratedPath(path), path).toBe(false);
    }
    expect(resolveLinkKind("/acme/analytics/overview")).toBe("router");
    expect(resolveLinkKind("/acme/analytics-archive")).toBe("anchor");
  });

  it("serves project and workspace views from web_new", () => {
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000/views",
      "/acme/projects/00000000-0000-0000-0000-000000000000/views/",
      "/acme/projects/00000000-0000-0000-0000-000000000000/views/00000000-0000-0000-0000-000000000000",
      "/acme/workspace-views",
      "/acme/workspace-views/",
      "/acme/workspace-views/all-issues",
      "/acme/workspace-views/00000000-0000-0000-0000-000000000000",
    ]) {
      expect(isMigratedPath(path), path).toBe(true);
    }
    // Near-miss spellings stay on the old app.
    for (const path of [
      "/acme/projects/00000000-0000-0000-0000-000000000000/view",
      "/acme/projects/00000000-0000-0000-0000-000000000000/views-archive",
      "/acme/workspace-view",
      "/acme/workspace-views-archive",
    ]) {
      expect(isMigratedPath(path), path).toBe(false);
    }
    expect(resolveLinkKind("/acme/workspace-views")).toBe("router");
    expect(resolveLinkKind("/acme/workspace-view")).toBe("anchor");
  });

  it("keeps workspace matching out of reserved top-level names and the bare root", () => {
    for (const name of [
      "api",
      "god-mode",
      "onboarding",
      "profile",
      "settings",
      "spaces",
      "sign-in",
      "auth",
      "static",
      "live",
      "ws",
    ]) {
      expect(MIGRATED_RESERVED_TOP_LEVEL).toContain(name);
      expect(isMigratedPath(`/${name}`, ["/{ws}"])).toBe(false);
      expect(isMigratedPath(`/${name}/x`, ["/{ws}/*"])).toBe(false);
    }
    expect(MIGRATED_RESERVED_TOP_LEVEL).not.toContain("acme");
    expect(isMigratedPath("/")).toBe(false);
  });

  it("still serves the auth literals themselves from web_new", () => {
    for (const path of ["/sign-in", "/sign-up", "/onboarding", "/accounts/reset-password"]) {
      expect(isMigratedPath(path)).toBe(true);
    }
    expect(isMigratedPath("/api")).toBe(false);
    // /god-mode is a migrated literal since the admin area gate (NEWFRONT-521).
    expect(isMigratedPath("/god-mode")).toBe(true);
    // /profile is a migrated literal since the Profile area gate (NEWFRONT-586).
    expect(isMigratedPath("/profile/me")).toBe(true);
    expect(isMigratedPath("/settings/acme")).toBe(false);
    expect(isMigratedPath("/spaces/acme")).toBe(false);
  });
});

/**
 * The Caddy generator and this matcher are two implementations of one rule
 * set. Run both over the same paths and require the same verdict, using the
 * regex the generator would actually emit.
 */
describe("parity with the Caddy generator", () => {
  const paths = [
    "/sign-in",
    "/sign-in/",
    "/sign-in/deep",
    "/accounts/reset-password",
    "/api",
    "/API",
    "/api/x",
    "/god-mode",
    "/god-mode/users/1",
    "/onboarding",
    "/profile/me",
    "/settings/acme/projects",
    "/spaces",
    "/spaces/projects",
    "/assets/app.js",
    "/live",
    "/ws/runner",
    "/static/x",
    "/",
    "/acme",
    "/acme/",
    "/ACME",
    "/acme/projects",
    "/acme/projects/",
    "/acme/projects/1/issues",
    "/acme/browse/ACME-1",
    "/acme/browse/ACME-1/",
    "/acme/browse/ACME-1/x",
    "/acme/browse",
    // Workspace settings (WSET area gate). The six sections and the whole
    // webhook detail subtree move; project settings moved with their own
    // gate (NEWFRONT-606); the profile-owned legacy forwards stay.
    "/acme/settings",
    "/acme/settings/",
    "/acme/settings/members",
    "/acme/settings/billing",
    "/acme/settings/exports",
    "/acme/settings/integrations",
    "/acme/settings/webhooks",
    "/acme/settings/webhooks/1",
    "/acme/settings/webhooks/1/x",
    "/acme/settings/projects/1",
    "/acme/settings/projects",
    "/acme/settings/projects/1/labels",
    "/acme/settings/api-tokens",
    "/acme/settings/account",
    "/acme/settings/unknown",
    // Runner area (RUN gate): the workspace runner shell and its subtree,
    // AI dev machines, and the per-project runner subtree.
    "/acme/runners",
    "/acme/runners/",
    "/acme/runners/approvals",
    "/acme/runners/runs/1",
    "/acme/runners/chat/1",
    "/acme/runner",
    "/acme/runs",
    "/acme/ai-dev-machines",
    "/acme/ai-dev-machines/1",
    "/acme/ai-dev-machine",
    "/acme/projects/1/runners",
    "/acme/projects/1/runners/runs",
    "/acme/projects/1/runners/",
    "/acme/projects/1/runners/runs/1",
    "/acme/projects/1/issues",
    "/acme.dev",
    "/acme.dev/projects",
    "/foo",
    "/foo/bar",
    "/acme/Projects",
  ];

  it("agrees on every path in the shared matrix", () => {
    const { entries, reservedTopLevel } = loadEntries() as {
      entries: string[];
      reservedTopLevel: string[];
    };
    const reserved = reservedTopLevel.map((name) => name.toLowerCase());
    // What each generated handler matches: literal entries keep their
    // exact-or-subtree redirect/proxy pair; workspace entries are the
    // named matcher (regex plus the reserved exclusion).
    const compiled: GeneratedHandler[] = entries.map((entry) => {
      const parsed = parseEntry(entry) as GeneratedEntry;
      if (parsed.catchAll) return { kind: "catchAll" as const };
      if (parsed.segments[0]?.kind !== "variable") {
        const base = entry.replace(/\/+$/, "");
        return { kind: "literal" as const, base };
      }
      return { kind: "matcher" as const, regex: new RegExp(entryRegex(parsed)) };
    });
    for (const path of paths) {
      const bare = path === "/" ? "/" : path.replace(/\/+$/, "");
      const first = path
        .split("/")
        .filter((segment) => segment !== "")[0]
        ?.toLowerCase();
      const viaCaddy = compiled.some((route) => {
        if (route.kind === "catchAll") return true;
        if (route.kind === "literal") return bare === route.base || bare.startsWith(`${route.base}/`);
        if (!route.regex.test(path)) return false;
        return !(first !== undefined && reserved.includes(first));
      });
      expect(isMigratedPath(path), path).toBe(viaCaddy);
    }
  });

  it("agrees on explicit fixtures shared with the generator's tests", () => {
    const entries = ["/sign-in", "/{ws}", "/{ws}/projects", "/{ws}/projects/*"];
    const reserved = ["api", "god-mode", "settings", "spaces"];
    for (const [path, expected] of [
      ["/acme", true],
      ["/acme/projects", true],
      ["/acme/projects/1/issues", true],
      ["/acme/active-cycles", false],
      ["/api/projects", false],
      ["/settings/acme/projects", false],
      ["/sign-in", true],
      ["/sign-in/x", true],
      ["/", false],
    ] as const) {
      expect(isMigratedPath(path, entries, reserved), path).toBe(expected);
    }
  });
});

describe("compileEntry", () => {
  it("marks literal entries as subtree and workspace entries as exact", () => {
    expect(compileEntry("/sign-in")).toMatchObject({ subtree: true, workspaceScoped: false });
    expect(compileEntry("/{ws}")).toMatchObject({ subtree: false, workspaceScoped: true });
    expect(compileEntry("/{ws}/x/*")).toMatchObject({ subtree: true, workspaceScoped: true });
    expect(compileEntry("/")).toMatchObject({ catchAll: true, subtree: false });
  });

  it("rejects malformed syntax", () => {
    expect(() => compileEntry("sign-in")).toThrow(/slash/);
    expect(() => compileEntry("/{ws}/*/x")).toThrow(/last segment/);
    expect(() => compileEntry("/{ws}/{bad name}")).toThrow(/bad segment/);
    expect(() => compileEntry("/{ws/x}")).toThrow(/bad segment/);
  });
});

describe("resolveLinkKind", () => {
  it("sends migrated paths to the router and everything else to a plain anchor", () => {
    const entries = ["/sign-in", "/{ws}/projects"];
    expect(resolveLinkKind("/sign-in", entries)).toBe("router");
    expect(resolveLinkKind("/sign-in/", entries)).toBe("router");
    expect(resolveLinkKind("/acme/projects", entries)).toBe("router");
    expect(resolveLinkKind("/acme/projects/1/issues", entries)).toBe("anchor");
    expect(resolveLinkKind("/acme", entries)).toBe("anchor");
    expect(resolveLinkKind("/projects/1/issues")).toBe("anchor");
  });

  it("flips a workspace link once its route is listed", () => {
    expect(resolveLinkKind("/acme/projects")).toBe("router");
    expect(resolveLinkKind("/acme/active-cycles")).toBe("anchor");
  });
});
