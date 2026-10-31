// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Onboarding flow logic (AUTH-029–033, AUTH-043). Pure step math for the
// single-URL flow: the visible sequence, resume, forward/back transitions
// with their progress stamps, the header progress, plus the fixed role and
// use-case vocabularies, the CLI install commands, and the profile-step
// validators. The workspace and invite steps are described here so the
// sequence stays whole; S8 (NEWFRONT-489) mounts their rendering inside
// the shell this slice owns. Written from parity/specs/auth.md.

import type { ProfilePatch, WorkspaceList } from "@pidash/api-client";

/** Every step of the flow, in canonical order. */
export type OnboardingStep = "cli-install" | "profile" | "role" | "use-case" | "workspace" | "invite";

export const ONBOARDING_STEPS: readonly OnboardingStep[] = [
  "cli-install",
  "profile",
  "role",
  "use-case",
  "workspace",
  "invite",
];

/** Progress flags the server stores on the profile's onboarding_step bag. */
export type OnboardingStamp = Partial<
  Record<"profile_complete" | "workspace_create" | "workspace_invite" | "workspace_join", boolean>
>;

/** True when the stored profile flags carry the named flag. */
export function isOnboardingFlagSet(flags: Record<string, unknown> | undefined, name: string): boolean {
  return flags?.[name] === true;
}

/** Stored flags with a transition stamp merged over them. */
export function mergeOnboardingFlags(
  flags: Record<string, unknown> | undefined,
  stamp: OnboardingStamp
): Record<string, unknown> {
  return { ...(flags ?? {}), ...stamp };
}

// --- fixed vocabularies ------------------------------------------------------
// Role ids and use-case strings are server-stored values (PATCH
// /api/users/me/profile/ with `role` / `use_case`), so they stay stable:
// the ids are the API vocabulary, the labels the fixed list the flow shows.

/** The seven fixed roles (AUTH-032). */
export const ONBOARDING_ROLES: readonly { id: string; label: string }[] = [
  { id: "product-manager", label: "Product Manager" },
  { id: "engineering-manager", label: "Engineering Manager" },
  { id: "designer", label: "Designer" },
  { id: "developer", label: "Developer" },
  { id: "founder-executive", label: "Founder/Executive" },
  { id: "operations-manager", label: "Operations Manager" },
  { id: "others", label: "Others" },
];

/** The five shared use cases (AUTH-033), stored combined with USE_CASE_SEPARATOR. */
export const ONBOARDING_USE_CASES: readonly string[] = [
  "Plan and track product roadmaps",
  "Manage engineering sprints",
  "Coordinate cross-functional projects",
  "Replace our current tool",
  "Just exploring",
];

/** Separator joining the chosen use cases into one stored profile value. */
export const USE_CASE_SEPARATOR = ". ";

/** The chosen use cases as one stored profile value. */
export function combineUseCases(selection: readonly string[]): string {
  return selection.join(USE_CASE_SEPARATOR);
}

/** A stored combined value back into its choices (for prefill). */
export function parseUseCases(stored: string | null | undefined): string[] {
  if (!stored) return [];
  return stored.split(USE_CASE_SEPARATOR).filter((choice) => choice.length > 0);
}

// --- CLI install content (AUTH-030) ------------------------------------------
// The commands are the install entry points users paste; the release base is
// the download origin they point at.

/** Download origin for the install wrappers (binary plus `pidash auth login`). */
export const CLI_INSTALL_RELEASE_BASE = "https://github.com/The-AI-Republic/pi-dash/releases/latest/download";

export type CliInstallTab = "unix" | "windows";

export interface CliInstallCommand {
  shell: string;
  command: string;
  /** Rendered above the block when a platform ships more than one shell. */
  hint?: string | undefined;
}

export const CLI_INSTALL_TABS: Record<CliInstallTab, { label: string; commands: CliInstallCommand[] }> = {
  unix: {
    label: "macOS / Linux",
    commands: [
      {
        shell: "Terminal",
        command: `curl --proto '=https' --tlsv1.2 -LsSf ${CLI_INSTALL_RELEASE_BASE}/install.sh | sh`,
      },
    ],
  },
  windows: {
    label: "Windows",
    commands: [
      {
        shell: "PowerShell",
        command: `irm ${CLI_INSTALL_RELEASE_BASE}/install.ps1 | iex`,
      },
      {
        shell: "Command Prompt",
        hint: "Prefer Command Prompt?",
        command: `powershell -c "irm ${CLI_INSTALL_RELEASE_BASE}/install.ps1 | iex"`,
      },
    ],
  },
};

