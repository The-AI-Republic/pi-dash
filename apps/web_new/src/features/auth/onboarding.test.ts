// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  advanceOnboardingStep,
  avatarFileError,
  AVATAR_MAX_BYTES,
  AVATAR_MIME_TYPES,
  CLI_INSTALL_TABS,
  combineUseCases,
  copyInstallCommand,
  decideInitialOnboardingStep,
  detectCliInstallTab,
  displayNameForAccount,
  finishProfileStamp,
  isOnboardingFlagSet,
  mergeOnboardingFlags,
  ONBOARDING_ROLES,
  ONBOARDING_USE_CASES,
  onboardingProgress,
  parseUseCases,
  previousOnboardingStep,
  shouldShowInviteStep,
  USE_CASE_SEPARATOR,
  validateProfileName,
  visibleOnboardingSteps,
} from "./onboarding.js";

describe("onboarding progress flags", () => {
  it("reads stored flags strictly", () => {
    expect(isOnboardingFlagSet({ profile_complete: true }, "profile_complete")).toBe(true);
    expect(isOnboardingFlagSet({ profile_complete: false }, "profile_complete")).toBe(false);
    expect(isOnboardingFlagSet({ profile_complete: 1 }, "profile_complete")).toBe(false);
    expect(isOnboardingFlagSet({}, "profile_complete")).toBe(false);
    expect(isOnboardingFlagSet(undefined, "profile_complete")).toBe(false);
  });

  it("merges a stamp over the stored flags", () => {
    expect(mergeOnboardingFlags({ profile_complete: true }, { workspace_create: true })).toEqual({
      profile_complete: true,
      workspace_create: true,
    });
    expect(mergeOnboardingFlags(undefined, { profile_complete: true })).toEqual({ profile_complete: true });
  });
});

describe("onboarding vocabularies", () => {
  it("lists seven fixed roles", () => {
    expect(ONBOARDING_ROLES).toHaveLength(7);
    expect(new Set(ONBOARDING_ROLES.map((role) => role.id)).size).toBe(7);
    expect(ONBOARDING_ROLES.map((role) => role.id)).toContain("developer");
  });

  it("combines and parses use-case selections", () => {
    expect(ONBOARDING_USE_CASES).toHaveLength(5);
    const combined = combineUseCases([ONBOARDING_USE_CASES[0] ?? "", ONBOARDING_USE_CASES[1] ?? ""]);
    expect(combined).toBe(`${ONBOARDING_USE_CASES[0]}${USE_CASE_SEPARATOR}${ONBOARDING_USE_CASES[1]}`);
    expect(parseUseCases(combined)).toEqual([ONBOARDING_USE_CASES[0], ONBOARDING_USE_CASES[1]]);
  });

  it("parses absent or empty stored values to no choices", () => {
    expect(parseUseCases(null)).toEqual([]);
    expect(parseUseCases(undefined)).toEqual([]);
    expect(parseUseCases("")).toEqual([]);
    expect(parseUseCases(`Choice${USE_CASE_SEPARATOR}`)).toEqual(["Choice"]);
  });
});

