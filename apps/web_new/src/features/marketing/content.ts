// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing copy and destinations (CLOUD-003/004/006–009,
// parity/specs/cloud.md §1–§2). Copy is ours; only the destinations,
// counts and pinned names are fixed by the oracle (hero actions /login,
// /downloads, /docs; closing action /login; the repository, terms and
// privacy leave the app; the stable release URLs; the pricing tier names
// and prices).

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
  { label: "Start free", href: SIGN_IN_PATH, emphasis: "primary" },
  { label: "Download the app", href: "/downloads", emphasis: "secondary" },
  { label: "Read the docs", href: "/docs", emphasis: "secondary" },
];

/** Small pill above the hero headline. */
export const HERO_BADGE = "Pi Dash · AI worker management";

/** Hero headline, one entry per rendered line. */
export const HERO_HEADLINES = ["Run your own AI", "like a team lead"] as const;

export const HERO_INTRO =
  "Pi Dash is an orchestration platform for asynchronous software work. You decide what should be done; Pi Dash hands it to an agent, follows the run, and keeps the issue moving until it is finished.";

export const HERO_NOTE = "Free to start · Codex, Claude Code or your preferred agent";

/** The hero illustration: a representative run, no data and no behavior. */
export const RUN_PREVIEW = {
  issue: "NEWFRONT-645",
  title: "Cloud marketing surface",
  status: "Running",
  heading: "Work plan",
  steps: [
    { label: "Read the issue", state: "done" },
    { label: "Build the sections", state: "done" },
    { label: "Run the checks", state: "active" },
    { label: "Open code review", state: "queued" },
  ],
  runner: { name: "Codex runner · online", note: "Working on an isolated branch of your repository." },
  activity: {
    heading: "Live activity",
    headline: "Sections are in place.",
    body: "Validating the public pages against the built site now.",
  },
  checks: {
    heading: "Checks",
    rows: [
      { label: "Type check", value: "Passed", state: "done" },
      { label: "Production build", value: "Running", state: "active" },
    ],
  },
  review: {
    title: "Ready for review",
    body: "Branch, commits, tests and issue context stay linked.",
  },
} as const;

/** The four highlights under the hero. */
export const PRODUCT_LOOP: readonly { title: string; body: string }[] = [
  { title: "Durable workpads", body: "Context survives every agent run." },
  { title: "Enrolled runners", body: "Agents work on machines you own." },
  { title: "Structured escalation", body: "People decide only where it counts." },
  { title: "Review-ready changes", body: "Code, proof and issue stay linked." },
];

/** The four delivery steps, in order. */
export const DELIVERY_STEPS: readonly { title: string; body: string }[] = [
  {
    title: "State the outcome",
    body: "One issue carries the goal, the acceptance criteria and the project context.",
  },
  {
    title: "Hand it to a runner",
    body: "Pi Dash gives the issue to an agent on a development machine you have connected.",
  },
  {
    title: "Build and verify",
    body: "The agent changes the code, runs the project's checks and records its progress as it goes.",
  },
  {
    title: "Review to done",
    body: "A review pass checks the result, findings are addressed, and the issue reaches done.",
  },
];

/** The four capability entries. */
export const CAPABILITIES: readonly { title: string; body: string }[] = [
  {
    title: "Issues to pull requests",
    body: "Planning, execution, validation and review share one workflow and one issue state.",
  },
  {
    title: "Your agent, your choice",
    body: "Pick Codex, Claude Code or another CLI agent per issue without changing how work is scoped.",
  },
  {
    title: "Your machines, your setup",
    body: "Runners execute where the repositories, tools and credentials already live.",
  },
  {
    title: "Humans where it counts",
    body: "Agents keep the context and move on their own, escalating only the decisions that need a person.",
  },
];

/** Section headings and supporting lines for the marketing home. */
export const HOME_SECTIONS = {
  steps: {
    eyebrow: "From issue to outcome",
    title: "Stop babysitting terminals. Start finishing work.",
    intro:
      "Pi Dash gives each agent enough structure to move on its own, and your team a clear record of what happened.",
  },
  capabilities: {
    eyebrow: "Built for agentic delivery",
    title: "The control plane between your backlog and your agents.",
  },
  openSource: {
    eyebrow: "Open source",
    title: "Built in the open. Yours to run.",
    body: "The Pi Dash core is available under the AGPL-3.0 license. Inspect the code, contribute, or self-host the platform and connect your own machines and agents.",
    action: "View Pi Dash on GitHub",
  },
  closing: {
    eyebrow: "Put an agent on it",
    title: "Give every good issue a path to done.",
    body: "Start with a free workspace, connect a development machine, and let Pi Dash coordinate the rest.",
    action: "Start using Pi Dash",
  },
} as const;

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