/** The tab matching the browser OS: Windows stays on Windows, everything else on Unix. */
export function detectCliInstallTab(userAgent: string): CliInstallTab {
  return userAgent.toLowerCase().includes("windows") ? "windows" : "unix";
}

/**
 * Copy a command to the clipboard: the async clipboard first, then a
 * detached-textarea fallback for origins without clipboard access. Rejects
 * when neither path works, so the caller can explain manual copying.
 */
export async function copyInstallCommand(command: string): Promise<void> {
  const clipboard = navigator.clipboard;
  if (clipboard?.writeText) {
    try {
      await clipboard.writeText(command);
      return;
    } catch {
      // Fall through to the textarea path: writeText can be present and
      // still reject (unfocused document, embedded-context denial).
    }
  }
  const area = document.createElement("textarea");
  area.value = command;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  try {
    const copied = document.execCommand("copy");
    if (!copied) throw new Error("copy unavailable");
  } finally {
    area.remove();
  }
}

// --- profile validators (AUTH-031) -------------------------------------------

/** Display names hold at most fifty characters. */
export const PROFILE_NAME_MAX_LENGTH = 50;

/** Avatars hold at most five megabytes. */
export const AVATAR_MAX_BYTES = 5 * 1024 * 1024;

/** Image types the avatar picker accepts. */
export const AVATAR_MIME_TYPES: readonly string[] = ["image/jpeg", "image/jpg", "image/png", "image/webp"];

