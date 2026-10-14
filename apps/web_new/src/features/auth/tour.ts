// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-run product tour logic (AUTH-042). The welcome stop plus the five
// content stops (work items, runners, chat, workpads, pages), the
// per-user visibility gate off the profile flag, and the step order the
// overlay walks. Written from parity/specs/auth.md.

import type { Profile } from "@pidash/api-client";

/** The product's public core-concept video, embedded on the welcome stop. */
export const TOUR_VIDEO_EMBED_URL = "https://www.youtube.com/embed/-sitbwrwjno";

/** Every tour position: the welcome entry plus the five content stops. */
export type TourStepKey = "welcome" | "work-items" | "runners" | "chat" | "workpads" | "pages";

export interface TourContentStop {
  key: Exclude<TourStepKey, "welcome">;
  /** Short sidebar label for the stop navigator. */
  navLabel: string;
  title: string;
  body: string;
}

/** The five content stops in walk order. */
export const TOUR_CONTENT_STOPS: readonly TourContentStop[] = [
  {
    key: "work-items",
    navLabel: "Work items",
    title: "Give every task a home",
    body: "A work item holds one piece of work: what it is, why it matters, and everything an agent needs to run with it. Write it once, then dispatch it or work it yourself.",
  },
  {
    key: "runners",
    navLabel: "Runners",
    title: "Put agents to work on your machine",
    body: "The Pi Dash Runner connects your computer to your workspace. Send it work items and it carries them out in the background with Claude Code or Codex.",
  },
  {
    key: "chat",
    navLabel: "Runner chat",
    title: "Guide agents while they run",
    body: "Each runner session has its own chat. Follow along live, answer questions, change direction mid-task, and review the result when it lands.",
  },
  {
    key: "workpads",
    navLabel: "Workpads",
    title: "Think alongside your agent",
    body: "Workpads are shared Markdown notes attached to every work item. Sketch the plan, record decisions, and leave handoff notes your agent can build on.",
  },
  {
    key: "pages",
    navLabel: "Pages",
    title: "Keep knowledge where agents find it",
    body: "Pages hold the notes that outlive any single task: specs, runbooks, and references your team — and your agents — can reach for any time.",
  },
];

/**
 * The tour gate: the overlay shows only once the profile has loaded and
 * only while the user has neither completed nor skipped the tour. Every
 * completion or dismissal flips the flag, so the tour runs once per user.
 */
export function shouldShowProductTour(profile: Pick<Profile, "is_tour_completed"> | null | undefined): boolean {
  return profile !== null && profile !== undefined && !profile.is_tour_completed;
}

/** The personalized welcome greeting; unnamed users get the bare greeting. */
export function tourWelcomeGreeting(firstName: string): string {
  return firstName.trim() === "" ? "Welcome to Pi Dash" : `Welcome to Pi Dash, ${firstName.trim()}`;
}

const TOUR_WALK_ORDER: readonly TourStepKey[] = ["welcome", "work-items", "runners", "chat", "workpads", "pages"];

/**
 * The step after `from` along the walk, or null past the last stop.
 * Welcome advances into the first content stop.
 */
export function nextTourStep(from: TourStepKey): TourStepKey | null {
  const at = TOUR_WALK_ORDER.indexOf(from);
  return at === -1 || at === TOUR_WALK_ORDER.length - 1 ? null : (TOUR_WALK_ORDER[at + 1] as TourStepKey);
}

/**
 * The step before `from`, or null where no Back control exists: welcome
 * is entry-only and the first content stop offers no way back to it.
 */
export function previousTourStep(from: TourStepKey): TourStepKey | null {
  if (from === "welcome" || from === "work-items") return null;
  const at = TOUR_WALK_ORDER.indexOf(from);
  return at === -1 ? null : (TOUR_WALK_ORDER[at - 1] as TourStepKey);
}

/** The content stop for `key`, or null on the welcome entry. */
export function tourContentStop(key: TourStepKey): TourContentStop | null {
  return TOUR_CONTENT_STOPS.find((stop) => stop.key === key) ?? null;
}