// --- Docs page (CLOUD-006, parity/specs/cloud.md §2) ---

/** The three parts of the model the docs page teaches, in order. */
export const DOCS_CONCEPTS: readonly { title: string; body: string }[] = [
  {
    title: "The cloud",
    body: "Hosted projects, issues and boards: where work is scoped, handed to agents and reviewed to done.",
  },
  {
    title: "The command line",
    body: "The local CLI that signs a machine in and carries commands, context and results both ways.",
  },
  {
    title: "The runner",
    body: "One agent bound to one project on a machine you control: where delegated work actually runs.",
  },
];

/** The four quick-start steps, in order. Commands render inside their step. */
export const DOCS_STEPS: readonly { title: string; body: string }[] = [
  {
    title: "Create a project",
    body: "Sign in, create a workspace and a project, and add at least one well-scoped issue so the first run has something concrete to drive.",
  },
  {
    title: "Install the CLI",
    body: "One command per platform downloads the installer, verifies it and drops the CLI on the path.",
  },
  {
    title: "Sign the machine in",
    body: "Device login pairs the host with the cloud. The installers start this step for you; run it by hand after an MSI install, on a headless host, or when re-authenticating.",
  },
  {
    title: "Enroll a runner",
    body: "Pick the project and the agent in the product, then run the enrollment commands on the host and check the diagnostics.",
  },
];

export interface DocsCommand {
  /** Short caption naming the platform or purpose. */
  label: string;
  /** Shell text, transcribed from the runner install guide. */
  command: string;
}

/** Per-OS installer one-liners (both reference the stable release-download address). */
export const DOCS_INSTALL_COMMANDS: readonly DocsCommand[] = [
  {
    label: "macOS and Linux",
    command:
      "curl --proto '=https' --tlsv1.2 -LsSf \\\n  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh",
  },
  {
    label: "Windows (PowerShell)",
    command: "irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.ps1 | iex",
  },
];

/** Device login against the hosted origin. */
export const DOCS_DEVICE_LOGIN_COMMAND = "pidash auth login";

/** Runner enrollment plus the diagnostics check, run on the host. */
export const DOCS_ENROLL_COMMAND = "pidash runner add --project <PROJECT_ID>\npidash doctor";

/** Optional agent skill installer: one command installs the skill into the coding agent. */
export const DOCS_SKILL_COMMAND = "npx @airepublic/pidash-skill-installer";

/** Managed Windows deployments can skip the PowerShell one-liner. */
export const DOCS_MSI_NOTE = "Managed Windows deployments can use the MSI installer instead.";

/** A Linux-subsystem install stays inside that subsystem. */
export const DOCS_WSL_NOTE =
  "A Linux-subsystem install only drives agents inside that subsystem; when the coding agents live on the Windows side, use the PowerShell command instead.";

/** The four phases of the operating loop, in order. */
export const DOCS_LOOP_PHASES: readonly { title: string; body: string }[] = [
  { title: "Delegate", body: "Scope an issue and hand it to a runner." },
  { title: "Execute", body: "The agent implements on its own machine." },
  { title: "Validate", body: "The project's checks prove the change." },
  { title: "Review", body: "A second pass takes it to done." },
];

/** The three trust notes closing the page. */
export const DOCS_TRUST_NOTES: readonly { title: string; body: string }[] = [
  {
    title: "Stays on your host",
    body: "Source checkouts and credentials never leave the runner machine.",
  },
  {
    title: "Access you grant",
    body: "Machine access is explicit, scoped and revocable at any time.",
  },
  {
    title: "Open at the core",
    body: "The core is open source and self-hostable; the cloud is optional.",
  },
];

// --- Downloads page (CLOUD-007/008, parity/specs/cloud.md §2) ---

/**
 * Stable desktop-release download base (CLOUD-007): per-format filenames
 * under one latest address, so the page needs no per-release edits.
 */
export const DESKTOP_RELEASE_BASE = "https://pidash-desktop-releases.s3.us-west-2.amazonaws.com/downloads/latest";

