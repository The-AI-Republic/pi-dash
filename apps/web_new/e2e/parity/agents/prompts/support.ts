// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared helpers for the prompts/automations oracle specs (NEWFRONT-186).
// Poll-based reads keep every scenario convergent on the shared stack.
import { expect } from "@playwright/test";
import type { ParityPromptSection } from "../../helpers/api";

/** Browsed prompt kinds, in page order. */
export const PROMPT_KINDS = ["coding-task", "review", "scheduler"] as const;

/** Kind slug to visible label. */
export const PROMPT_KIND_LABELS: Record<string, string> = {
  "coding-task": "Coding task",
  review: "Review",
  scheduler: "Scheduler",
};

/** Overridable section used for edit flows (coding-task only). */
export const EDITABLE_SECTION = "autonomy";

/** Locked section used for refusal halves. */
export const LOCKED_SECTION = "blocking";

/** Poll visible toasts until one carries `title`; resolves with its message. */
export async function toastMessage(
  driver: { schedulerVisibleToasts: () => Promise<{ title: string; message: string }[]> },
  title: string
): Promise<string> {
  let message = "";
  await expect
    .poll(
      async () => {
        const found = (await driver.schedulerVisibleToasts()).find((toast) => toast.title === title);
        message = found?.message ?? "";
        return message;
      },
      { timeout: 30_000 }
    )
    .not.toBe("");
  return message;
}

/** Poll the sections browser until its card keys match (lists resolve async). */
export async function expectCardKeys(
  driver: { promptsSectionCards: () => Promise<{ key: string }[]> },
  keys: string[]
): Promise<void> {
  const wanted = [...keys].sort();
  await expect
    .poll(async () => (await driver.promptsSectionCards()).map((card) => card.key).sort(), { timeout: 30_000 })
    .toEqual(wanted);
}

/** Union of section keys across the per-kind lists, first-seen order. */
export function unionKeys(byKind: Map<string, ParityPromptSection[]>): string[] {
  return [...new Set([...byKind.values()].flat().map((section) => section.key))];
}

/** Poll one card until `match` holds; resolves with the card. */
export async function expectCard<T extends { key?: string }>(
  driver: { promptsSectionCard: (key: string) => Promise<T | null> },
  key: string,
  match: (card: T) => boolean
): Promise<T> {
  let current: T | null = null;
  await expect
    .poll(
      async () => {
        current = await driver.promptsSectionCard(key);
        return current !== null && match(current);
      },
      { timeout: 30_000 }
    )
    .toBe(true);
  if (current === null) throw new Error(`[parity] expected a ${key} card.`);
  return current;
}
