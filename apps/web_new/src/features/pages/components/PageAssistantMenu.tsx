// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page assistant menu (parity/specs/pages.md §9.3, PAGE-075). The
// cloud-gated writing menu on the editor handle: a gutter entry opens a
// compact panel where a free-text instruction plus a tone preset runs one
// conversational task over the snapshotted selection. The reply renders
// read-only with apply-in-place, append-below and regenerate; loading
// and failure render inline; closing unmounts the panel, which resets
// the task state.

import { Button, Textarea } from "@pidash/kit";
import { Sparkle } from "lucide-react";
import * as React from "react";

import { usePageAssistantAnswer } from "../api/assistantMutations.js";
import {
  PAGE_ASSISTANT_APPEND_LABEL,
  PAGE_ASSISTANT_APPLY_LABEL,
  PAGE_ASSISTANT_CLOSE_LABEL,
  PAGE_ASSISTANT_EMPTY_INSTRUCTION,
  PAGE_ASSISTANT_GENERATE_LABEL,
  PAGE_ASSISTANT_GENERATING_LABEL,
  PAGE_ASSISTANT_HANDLE_LABEL,
  PAGE_ASSISTANT_INSTRUCTION_LABEL,
  PAGE_ASSISTANT_INSTRUCTION_PLACEHOLDER,
  PAGE_ASSISTANT_LOADING_LABEL,
  PAGE_ASSISTANT_PANEL_LABEL,
  PAGE_ASSISTANT_REGENERATE_LABEL,
  PAGE_ASSISTANT_REPLY_LABEL,
  PAGE_ASSISTANT_TONE_LABEL,
  PAGE_ASSISTANT_TONES,
  pageAssistantFailureMessage,
  pageAssistantTask,
  PAGE_ASSISTANT_DEFAULT_TONE,
} from "../lib/pageAssistant.js";

/** Notice when the task answers without text. */
export const PAGE_ASSISTANT_EMPTY_REPLY = "The reply came back empty. Add detail to the instruction and try again.";

/** The gutter entry beside the block handle. */
export function PageAssistantHandle({ onAsk }: { onAsk: () => void }): React.ReactElement {
  return (
    <button
      type="button"
      aria-label={PAGE_ASSISTANT_HANDLE_LABEL}
      data-testid="page-ai-handle"
      onMouseDown={(event) => event.preventDefault()}
      onClick={onAsk}
      className="grid size-6 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
    >
      <Sparkle size={14} strokeWidth={1.75} aria-hidden="true" />
    </button>
  );
}

export interface PageAssistantMenuProps {
  workspaceSlug: string;
  /** The selection snapshot the task runs over. */
  selection: string;
  /** Replace the snapshotted selection with the sanitized reply. */
  onApply: (html: string) => void;
  /** Insert the sanitized reply below the snapshotted selection. */
  onAppend: (html: string) => void;
  /** Close the menu; the unmount resets the task state. */
  onClose: () => void;
}

