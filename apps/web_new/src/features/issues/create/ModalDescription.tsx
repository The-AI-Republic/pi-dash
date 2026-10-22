// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modal description panel (parity/specs/issues.md §11, ISS-125/127).
// The rich description the create and edit forms share: a lazy Tiptap
// surface with person mentions and image upload, plus the assistant
// footer. "I'm feeling lucky" drafts a body from the title; the AI
// popover answers a typed task, shows the markup for review, and inserts
// it on confirmation (AGT-063). The panel leaves Enter to the editor
// (it starts a new line) and Escape to the dialog, which asks before
// discarding. A panel-side upload is not the dialog's write state, so
// the panel reports when it is still processing and the dialog refuses
// to close until it settles (ISS-125).

import { Button, Popover, cn } from "@pidash/kit";
import { Sparkle } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import {
  descriptionAssetSrc,
  hasStoredDescriptionContent,
  IssueDescriptionEditor,
  type RichTextEditorHandle,
} from "../../../shared/editor/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  DESCRIBE_EMPTY_TITLE,
  DESCRIBE_EMPTY_TITLE_BODY,
  DESCRIBE_FAILED_BODY,
  DESCRIBE_FAILED_TITLE,
  DESCRIBE_RATE_LIMITED_BODY,
  EDITOR_EMPTY_ANSWER,
  EDITOR_EMPTY_TASK_BODY,
  EDITOR_FAILED_TITLE,
  editorFailureMessage,
  isAssistantRateLimited,
  useDescribeFromTitle,
  useEditorAnswer,
  useMentionSearch,
  useUploadDescriptionAsset,
  type EditorAnswer,
  type MentionSearchResults,
} from "./createApi.js";
import type { DescriptionAssetScope } from "@pidash/api-client";

/** Accessible name of the description editor and its heading. */
export const DESCRIPTION_LABEL = "Description";
export const DESCRIBE_BUTTON_LABEL = "I'm feeling lucky";
export const DESCRIBE_BUSY_LABEL = "Writing…";
export const AI_BUTTON_LABEL = "AI";
export const AI_PROMPT_LABEL = "Ask the assistant";
export const AI_RESPONSE_LABEL = "Response";
export const AI_GENERATE_LABEL = "Generate response";
export const AI_REGENERATE_LABEL = "Generate again";
export const AI_GENERATING_LABEL = "Generating response…";
export const AI_USE_RESPONSE_LABEL = "Use this response";
export const AI_CLOSE_LABEL = "Close";
export const UPLOAD_FAILED_TITLE = "Couldn't upload the image";
export const UPLOAD_FAILED_BODY = "Check your connection and try again.";

/** Stored description content including images and mentions. */
export function hasDescriptionContent(html: string): boolean {
  return hasStoredDescriptionContent(html);
}

export interface ModalDescriptionProps {
  workspaceSlug: string;
  projectId: string;
  /** Current description HTML, owned by the form. */
  value: string;
  onChange: (html: string) => void;
  /** True while a write is in flight. */
  disabled: boolean;
  /** Title the assistant describes (create and edit alike). */
  title: string;
  /** Assistant configured on this instance. */
  assistantConfigured: boolean;
  /** Entity type and row the description's images park under. */
  assetScope: DescriptionAssetScope;
  /** Assets uploaded here, reported so the submit can link them. */
  onAssetUploaded: (assetId: string) => void;
  /** Remount key: the editor reads its content once at mount. */
  generation: number;
  /** Tells the dialog whether Escape/Discard may close right now (ISS-125). */
  onReadinessChange?: ((ready: boolean) => void) | undefined;
  /** Inserting text at the caret, e.g. a picked mention. */
  registerHandle?: (handle: RichTextEditorHandle | null) => void;
  /** Explicit keyboard position of the editing surface (ISS-125). */
  tabIndex?: number | undefined;
}

