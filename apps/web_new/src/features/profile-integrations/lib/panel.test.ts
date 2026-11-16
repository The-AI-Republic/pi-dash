// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Panel model (parity/specs/profile.md §11, PROF-025): which workspace is
// shown, how the installation's flags become the badge/banners, and the
// identifier grid a connected panel renders.
import type { GithubAppInstallation, GithubAppStatus } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  formatStamp,
  installDisabled,
  installLabel,
  isConnected,
  isSuspended,
  lastCheckError,
  refreshVisible,
  selectedWorkspace,
  statusFields,
} from "./panel.js";

const ID_ONE = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";
const ID_TWO = "8a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

function installation(overrides: Partial<GithubAppInstallation> = {}): GithubAppInstallation {
  return { connected: true, ...overrides };
}

function status(overrides: Partial<GithubAppStatus> = {}): GithubAppStatus {
  return {
    configured: true,
    app_slug: "pi-dash",
    workspaces: [
      { id: ID_ONE, slug: "acme", name: "Acme", github_app: installation() },
      { id: ID_TWO, slug: "beta", name: "Beta", github_app: { connected: false } },
    ],
    ...overrides,
  };
}

describe("selectedWorkspace", () => {
  it("returns nothing before the read", () => {
    expect(selectedWorkspace(undefined, null)).toBeUndefined();
  });

  it("prefers the picked workspace", () => {
    expect(selectedWorkspace(status(), "beta")?.name).toBe("Beta");
  });

  it("falls back to the first workspace, including for an unknown pick", () => {
    expect(selectedWorkspace(status(), null)?.slug).toBe("acme");
    expect(selectedWorkspace(status(), "ghost")?.slug).toBe("acme");
  });

  it("resolves nothing for a member with no admin workspace, which is the explanatory view", () => {
    expect(selectedWorkspace(status({ workspaces: [] }), null)).toBeUndefined();
    expect(selectedWorkspace(status({ workspaces: [] }), "acme")).toBeUndefined();
  });
});

describe("installation flags", () => {
  it("counts only the server's connected flag", () => {
    expect(isConnected(installation())).toBe(true);
    expect(isConnected({ connected: false })).toBe(false);
    expect(isConnected(undefined)).toBe(false);
  });

  it("reads a suspended stamp, ignoring absent and blank", () => {
    expect(isSuspended(installation({ suspended_at: "2026-10-19T00:00:00Z" }))).toBe(true);
    expect(isSuspended(installation({ suspended_at: null }))).toBe(false);
    expect(isSuspended(installation({ suspended_at: "" }))).toBe(false);
    expect(isSuspended(undefined)).toBe(false);
  });

  it("keeps install inert until the instance is configured", () => {
    expect(installDisabled(undefined)).toBe(true);
    expect(installDisabled(false)).toBe(true);
    expect(installDisabled(true)).toBe(false);
  });

  it("offers reconnect once connected", () => {
    expect(installLabel(true)).toBe("Reconnect");
    expect(installLabel(false)).toBe("Install");
  });

  it("shows refresh only for a connected installation", () => {
    expect(refreshVisible(installation())).toBe(true);
    expect(refreshVisible({ connected: false })).toBe(false);
    expect(refreshVisible(undefined)).toBe(false);
  });
});

describe("formatStamp", () => {
  it("says Never for a missing or unreadable stamp", () => {
    expect(formatStamp(undefined)).toBe("Never");
    expect(formatStamp(null)).toBe("Never");
    expect(formatStamp("")).toBe("Never");
    expect(formatStamp("not-a-date")).toBe("Never");
  });

  it("formats a real stamp", () => {
    expect(formatStamp("2026-10-19T12:00:00Z")).toBe(new Date("2026-10-19T12:00:00Z").toLocaleString());
  });
});

describe("statusFields", () => {
  it("renders nothing for a disconnected installation", () => {
    expect(statusFields({ connected: false })).toEqual([]);
    expect(statusFields(undefined)).toEqual([]);
  });

  it("lists the six identifier rows in display order", () => {
    const fields = statusFields(
      installation({
        account_login: "octo",
        account_type: "Organization",
        repository_selection: "all",
        repository_count: 12,
        verified_at: "2026-10-19T12:00:00Z",
        last_checked_at: null,
      })
    );
    expect(fields.map((field) => field.label)).toEqual([
      "Account",
      "Account type",
      "Repository access",
      "Repositories visible",
      "Verified",
      "Last checked",
    ]);
    expect(fields[0]?.value).toBe("octo");
    expect(fields[2]?.value).toBe("All repositories");
    expect(fields[3]?.value).toBe("12");
    expect(fields[5]?.value).toBe("Never");
  });

  it("names a selected-repository install", () => {
    expect(statusFields(installation({ repository_selection: "selected" }))[2]?.value).toBe("Selected repositories");
  });

  it("fills unknown identifiers and a missing count", () => {
    const fields = statusFields(installation());
    expect(fields[0]?.value).toBe("Unknown");
    expect(fields[1]?.value).toBe("Unknown");
    expect(fields[3]?.value).toBe("0");
  });
});

describe("lastCheckError", () => {
  it("returns the host's detail only when the last check failed", () => {
    expect(lastCheckError(installation({ last_check_error: "403 Forbidden" }))).toBe("403 Forbidden");
    expect(lastCheckError(installation({ last_check_error: "" }))).toBeNull();
    expect(lastCheckError(installation())).toBeNull();
    expect(lastCheckError(undefined)).toBeNull();
  });
});