export function PageAssistantMenu({
  workspaceSlug,
  selection,
  onApply,
  onAppend,
  onClose,
}: PageAssistantMenuProps): React.ReactElement {
  const [instruction, setInstruction] = React.useState("");
  const [tone, setTone] = React.useState<string>(PAGE_ASSISTANT_DEFAULT_TONE);
  const [answer, setAnswer] = React.useState<{ text: string; html: string } | null>(null);
  const [failure, setFailure] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [invalid, setInvalid] = React.useState(false);
  const task = usePageAssistantAnswer(workspaceSlug);

  async function runWith(preset: string): Promise<void> {
    const text = instruction.trim();
    if (text === "") {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setFailure(null);
    setNotice(null);
    try {
      // The markup is cleaned before it is ever shown or inserted; the
      // cleaner loads on demand so the panel never carries it eagerly.
      const [{ sanitizeHtml }, reply] = await Promise.all([
        import("../../../shared/editor/sanitize.js"),
        task.mutateAsync({ task: pageAssistantTask(text, preset), prompt: selection }),
      ]);
      const html = sanitizeHtml(reply.html);
      setAnswer({ text: reply.text, html });
      if (reply.text === "") setNotice(PAGE_ASSISTANT_EMPTY_REPLY);
    } catch (error) {
      setFailure(pageAssistantFailureMessage(error));
    }
  }

  // A preset picked after a reply re-runs the task with the new tone; a
  // preset picked before the first run only arms it.
  function pickTone(preset: string): void {
    setTone(preset);
    if (answer !== null) void runWith(preset);
  }

  const expanded = task.isPending || answer !== null || failure !== null || notice !== null;
  const usable = answer !== null && answer.html !== "";
  return (
    <div
      data-testid="page-assistant-menu"
      role="dialog"
      aria-label={PAGE_ASSISTANT_PANEL_LABEL}
      className={`shadow-popover flex max-h-96 flex-col gap-(--space-2) overflow-y-auto rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-3) ${
        expanded ? "w-[30rem] max-w-[calc(100vw-2rem)]" : "w-72 max-w-[calc(100vw-2rem)]"
      }`}
    >
      {answer !== null && answer.html !== "" ? (
        <div className="flex flex-col gap-(--space-1)">
          <p className="text-meta font-medium text-(--text)">{PAGE_ASSISTANT_REPLY_LABEL}</p>
          <div
            data-testid="page-assistant-reply"
            className="text-body max-h-40 overflow-y-auto rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2) text-(--text)"
            dangerouslySetInnerHTML={{ __html: answer.html }}
          />
        </div>
      ) : null}
      {task.isPending ? (
        <p role="status" className="text-body text-(--text-muted)">
          {PAGE_ASSISTANT_LOADING_LABEL}
        </p>
      ) : null}
      {failure !== null ? (
        <p data-testid="page-assistant-error" role="alert" className="text-meta text-(--danger)">
          {failure}
        </p>
      ) : null}
      {notice !== null ? (
        <p data-testid="page-assistant-notice" role="status" className="text-meta text-(--text-muted)">
          {notice}
        </p>
      ) : null}
      <Textarea
        label={PAGE_ASSISTANT_INSTRUCTION_LABEL}
        placeholder={PAGE_ASSISTANT_INSTRUCTION_PLACEHOLDER}
        rows={3}
        value={instruction}
        onChange={(event) => setInstruction(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void runWith(tone);
          }
        }}
        {...(invalid ? { error: PAGE_ASSISTANT_EMPTY_INSTRUCTION } : {})}
      />
      <div role="radiogroup" aria-label={PAGE_ASSISTANT_TONE_LABEL} className="flex items-center gap-(--space-1)">
        {PAGE_ASSISTANT_TONES.map((preset) => (
          <button
            key={preset.key}
            type="button"
            role="radio"
            aria-checked={tone === preset.key}
            onClick={() => pickTone(preset.key)}
            className={
              tone === preset.key
                ? "text-meta rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) font-medium text-(--text)"
                : "text-meta rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            }
          >
            {preset.label}
          </button>
        ))}
      </div>
      <div className="flex justify-end gap-(--space-2)">
        {usable && answer !== null ? (
          <>
            <Button
              variant="secondary"
              size="small"
              onClick={() => {
                onApply(answer.html);
              }}
            >
              {PAGE_ASSISTANT_APPLY_LABEL}
            </Button>
            <Button
              variant="secondary"
              size="small"
              onClick={() => {
                onAppend(answer.html);
              }}
            >
              {PAGE_ASSISTANT_APPEND_LABEL}
            </Button>
          </>
        ) : null}
        <Button variant="ghost" size="small" onClick={onClose}>
          {PAGE_ASSISTANT_CLOSE_LABEL}
        </Button>
        <Button variant="primary" size="small" disabled={task.isPending} onClick={() => void runWith(tone)}>
          {task.isPending
            ? PAGE_ASSISTANT_GENERATING_LABEL
            : answer !== null
              ? PAGE_ASSISTANT_REGENERATE_LABEL
              : PAGE_ASSISTANT_GENERATE_LABEL}
        </Button>
      </div>
    </div>
  );
}