export function ModalDescription({
  workspaceSlug,
  projectId,
  value,
  onChange,
  disabled,
  title,
  assistantConfigured,
  assetScope,
  onAssetUploaded,
  generation,
  onReadinessChange,
  registerHandle,
  tabIndex,
}: ModalDescriptionProps): React.ReactElement {
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const [mentionQuery, setMentionQuery] = React.useState<string | null>(null);
  const [aiOpen, setAiOpen] = React.useState(false);
  const [aiPrompt, setAiPrompt] = React.useState("");
  const [aiAnswer, setAiAnswer] = React.useState<EditorAnswer | null>(null);
  const [aiInvalid, setAiInvalid] = React.useState(false);
  const describe = useDescribeFromTitle(workspaceSlug);
  const answerMutation = useEditorAnswer(workspaceSlug);
  const upload = useUploadDescriptionAsset(workspaceSlug);
  const mentions = useMentionSearch(workspaceSlug, projectId, mentionQuery ?? "", mentionQuery !== null);
  const fileRef = React.useRef<HTMLInputElement>(null);
  // Legacy bodies keep the asset id in the image source; resolve it
  // through the row's own project so an existing picture opens (ISS-127).
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId, apiBaseUrl: getAppClient().baseUrl }),
    [workspaceSlug, projectId]
  );
  // The panel's own work (an upload, a draft, an answer) never reaches
  // the dialog's write state, so readiness is counted here and reported
  // as it changes: closing mid-upload would strand the parked asset.
  const inFlight = React.useRef(0);
  const disabledRef = React.useRef(disabled);
  disabledRef.current = disabled;
  const reportReadiness = React.useCallback(() => {
    onReadinessChange?.(!disabledRef.current && inFlight.current === 0);
  }, [onReadinessChange]);

  async function withPanelWork<T>(work: () => Promise<T>): Promise<T> {
    inFlight.current += 1;
    reportReadiness();
    try {
      return await work();
    } finally {
      inFlight.current -= 1;
      reportReadiness();
    }
  }

  // The editor may be replaced once the lazy chunk settles; the parent
  // only ever sees the newest handle.
  const handleEditorReady = React.useCallback(
    (handle: RichTextEditorHandle) => {
      handleRef.current = handle;
      registerHandle?.(handle);
    },
    [registerHandle]
  );

  async function describeFromTitle(): Promise<void> {
    // The control only renders for a titled form, so the title is never
    // blank here; the endpoint still answers an empty body at times.
    try {
      const html = await withPanelWork(() => describe.mutateAsync({ projectId, title: title.trim() }));
      if (html === "") {
        notifyError(DESCRIBE_EMPTY_TITLE, DESCRIBE_EMPTY_TITLE_BODY);
        return;
      }
      onChange(html);
      // The editor reads its content at mount only; a new key makes it
      // adopt the generated body.
      setRemount((current) => current + 1);
    } catch (error) {
      const rateLimited = isAssistantRateLimited(error);
      notifyError(DESCRIBE_FAILED_TITLE, rateLimited ? DESCRIBE_RATE_LIMITED_BODY : DESCRIBE_FAILED_BODY);
    }
  }

  // Closing discards the draft request: task, answer and marker alike.
  function closeAi(): void {
    setAiOpen(false);
    setAiPrompt("");
    setAiAnswer(null);
    setAiInvalid(false);
  }

  async function generateAnswer(): Promise<void> {
    const task = aiPrompt.trim();
    if (task === "") {
      notifyError(EDITOR_FAILED_TITLE, EDITOR_EMPTY_TASK_BODY);
      return;
    }
    try {
      // The markup is cleaned before it is ever shown or inserted; the
      // cleaner loads on demand so the dialog never carries it eagerly.
      const [{ sanitizeHtml }, answer] = await withPanelWork(() =>
        Promise.all([
          import("../../../shared/editor/sanitize.js"),
          answerMutation.mutateAsync({ projectId, prompt: task }),
        ])
      );
      const html = sanitizeHtml(answer.html);
      setAiAnswer({ text: answer.text, html });
      setAiInvalid(answer.text === "");
    } catch (error) {
      notifyError(EDITOR_FAILED_TITLE, editorFailureMessage(error));
    }
  }

  // The insert button only renders once an answer with markup exists,
  // so the answer arrives as an argument instead of a guarded read.
  function useAnswer(answer: EditorAnswer): void {
    const handle = handleRef.current;
    if (handle !== null) {
      // The old editor inserts at `selection.anchor`; appending would
      // always land the answer at the end of the body (ISS-127).
      handle.insertHtml(answer.html);
    } else {
      onChange(value.trim() === "" ? answer.html : `${value}${answer.html}`);
      setRemount((currentKey) => currentKey + 1);
    }
    closeAi();
    notifySuccess("Inserted", "The assistant's answer is in the description.");
  }

  async function uploadImage(file: File): Promise<void> {
    try {
      const uploaded = await withPanelWork(() => upload.mutateAsync({ projectId, scope: assetScope, file }));
      onAssetUploaded(uploaded.assetId);
      handleRef.current?.insertImage(uploaded.assetUrl);
    } catch {
      notifyError(UPLOAD_FAILED_TITLE, UPLOAD_FAILED_BODY);
    }
  }

  function pickMention(id: string, name: string): void {
    handleRef.current?.insertMention({ id, name });
    setMentionQuery(null);
  }

  const [remount, setRemount] = React.useState(0);
  const busy = disabled || describe.isPending || answerMutation.isPending || upload.isPending;
  React.useEffect(() => {
    reportReadiness();
  }, [disabled, reportReadiness]);

  return (
    <div className="flex flex-col gap-(--space-2)">
      <span className="text-body font-medium text-(--text)">{DESCRIPTION_LABEL}</span>
      <div className="rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2)">
        <React.Suspense fallback={<div aria-hidden="true" className="min-h-24" />}>
          <IssueDescriptionEditor
            key={`${projectId}:${assetScope.entityType}:${assetScope.entityId}:${generation}:${remount}`}
            initialHtml={value}
            onChange={onChange}
            placeholder="Add a description…"
            label={DESCRIPTION_LABEL}
            onReady={handleEditorReady}
            onMentionQuery={setMentionQuery}
            {...(tabIndex === undefined ? {} : { tabIndex })}
            resolveAsset={resolveAsset}
          />
        </React.Suspense>
      </div>
      {mentionQuery === null ? null : <MentionList query={mentionQuery} results={mentions.data} onPick={pickMention} />}
      <div className="flex flex-wrap items-center justify-end gap-(--space-2)">
        {assistantConfigured && title.trim() !== "" ? (
          <Button
            variant="ghost"
            size="small"
            disabled={busy}
            onClick={() => {
              void describeFromTitle();
            }}
          >
            <Sparkle size={14} strokeWidth={1.5} aria-hidden="true" />
            {describe.isPending ? DESCRIBE_BUSY_LABEL : DESCRIBE_BUTTON_LABEL}
          </Button>
        ) : null}
        <input
          ref={fileRef}
          type="file"
          accept="image/*"
          aria-label="Attach an image to the description"
          className="sr-only"
          disabled={busy}
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (file !== undefined) void uploadImage(file);
          }}
        />
        <Button variant="ghost" size="small" disabled={busy} onClick={() => fileRef.current?.click()}>
          Add image
        </Button>
        {assistantConfigured ? (
          <Popover
            open={aiOpen}
            onOpenChange={(open) => {
              if (open) setAiOpen(true);
              else closeAi();
            }}
            trigger={
              <Button variant="ghost" size="small" disabled={busy}>
                <Sparkle size={14} strokeWidth={1.5} aria-hidden="true" />
                {AI_BUTTON_LABEL}
              </Button>
            }
            title="Assistant writing"
          >
            <div data-testid="modal-ai-panel" className="flex w-72 flex-col gap-(--space-2)">
              {aiAnswer !== null && aiAnswer.html !== "" ? (
                <div className="flex flex-col gap-(--space-1)">
                  <p className="text-meta font-medium text-(--text)">{AI_RESPONSE_LABEL}</p>
                  <div
                    data-testid="modal-ai-response"
                    className="text-body max-h-40 overflow-y-auto rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2) text-(--text)"
                    dangerouslySetInnerHTML={{ __html: aiAnswer.html }}
                  />
                </div>
              ) : null}
              {aiInvalid ? (
                <p data-testid="modal-ai-invalid" role="alert" className="text-meta text-(--danger)">
                  {EDITOR_EMPTY_ANSWER}
                </p>
              ) : null}
              <label className="text-meta text-(--text-muted)" htmlFor="modal-ai-prompt">
                {AI_PROMPT_LABEL}
              </label>
              <textarea
                id="modal-ai-prompt"
                value={aiPrompt}
                onChange={(event) => setAiPrompt(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.stopPropagation();
                    closeAi();
                    return;
                  }
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    void generateAnswer();
                  }
                }}
                className="text-body min-h-20 w-full rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2) text-(--text)"
              />
              <div className="flex justify-end gap-(--space-2)">
                {aiAnswer !== null && aiAnswer.html !== "" ? (
                  <Button
                    variant="primary"
                    size="small"
                    onClick={() => {
                      useAnswer(aiAnswer);
                    }}
                  >
                    {AI_USE_RESPONSE_LABEL}
                  </Button>
                ) : null}
                <Button
                  variant="secondary"
                  size="small"
                  onClick={() => {
                    closeAi();
                  }}
                >
                  {AI_CLOSE_LABEL}
                </Button>
                <Button
                  variant="primary"
                  size="small"
                  disabled={answerMutation.isPending}
                  onClick={() => {
                    void generateAnswer();
                  }}
                >
                  {answerMutation.isPending
                    ? AI_GENERATING_LABEL
                    : aiAnswer !== null
                      ? AI_REGENERATE_LABEL
                      : AI_GENERATE_LABEL}
                </Button>
              </div>
            </div>
          </Popover>
        ) : null}
      </div>
    </div>
  );
}

/** The @ popup: people the lookup returned, or a quiet empty note. */
function MentionList({
  query,
  results,
  onPick,
}: {
  query: string;
  results: MentionSearchResults | undefined;
  onPick: (id: string, name: string) => void;
}): React.ReactElement | null {
  const people = results?.people ?? [];
  if (query !== "" && people.length === 0) return null;
  return (
    <ul
      aria-label="Mention suggestions"
      className={cn(
        "flex flex-col gap-px rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)",
        "shadow-popover"
      )}
    >
      {people.map((person) => (
        <li key={person.id}>
          <button
            type="button"
            onMouseDown={(event) => {
              event.preventDefault();
              onPick(person.id, person.name);
            }}
            className="text-body w-full rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
          >
            {person.name}
          </button>
        </li>
      ))}
    </ul>
  );
}