/** Standalone Windows CLI installer (CLOUD-006 MSI alternative, CLOUD-008 CLI card). */
export const CLI_MSI_URL =
  "https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi";

export interface DownloadFormat {
  label: string;
  href: string;
}

export interface DownloadCard {
  /** Operating system, exactly as the oracle names it. */
  os: string;
  /** Architecture and format note. */
  arch: string;
  primary: DownloadFormat;
  /** Second format where one exists; macOS ships one. */
  secondary: DownloadFormat | null;
}

/** One card per supported OS, in oracle order. */
export const DOWNLOAD_CARDS: readonly DownloadCard[] = [
  {
    os: "macOS",
    arch: "Universal disk image · Apple silicon and Intel",
    primary: { label: "Download for macOS", href: `${DESKTOP_RELEASE_BASE}/pidash-macos-universal.dmg` },
    secondary: null,
  },
  {
    os: "Windows",
    arch: "x64 installer · MSI available",
    primary: { label: "Download for Windows", href: `${DESKTOP_RELEASE_BASE}/pidash-windows-x64-setup.exe` },
    secondary: { label: "MSI installer", href: `${DESKTOP_RELEASE_BASE}/pidash-windows-x64.msi` },
  },
  {
    os: "Linux",
    arch: "x64 AppImage · Debian package available",
    primary: { label: "Download for Linux", href: `${DESKTOP_RELEASE_BASE}/pidash-linux-x64.AppImage` },
    secondary: { label: "Debian package", href: `${DESKTOP_RELEASE_BASE}/pidash-linux-x64.deb` },
  },
];

/** Unsigned-installer notice (CLOUD-007): pinned wording, matched by the parity read. */
export const DOWNLOADS_SIGNING_NOTICE =
  "These installers are not yet publisher-signed, so your operating system may warn about an unverified publisher.";

/** Post-install sign-in hint for the CLI card (CLOUD-008): pinned command, matched by the parity read. */
export const CLI_SIGNIN_HINT = "Open a fresh terminal after installing, then run pidash auth login to finish setup.";

// --- Pricing page (CLOUD-009, parity/specs/cloud.md §2) ---

/** External membership checkout and enterprise contact destinations. */
export const MEMBERSHIP_CHECKOUT_URL = "https://airepublic.com/pricing";
export const ENTERPRISE_CONTACT_URL = "https://airepublic.com/contact";

export interface PricingTier extends MarketingLink {
  /** Tier name, exactly as the oracle names it. */
  name: string;
  /** Price line, exactly as the oracle renders it. */
  price: string;
  /** Billing mechanics under the price. */
  billing: string;
  /** Feature bullets: five per tier. */
  features: readonly string[];
}

/** The two tiers, free first, in oracle order. */
export const PRICING_TIERS: readonly PricingTier[] = [
  {
    name: "Free + pay as you go",
    price: "$0",
    billing: "No platform fee",
    features: [
      "Up to 500 created issues",
      "Unlimited machines and agents",
      "Wallet-funded metered AI usage",
      "The full loop, delegate to review",
      "Community support",
    ],
    label: "Start for free",
    href: SIGN_IN_PATH,
  },
  {
    name: "AI Republic membership",
    price: "$7.99",
    billing: "Per month, billed annually plus tax",
    features: [
      "Uncapped issue creation",
      "Larger included AI usage quotas",
      "Everything in Free, included",
      "Pay-as-you-go past the included quotas",
      "Annual billing through secure checkout",
    ],
    label: "Become a member",
    href: MEMBERSHIP_CHECKOUT_URL,
    external: true,
  },
];

/** The four FAQ entries, in page order. */
export const PRICING_FAQ: readonly { question: string; answer: string }[] = [
  {
    question: "What counts toward the free cap?",
    answer:
      "Each created issue or work item counts once. Hitting the cap stops new creation; everything already created stays available.",
  },
  {
    question: "What does the wallet cover?",
    answer: "Metered model and connected-app usage. The product itself stays free within the cap.",
  },
  {
    question: "Do members still pay beyond their quotas?",
    answer: "Yes. Usage past the included quotas continues pay-as-you-go from the same wallet.",
  },
  {
    question: "Are self-hosted deployments affected?",
    answer: "No. Hosted-plan limits never apply to self-hosted deployments.",
  },
];
