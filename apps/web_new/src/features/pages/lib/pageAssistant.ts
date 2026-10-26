// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page editor assistant (parity/specs/pages.md §9.3, PAGE-075). The
// cloud-gated writing menu that works on the body's text selection: its
// availability gate, its tone presets, the task composition it posts,
// and the failure lines it renders inline. The request itself lives in
// the sibling api module; this module stays pure and React-free.

/** Accessible name of the gutter handle that opens the assistant menu. */
export const PAGE_ASSISTANT_HANDLE_LABEL = "Ask Pi Dash";
/** Accessible name of the open assistant panel. */
export const PAGE_ASSISTANT_PANEL_LABEL = "Assistant";
/** Field label of the free-text instruction. */
export const PAGE_ASSISTANT_INSTRUCTION_LABEL = "Instruction";
/** Placeholder of the free-text instruction. */
export const PAGE_ASSISTANT_INSTRUCTION_PLACEHOLDER = "Describe the change…";
/** Validation hint when the instruction is submitted empty. */
export const PAGE_ASSISTANT_EMPTY_INSTRUCTION = "Describe the change first.";
/** Heading above the read-only reply. */
export const PAGE_ASSISTANT_REPLY_LABEL = "Reply";
/** First-run submit label. */
export const PAGE_ASSISTANT_GENERATE_LABEL = "Generate";
/** Submit label while a task is in flight. */
export const PAGE_ASSISTANT_GENERATING_LABEL = "Generating…";
/** Submit label once a reply exists (re-runs the same task). */
export const PAGE_ASSISTANT_REGENERATE_LABEL = "Regenerate";
/** Replace the snapshotted selection with the reply. */
export const PAGE_ASSISTANT_APPLY_LABEL = "Apply in place";
/** Insert the reply as a paragraph below the snapshotted selection. */
export const PAGE_ASSISTANT_APPEND_LABEL = "Append below";
/** Close the menu; closing resets the task state. */
export const PAGE_ASSISTANT_CLOSE_LABEL = "Close";
/** Loading line while the task is in flight. */
export const PAGE_ASSISTANT_LOADING_LABEL = "Writing a reply…";
/** Group label of the tone presets. */
export const PAGE_ASSISTANT_TONE_LABEL = "Tone";
/** Failure line when the task fails without a server message. */
export const PAGE_ASSISTANT_FAILED_BODY = "The assistant couldn't answer that. Try again in a moment.";
/** Failure line when the member's assistant quota is spent (429). */
export const PAGE_ASSISTANT_QUOTA_BODY = "The assistant quota for this workspace is spent for now.";

/** One tone preset: the direction it appends to the instruction, if any. */
export interface PageAssistantTone {
  key: string;
  label: string;
  /** Extra instruction sent with the task; null leaves the task untouched. */
  direction: string | null;
}

/**
 * Tone presets, in menu order. The task endpoint answers `{task, prompt}`
 * only — it takes no tone parameters — so a preset adjusts formality by
 * extending the instruction text, and the licensed endpoint effects stay
 * a recorded gap rather than invented coverage (spec §9.3).
 */
export const PAGE_ASSISTANT_TONES: readonly PageAssistantTone[] = [
  { key: "default", label: "Default", direction: null },
  { key: "formal", label: "Formal", direction: "Answer in a formal tone." },
  { key: "casual", label: "Casual", direction: "Answer in a casual tone." },
];

/** The preset a fresh menu opens with. */
export const PAGE_ASSISTANT_DEFAULT_TONE = "default";

/**
 * Compose the task text for one assistant call: the free-text
 * instruction plus the preset's tone direction. An unknown preset key
 * leaves the instruction untouched.
 */
export function pageAssistantTask(instruction: string, toneKey: string): string {
  const tone = PAGE_ASSISTANT_TONES.find((entry) => entry.key === toneKey);
  if (tone === undefined || tone.direction === null) return instruction;
  return `${instruction}\n\n${tone.direction}`;
}

/** The three inputs the assistant gate reads. */
export interface PageAssistantAccess {
  /** The edition declares the page-assistant flag (cloud builds). */
  editionEnabled: boolean;
  /** The viewer stands as a project member or above. */
  authorized: boolean;
  /** The instance has an LLM configuration to answer with. */
  configured: boolean;
}

/**
 * Whether the assistant handle may mount. Every input must hold; the
 * server enforces workspace member-or-admin regardless, so this gate
 * fails closed.
 */
export function pageAssistantReady(access: PageAssistantAccess): boolean {
  return access.editionEnabled && access.authorized && access.configured;
}

/** One server line off an error body, when the body carries a real line. */
function assistantServerLine(error: unknown, field: "detail" | "error"): string | null {
  const body = (error as { body?: unknown } | null)?.body;
  if (typeof body !== "object" || body === null) return null;
  const line = (body as Record<string, unknown>)[field];
  return typeof line === "string" && line !== "" ? line : null;
}

/**
 * The message a failed task renders inline: the server's detail or code
 * when the body carries one, the quota line for a bare 429, else the
 * generic line.
 */
export function pageAssistantFailureMessage(error: unknown): string {
  const detail = assistantServerLine(error, "detail");
  const code = assistantServerLine(error, "error");
  const status = (error as { status?: unknown } | null)?.status;
  if (status === 429) return detail ?? code ?? PAGE_ASSISTANT_QUOTA_BODY;
  return detail ?? code ?? PAGE_ASSISTANT_FAILED_BODY;
}

/**
 * Wrap a reply as the paragraph the append control inserts below the
 * selection. The task endpoint answers inline markup (newlines as
 * breaks), so one paragraph holds the whole reply.
 */
export function pageAssistantBelowHtml(html: string): string {
  return `<p>${html}</p>`;
}