/** Characters that can never appear in a display name (injection-risk set). */
const NAME_FORBIDDEN_CHARS = /[<>"{}[\]*^!#%]/;

/** Person-name charset: unicode letters, whitespace, hyphens, apostrophes. */
const PERSON_NAME_CHARS = /^[\p{L}\s'-]+$/u;

/**
 * Display-name check (AUTH-031): required, length-checked, no
 * injection-risk characters, person-name charset. Null when valid.
 * Apostrophes stay allowed per the spec (person names like O'Brien);
 * the forbidden set is the injection-risk remainder.
 */
export function validateProfileName(name: string): string | null {
  if (name.trim() === "") return "Enter your name to continue.";
  if (name.length > PROFILE_NAME_MAX_LENGTH) return `Keep your name within ${PROFILE_NAME_MAX_LENGTH} characters.`;
  if (NAME_FORBIDDEN_CHARS.test(name)) return 'Names cannot contain symbols like < > " { } [ ] * ^ ! # %.';
  if (!PERSON_NAME_CHARS.test(name)) return "Use only letters, spaces, hyphens, and apostrophes.";
  return null;
}

/** Avatar-file check: an accepted image within the size limit. Null when valid. */
export function avatarFileError(file: File): string | null {
  if (!AVATAR_MIME_TYPES.includes(file.type)) return "Choose a JPEG, PNG, or WebP image.";
  if (file.size > AVATAR_MAX_BYTES) return "Choose an image under 5 MB.";
  return null;
}

// --- visible sequence, resume, transitions (AUTH-029, AUTH-043) --------------

/**
 * The visible sequence (AUTH-029 plus the desktop target): the CLI step
 * only on browser builds, role and use-case only off self-managed, the
 * invite step only while the user has no invitations or is already on it.
 */
export function visibleOnboardingSteps(args: {
  showCli: boolean;
  selfManaged: boolean;
  showInvite: boolean;
}): OnboardingStep[] {
  return ONBOARDING_STEPS.filter((step) => {
    if (step === "cli-install") return args.showCli;
    if (step === "role" || step === "use-case") return !args.selfManaged;
    if (step === "invite") return args.showInvite;
    return true;
  });
}

/** The invite step counts while the user has no invitations, or is already on it. */
export function shouldShowInviteStep(args: { hasInvitations: boolean; current: OnboardingStep }): boolean {
  return !args.hasInvitations || args.current === "invite";
}

/**
 * Where a refresh resumes (AUTH-029): stored profile progress without a
 * created or joined workspace reopens the workspace step; a created
 * workspace without invited members reopens the invite step. Anything
 * earlier restarts at the first visible step (CLI install on browser,
 * profile setup on desktop).
 */
export function decideInitialOnboardingStep(args: {
  showCli: boolean;
  flags: Record<string, unknown> | undefined;
}): OnboardingStep {
  const profileComplete = isOnboardingFlagSet(args.flags, "profile_complete");
  const created = isOnboardingFlagSet(args.flags, "workspace_create");
  const joined = isOnboardingFlagSet(args.flags, "workspace_join");
  const invited = isOnboardingFlagSet(args.flags, "workspace_invite");
  if (profileComplete && !created && !joined) return "workspace";
  if (profileComplete && created && !invited) return "invite";
  return args.showCli ? "cli-install" : "profile";
}

export type OnboardingAdvance = { to: OnboardingStep | "finish"; stamp: OnboardingStamp | null };

/**
 * One step forward (AUTH-029): the next step plus the progress stamp the
 * transition stores (null when it stores nothing). Leaving profile setup
 * on self-managed instances, or use-case setup anywhere, stamps
 * profile-complete and finishes at once for users who already belong to a
 * workspace. The workspace and invite edges belong to S8's steps; they are
 * described here so the sequence stays whole.
 */
export function advanceOnboardingStep(args: {
  from: OnboardingStep;
  selfManaged: boolean;
  hasWorkspaces: boolean;
  /** The solo-size and already-member shortcuts finish without the invite step. */
  skipInvites?: boolean | undefined;
}): OnboardingAdvance {
  switch (args.from) {
    case "cli-install":
      return { to: "profile", stamp: null };
    case "profile":
      if (!args.selfManaged) return { to: "role", stamp: null };
      return {
        to: args.hasWorkspaces ? "finish" : "workspace",
        stamp: { profile_complete: true },
      };
    case "role":
      return { to: "use-case", stamp: null };
    case "use-case":
      return {
        to: args.hasWorkspaces ? "finish" : "workspace",
        stamp: { profile_complete: true },
      };
    case "workspace":
      if (args.skipInvites) return { to: "finish", stamp: null };
      return { to: "invite", stamp: { workspace_create: true } };
    case "invite":
      return { to: "finish", stamp: { workspace_invite: true } };
  }
}

/**
 * One step back (AUTH-043, desktop target per main#494): profile returns to
 * the CLI step on browser builds and nowhere on desktop (it is first
 * there); role to profile; use-case to role; workspace to profile on
 * self-managed and to use-case otherwise. Null means no back affordance
 * (the first and last steps); the browser URL never changes with the step.
 */
export function previousOnboardingStep(args: {
  from: OnboardingStep;
  showCli: boolean;
  selfManaged: boolean;
}): OnboardingStep | null {
  switch (args.from) {
    case "profile":
      return args.showCli ? "cli-install" : null;
    case "role":
      return "profile";
    case "use-case":
      return "role";
    case "workspace":
      return args.selfManaged ? "profile" : "use-case";
    case "cli-install":
    case "invite":
      return null;
  }
}

/** Header progress (AUTH-029): the current step over the visible total. */
export function onboardingProgress(args: { steps: readonly OnboardingStep[]; current: OnboardingStep }): {
  current: number;
  total: number;
} {
  return { current: args.steps.indexOf(args.current) + 1, total: args.steps.length };
}

// --- header account label (AUTH-018 seam) -------------------------------------

/** The account menu names the display name, else first-plus-last, else the address. */
export function displayNameForAccount(args: {
  display_name: string;
  first_name: string;
  last_name: string;
  email: string;
}): string {
  if (args.display_name) return args.display_name;
  if (args.first_name) return `${args.first_name} ${args.last_name}`.trim();
  return args.email;
}

// --- finish payload (AUTH-039, shared with S8) --------------------------------

/**
 * The profile patch finishing writes: every progress flag true plus the
 * last-workspace pointer at the first workspace (left out when the user
 * belongs nowhere yet).
 */
export function finishProfileStamp(workspaces: WorkspaceList | undefined): ProfilePatch {
  const patch: ProfilePatch = {
    onboarding_step: {
      profile_complete: true,
      workspace_create: true,
      workspace_invite: true,
      workspace_join: true,
    },
  };
  const first = workspaces?.[0];
  if (first !== undefined) patch.last_workspace_id = first.id;
  return patch;
}
