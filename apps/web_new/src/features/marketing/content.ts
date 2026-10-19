// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing copy and destinations (CLOUD-003/004, parity/specs/cloud.md
// §1). Copy is ours; only the destinations are fixed by the oracle
// (hero actions /login, /downloads, /docs; closing action /login; the
// repository, terms and privacy leave the app).

/** Public repository the open-source panel and frame link to. */
export const REPOSITORY_URL = "https://github.com/The-AI-Republic/pi-dash";

/** Company-site legal pages the footer links to. */
export const TERMS_URL = "https://airepublic.com/terms";
export const PRIVACY_URL = "https://airepublic.com/privacy";

/** Where sign-in lives for cloud builds. */
export const SIGN_IN_PATH = "/login";

export interface MarketingLink {
  label: string;
  href: string;
  external?: boolean;
}

/** Header actions, in the order the frame presents them (CLOUD-004). */
export const HEADER_LINKS: readonly MarketingLink[] = [
  { label: "Downloads", href: "/downloads" },
  { label: "Docs", href: "/docs" },
  { label: "Pricing", href: "/pricing" },
  { label: "GitHub", href: REPOSITORY_URL, external: true },
  { label: "Sign in", href: SIGN_IN_PATH },
  { label: "Start free", href: SIGN_IN_PATH },
];

/** Footer actions: product links first, then legal pages (CLOUD-004). */
export const FOOTER_LINKS: readonly MarketingLink[] = [
  { label: "Downloads", href: "/downloads" },
  { label: "Docs", href: "/docs" },
  { label: "Pricing", href: "/pricing" },
  { label: "GitHub", href: REPOSITORY_URL, external: true },
  { label: "Terms", href: TERMS_URL },
  { label: "Privacy", href: PRIVACY_URL },
];

export interface HeroAction extends MarketingLink {
  /** Emphasis used by the hero; the first action is the primary one. */
  emphasis: "primary" | "secondary";
}

/** The three onward actions the hero offers (order is asserted). */
export const HERO_ACTIONS: readonly HeroAction[] = [
  { label: "Sign in", href: SIGN_IN_PATH, emphasis: "primary" },
  { label: "Download the app", href: "/downloads", emphasis: "secondary" },
  { label: "Read the docs", href: "/docs", emphasis: "secondary" },
];

/** The four highlights under the hero. */
export const PRODUCT_LOOP: readonly { title: string; body: string }[] = [
  {
    title: "Issues in",
    body: "Every ticket, project and board in one workspace.",
  },
  {
    title: "Agents working",
    body: "Runners pick up delegated issues and keep going.",
  },
  {
    title: "Checks running",
    body: "Types, lint and tests gate every change.",
  },
  {
    title: "Reviewed to done",
    body: "Humans step in only where judgment is needed.",
  },
];

/** The four delivery steps, in order. */
export const DELIVERY_STEPS: readonly { title: string; body: string }[] = [
  {
    title: "Scope the outcome",
    body: "Describe the result and the checks that prove it. Acceptance criteria travel with the issue.",
  },
  {
    title: "Hand it to a runner",
    body: "Work is delegated to an agent on a machine you control, with the repository and context it needs.",
  },
  {
    title: "Implement and validate",
    body: "The agent edits, runs the project's own checks and reports what it changed.",
  },
  {
    title: "Review to done",
    body: "A second pass checks the result against the issue and the specification before it merges.",
  },
];

/** The four capability entries. */
export const CAPABILITIES: readonly { title: string; body: string }[] = [
  {
    title: "Orchestration across the loop",
    body: "Plan, build, review and test hand off to each other without losing the issue's context.",
  },
  {
    title: "Choose the agent",
    body: "Different models and coding agents can take different issues; the workflow stays the same.",
  },
  {
    title: "Runs on your machines",
    body: "Execution happens on runners you enroll, against repositories you already own.",
  },
  {
    title: "Human control at the edges",
    body: "Decisions, escalations and approvals stay with people, on the issues that need them.",
  },
];

/** Positioning line shared by the hero and the footer. */
export const POSITIONING_LINE = "Project management that keeps agents working until the work is done.";

/** Title / description pairs, used for the document head (CLOUD-005). */
export interface MarketingPageMeta {
  title: string;
  description: string;
}

export const MARKETING_META: Readonly<Record<string, MarketingPageMeta>> = {
  "/home": {
    title: "Pi Dash — agents that drive issues to done",
    description:
      "Pi Dash orchestrates AI agents across planning, implementation, review and testing so issues reach done with minimal human interaction.",
  },
  "/docs": {
    title: "Pi Dash docs",
    description: "How Pi Dash connects projects, runners and agents, and how to install it on your own machines.",
  },
  "/downloads": {
    title: "Download Pi Dash",
    description: "Desktop installers for macOS, Windows and Linux, plus the command-line setup for runners.",
  },
  "/pricing": {
    title: "Pi Dash pricing",
    description: "A free tier for small work and a membership for teams that run agents continuously.",
  },
};

export function metaFor(pathname: string): MarketingPageMeta {
  return MARKETING_META[pathname] ?? MARKETING_META["/home"]!;
}
