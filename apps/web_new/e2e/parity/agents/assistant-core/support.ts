// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared helpers for the assistant-core oracle specs (NEWFRONT-187).
// Poll-based reads keep every scenario convergent on the shared stack.
import { expect } from "@playwright/test";
import type { AssistantBubble, ParityDriver } from "../../drivers/parity-driver";

/**
 * Dummy provider config: a refused-port base URL fails turns fast with the
 * provider-unreachable detail. Dummy values only — never a real key; every
 * spec deletes the config again afterwards.
 */
export const DUMMY_PROVIDER = {
  provider_kind: "openai_compatible",
  base_url: "http://127.0.0.1:9/",
  model_name: "parity-dummy",
  api_key: "parity-dummy-key-not-real",
};

/** Dummy speech config (same refused-port trick, restored after). */
export const DUMMY_STT = {
  base_url: "http://127.0.0.1:9/",
  model_name: "parity-dummy-stt",
  api_key: "parity-dummy-key-not-real",
};

/** Dead tool-server URL: refused port, no DNS, fails at run entry. */
export const DEAD_MCP_URL = "http://127.0.0.1:9/mcp";

/** Turn-failure detail the seeded stack produces for the dummy provider. */
export const PROVIDER_UNREACHABLE = "Could not reach the configured provider endpoint.";

/** Keyless-composer lockdown line both thread and landing share. */
export const API_KEY_REMINDER = "Please set your API key in Settings first to start using AI Assistant.";

/** Sidebar/dashboard fallback for untitled threads. */
export const UNTITLED = "Untitled conversation";

/** Poll the newest toast until `title` shows; resolves with its message. */
export async function assistantToast(
  driver: { assistantLastToast: () => Promise<{ title: string; message: string } | null> },
  title: string
): Promise<string> {
  let message = "";
  await expect
    .poll(
      async () => {
        const toast = await driver.assistantLastToast();
        message = toast !== null && toast.title === title ? toast.message : "";
        return message;
      },
      { timeout: 30_000 }
    )
    .not.toBe("");
  return message;
}

/** Poll transcript bubbles until `match` holds; resolves with the bubbles. */
export async function expectBubbles(
  driver: Pick<ParityDriver, "assistantBubbles">,
  match: (bubbles: AssistantBubble[]) => boolean
): Promise<AssistantBubble[]> {
  let current: AssistantBubble[] = [];
  await expect
    .poll(
      async () => {
        current = await driver.assistantBubbles();
        return match(current);
      },
      { timeout: 60_000 }
    )
    .toBe(true);
  return current;
}

/** Poll the inline error line until it carries text; resolves with it. */
export async function expectErrorLine(driver: Pick<ParityDriver, "assistantErrorLine">): Promise<string> {
  let line: string | null = null;
  await expect
    .poll(
      async () => {
        line = await driver.assistantErrorLine();
        return line ?? "";
      },
      { timeout: 60_000 }
    )
    .not.toBe("");
  return line ?? "";
}

/** Poll sidebar titles until they equal `titles` top to bottom. */
export async function expectSidebarTitles(
  driver: Pick<ParityDriver, "assistantSidebarThreads">,
  titles: string[]
): Promise<void> {
  await expect
    .poll(async () => (await driver.assistantSidebarThreads()).map((row) => row.title), {
      timeout: 60_000,
    })
    .toEqual(titles);
}

/**
 * Dismiss the first-visit product tour covering the dashboard. Fresh users
 * always get the tour until declined, so the welcome is polled for rather
 * than probed once (the profile fetch can trail the dashboard render).
 */
export async function dismissTour(driver: Pick<ParityDriver, "isTourWelcomeVisible" | "declineTour">): Promise<void> {
  await expect.poll(() => driver.isTourWelcomeVisible(), { timeout: 60_000 }).toBe(true);
  await driver.declineTour();
  await expect.poll(() => driver.isTourWelcomeVisible(), { timeout: 30_000 }).toBe(false);
}

/** Poll skipped-server notice lines until `match` holds. */
export async function expectNoticeLines(
  driver: Pick<ParityDriver, "assistantNoticeLines">,
  match: (lines: string[]) => boolean
): Promise<string[]> {
  let current: string[] = [];
  await expect
    .poll(
      async () => {
        current = await driver.assistantNoticeLines();
        return match(current);
      },
      { timeout: 60_000 }
    )
    .toBe(true);
  return current;
}