describe("CLI install content", () => {
  it("ships one Unix command and two Windows commands", () => {
    expect(CLI_INSTALL_TABS.unix.label).toBe("macOS / Linux");
    expect(CLI_INSTALL_TABS.unix.commands).toHaveLength(1);
    expect(CLI_INSTALL_TABS.windows.label).toBe("Windows");
    expect(CLI_INSTALL_TABS.windows.commands).toHaveLength(2);
    expect(CLI_INSTALL_TABS.windows.commands[1]?.hint).toBeDefined();
  });

  it("preselects the tab matching the browser OS", () => {
    expect(detectCliInstallTab("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows");
    expect(detectCliInstallTab("windows")).toBe("windows");
    expect(detectCliInstallTab("Mozilla/5.0 (Macintosh; Intel Mac OS X)")).toBe("unix");
    expect(detectCliInstallTab("Mozilla/5.0 (X11; Linux x86_64)")).toBe("unix");
    expect(detectCliInstallTab("")).toBe("unix");
  });
});

describe("copyInstallCommand", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("copies through the async clipboard when available", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await copyInstallCommand("curl example");
    expect(writeText).toHaveBeenCalledWith("curl example");
  });

  it("falls back to the textarea path when the clipboard rejects", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const execCommand = vi.fn().mockReturnValue(true);
    (document as unknown as Record<string, unknown>)["execCommand"] = execCommand;
    await copyInstallCommand("curl example");
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("falls back when no clipboard exists", async () => {
    vi.stubGlobal("navigator", {});
    const execCommand = vi.fn().mockReturnValue(true);
    (document as unknown as Record<string, unknown>)["execCommand"] = execCommand;
    await copyInstallCommand("curl example");
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("rejects when neither path copies", async () => {
    vi.stubGlobal("navigator", {});
    const execCommand = vi.fn().mockReturnValue(false);
    (document as unknown as Record<string, unknown>)["execCommand"] = execCommand;
    await expect(copyInstallCommand("curl example")).rejects.toThrow();
    expect(document.querySelector("textarea")).toBeNull();
  });
});

describe("validateProfileName", () => {
  it("requires a name", () => {
    expect(validateProfileName("")).not.toBeNull();
    expect(validateProfileName("   ")).not.toBeNull();
  });

  it("length-checks at fifty characters", () => {
    expect(validateProfileName("A".repeat(50))).toBeNull();
    expect(validateProfileName("A".repeat(51))).not.toBeNull();
  });

  it("rejects injection-risk characters", () => {
    for (const char of ["<", ">", '"', "{", "}", "[", "]", "*", "^", "!", "#", "%"]) {
      expect(validateProfileName(`Ada${char}`)).not.toBeNull();
    }
  });

  it("rejects characters outside the person-name charset", () => {
    expect(validateProfileName("Ada123")).not.toBeNull();
    expect(validateProfileName("Ada_Byron")).not.toBeNull();
    expect(validateProfileName("Ada.Byron")).not.toBeNull();
  });

  it("accepts letters, spaces, hyphens, and apostrophes", () => {
    expect(validateProfileName("Ada")).toBeNull();
    expect(validateProfileName("Ada Byron")).toBeNull();
    expect(validateProfileName("Jean-Paul")).toBeNull();
    expect(validateProfileName("O'Brien")).toBeNull();
    expect(validateProfileName("José Müller")).toBeNull();
  });
});

describe("avatarFileError", () => {
  function file(type: string, size: number): File {
    const bytes = new Uint8Array(size);
    return new File([bytes], "avatar.png", { type });
  }

  it("accepts the image types within the size limit", () => {
    for (const type of AVATAR_MIME_TYPES) {
      expect(avatarFileError(file(type, 1024))).toBeNull();
    }
    expect(avatarFileError(file("image/png", AVATAR_MAX_BYTES))).toBeNull();
  });

  it("rejects other types and oversize files", () => {
    expect(avatarFileError(file("image/gif", 1024))).not.toBeNull();
    expect(avatarFileError(file("application/pdf", 1024))).not.toBeNull();
    expect(avatarFileError(file("", 1024))).not.toBeNull();
    expect(avatarFileError(file("image/png", AVATAR_MAX_BYTES + 1))).not.toBeNull();
  });
});

describe("visibleOnboardingSteps", () => {
  it("shows the full sequence on browser builds off self-managed", () => {
    expect(visibleOnboardingSteps({ showCli: true, selfManaged: false, showInvite: true })).toEqual([
      "cli-install",
      "profile",
      "role",
      "use-case",
      "workspace",
      "invite",
    ]);
  });

  it("omits the CLI step where it does not apply", () => {
    expect(visibleOnboardingSteps({ showCli: false, selfManaged: false, showInvite: true })[0]).toBe("profile");
  });

  it("skips role and use-case on self-managed instances", () => {
    expect(visibleOnboardingSteps({ showCli: true, selfManaged: true, showInvite: true })).toEqual([
      "cli-install",
      "profile",
      "workspace",
      "invite",
    ]);
  });

  it("hides the invite step while invitations exist and it is not current", () => {
    const steps = visibleOnboardingSteps({ showCli: true, selfManaged: false, showInvite: false });
    expect(steps).not.toContain("invite");
    expect(steps).toHaveLength(5);
  });

  it("combines desktop with self-managed to three steps", () => {
    expect(visibleOnboardingSteps({ showCli: false, selfManaged: true, showInvite: false })).toEqual([
      "profile",
      "workspace",
    ]);
  });
});

describe("shouldShowInviteStep", () => {
  it("counts the invite step without invitations or while on it", () => {
    expect(shouldShowInviteStep({ hasInvitations: false, current: "profile" })).toBe(true);
    expect(shouldShowInviteStep({ hasInvitations: true, current: "invite" })).toBe(true);
    expect(shouldShowInviteStep({ hasInvitations: true, current: "workspace" })).toBe(false);
  });
});

describe("decideInitialOnboardingStep", () => {
  it("resumes at the workspace step once the profile is stored", () => {
    expect(decideInitialOnboardingStep({ showCli: true, flags: { profile_complete: true } })).toBe("workspace");
    expect(
      decideInitialOnboardingStep({
        showCli: false,
        flags: { profile_complete: true, workspace_create: false, workspace_join: false },
      })
    ).toBe("workspace");
  });

  it("resumes at the invite step once a workspace is created", () => {
    expect(
      decideInitialOnboardingStep({
        showCli: true,
        flags: { profile_complete: true, workspace_create: true, workspace_invite: false },
      })
    ).toBe("invite");
  });

  it("restarts anything earlier at the first visible step", () => {
    expect(decideInitialOnboardingStep({ showCli: true, flags: undefined })).toBe("cli-install");
    expect(decideInitialOnboardingStep({ showCli: true, flags: {} })).toBe("cli-install");
    expect(decideInitialOnboardingStep({ showCli: false, flags: undefined })).toBe("profile");
    expect(
      decideInitialOnboardingStep({
        showCli: true,
        flags: { profile_complete: true, workspace_create: true, workspace_invite: true },
      })
    ).toBe("cli-install");
    expect(
      decideInitialOnboardingStep({ showCli: true, flags: { profile_complete: true, workspace_join: true } })
    ).toBe("cli-install");
  });
});

describe("advanceOnboardingStep", () => {
  it("leaves the CLI step for profile setup storing nothing", () => {
    expect(advanceOnboardingStep({ from: "cli-install", selfManaged: false, hasWorkspaces: false })).toEqual({
      to: "profile",
      stamp: null,
    });
  });

  it("leaves profile setup for role setup off self-managed", () => {
    expect(advanceOnboardingStep({ from: "profile", selfManaged: false, hasWorkspaces: false })).toEqual({
      to: "role",
      stamp: null,
    });
  });

  it("leaves profile setup for the workspace step on self-managed, stamped", () => {
    expect(advanceOnboardingStep({ from: "profile", selfManaged: true, hasWorkspaces: false })).toEqual({
      to: "workspace",
      stamp: { profile_complete: true },
    });
  });

  it("finishes at once for self-managed users who already belong somewhere", () => {
    expect(advanceOnboardingStep({ from: "profile", selfManaged: true, hasWorkspaces: true })).toEqual({
      to: "finish",
      stamp: { profile_complete: true },
    });
  });

  it("leaves role setup for use-case setup storing nothing", () => {
    expect(advanceOnboardingStep({ from: "role", selfManaged: false, hasWorkspaces: false })).toEqual({
      to: "use-case",
      stamp: null,
    });
  });

  it("leaves use-case setup stamped, finishing for members", () => {
    expect(advanceOnboardingStep({ from: "use-case", selfManaged: false, hasWorkspaces: false })).toEqual({
      to: "workspace",
      stamp: { profile_complete: true },
    });
    expect(advanceOnboardingStep({ from: "use-case", selfManaged: false, hasWorkspaces: true })).toEqual({
      to: "finish",
      stamp: { profile_complete: true },
    });
  });

  it("leaves the workspace step for invites, stamped", () => {
    expect(advanceOnboardingStep({ from: "workspace", selfManaged: false, hasWorkspaces: false })).toEqual({
      to: "invite",
      stamp: { workspace_create: true },
    });
  });

  it("finishes from the workspace step on the shortcuts without stamping", () => {
    expect(
      advanceOnboardingStep({ from: "workspace", selfManaged: false, hasWorkspaces: true, skipInvites: true })
    ).toEqual({ to: "finish", stamp: null });
  });

  it("leaves the invite step for the finish, stamped", () => {
    expect(advanceOnboardingStep({ from: "invite", selfManaged: false, hasWorkspaces: true })).toEqual({
      to: "finish",
      stamp: { workspace_invite: true },
    });
  });
});

describe("previousOnboardingStep", () => {
  it("returns profile to the CLI step on browser builds only", () => {
    expect(previousOnboardingStep({ from: "profile", showCli: true, selfManaged: false })).toBe("cli-install");
    expect(previousOnboardingStep({ from: "profile", showCli: false, selfManaged: false })).toBeNull();
  });

  it("walks role and use-case back one step each", () => {
    expect(previousOnboardingStep({ from: "role", showCli: true, selfManaged: false })).toBe("profile");
    expect(previousOnboardingStep({ from: "use-case", showCli: true, selfManaged: false })).toBe("role");
  });

  it("returns the workspace step past the skipped steps on self-managed", () => {
    expect(previousOnboardingStep({ from: "workspace", showCli: true, selfManaged: true })).toBe("profile");
    expect(previousOnboardingStep({ from: "workspace", showCli: true, selfManaged: false })).toBe("use-case");
  });

  it("offers no back on the first and last steps", () => {
    expect(previousOnboardingStep({ from: "cli-install", showCli: true, selfManaged: false })).toBeNull();
    expect(previousOnboardingStep({ from: "invite", showCli: true, selfManaged: false })).toBeNull();
  });
});

describe("onboardingProgress", () => {
  it("counts the current step over the visible total", () => {
    expect(onboardingProgress({ steps: ["cli-install", "profile", "workspace"], current: "cli-install" })).toEqual({
      current: 1,
      total: 3,
    });
    expect(onboardingProgress({ steps: ["cli-install", "profile", "workspace"], current: "profile" })).toEqual({
      current: 2,
      total: 3,
    });
    expect(onboardingProgress({ steps: ["profile", "workspace"], current: "workspace" })).toEqual({
      current: 2,
      total: 2,
    });
  });
});

describe("displayNameForAccount", () => {
  it("prefers the display name, then first-plus-last, then the address", () => {
    expect(
      displayNameForAccount({ display_name: "Ada B", first_name: "Ada", last_name: "Byron", email: "a@x.test" })
    ).toBe("Ada B");
    expect(displayNameForAccount({ display_name: "", first_name: "Ada", last_name: "Byron", email: "a@x.test" })).toBe(
      "Ada Byron"
    );
    expect(displayNameForAccount({ display_name: "", first_name: "Ada", last_name: "", email: "a@x.test" })).toBe(
      "Ada"
    );
    expect(displayNameForAccount({ display_name: "", first_name: "", last_name: "", email: "a@x.test" })).toBe(
      "a@x.test"
    );
  });
});

describe("finishProfileStamp", () => {
  it("stamps every flag plus the first workspace", () => {
    const patch = finishProfileStamp([
      { id: "w1", name: "Acme", slug: "acme", logo_url: "", total_members: 1, role: 20 },
    ]);
    expect(patch.onboarding_step).toEqual({
      profile_complete: true,
      workspace_create: true,
      workspace_invite: true,
      workspace_join: true,
    });
    expect(patch.last_workspace_id).toBe("w1");
  });

  it("leaves the workspace pointer out when the user belongs nowhere", () => {
    expect(finishProfileStamp([]).last_workspace_id).toBeUndefined();
    expect(finishProfileStamp(undefined).last_workspace_id).toBeUndefined();
  });
});
