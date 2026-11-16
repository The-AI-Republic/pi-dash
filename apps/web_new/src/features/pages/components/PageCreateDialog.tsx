// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page creation dialog (parity/specs/pages.md §3.6 and §11.3, PAGE-008).
// A second path to a new page beside the header's quick action: an optional
// mark, a required title with an inline block past the cap, and an open or
// restricted choice. Cancel and Escape discard the draft without a request;
// a valid submit closes the dialog and hands the new id back so the caller
// can decide whether to go to the page. A failed write keeps the dialog and
// everything typed into it.

import { PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC } from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";
import { Globe2, Lock } from "lucide-react";
import * as React from "react";

import { useCreatePage } from "../api/mutations.js";
import {
  pageCreateInput,
  pageDraftSubmittable,
  pageMarkFromDraft,
  pageTitleTooLong,
  PAGE_TITLE_TOO_LONG_MESSAGE,
} from "../lib/pageCreate.js";
import { PageMarkPicker } from "./PageMarkPicker.js";

export const PAGE_CREATE_DIALOG_TITLE = "Create page";
export const PAGE_CREATE_SUBMIT_LABEL = "Create Page";

const ACCESS_CHOICES: ReadonlyArray<{ access: number; label: string; Icon: typeof Globe2 }> = [
  { access: PAGE_ACCESS_PUBLIC, label: "Public", Icon: Globe2 },
  { access: PAGE_ACCESS_PRIVATE, label: "Private", Icon: Lock },
];

/** The word the current choice shows; the contract carries only these two. */
function accessLabel(access: number): string {
  return access === PAGE_ACCESS_PRIVATE ? "Private" : "Public";
}

export interface PageCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  /** Visibility the dialog starts on; the active tab supplies it. */
  initialAccess?: number;
  /** Runs with the new page's id after the record exists. */
  onCreated?: (pageId: string) => void;
}

interface Draft {
  title: string;
  access: number;
  mark: Record<string, unknown>;
}

function draftFor(initialAccess: number): Draft {
  return { title: "", access: initialAccess, mark: {} };
}

export function PageCreateDialog({
  open,
  onOpenChange,
  workspaceSlug,
  projectId,
  initialAccess = PAGE_ACCESS_PUBLIC,
  onCreated,
}: PageCreateDialogProps): React.ReactElement {
  const [draft, setDraft] = React.useState<Draft>(() => draftFor(initialAccess));
  const create = useCreatePage(workspaceSlug, projectId);
  const titleRef = React.useRef<HTMLInputElement | null>(null);
  const reset = create.reset;
  const pending = create.isPending;

  // Every open starts from a clean draft and a cleared failure, so a
  // cancelled or failed visit never leaks into the next one.
  React.useEffect(() => {
    if (!open) return;
    setDraft(draftFor(initialAccess));
    reset();
  }, [open, initialAccess, reset]);

  const close = (next: boolean): void => {
    if (!next && pending) return;
    setDraft(draftFor(initialAccess));
    onOpenChange(next);
  };

  const overLength = pageTitleTooLong(draft.title);
  const submittable = pageDraftSubmittable(draft.title) && !pending;

  const submit = (): void => {
    if (!pageDraftSubmittable(draft.title) || pending) return;
    const mark = pageMarkFromDraft(draft.mark);
    create.mutate(
      { access: draft.access, input: pageCreateInput(draft.title, draft.access, mark) },
      {
        onSuccess: (created) => {
          close(false);
          onCreated?.(created.id);
        },
      }
    );
  };

  return (
    <Dialog
      title={PAGE_CREATE_DIALOG_TITLE}
      open={open}
      onOpenChange={(next) => {
        close(next);
      }}
      className="max-w-lg"
      bodyClassName="mt-(--space-6)"
    >
      <form
        className="flex flex-col gap-(--space-6)"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <div className="flex items-start gap-(--space-3)">
          <PageMarkPicker draft={draft.mark} onDraftChange={(mark) => setDraft((current) => ({ ...current, mark }))} />
          <div className="flex min-w-0 flex-1 flex-col gap-(--space-2)">
            <Input
              label="Title"
              hideLabel
              inputRef={titleRef}
              value={draft.title}
              placeholder="Title"
              autoFocus
              required
              aria-invalid={overLength || undefined}
              {...(overLength ? { error: PAGE_TITLE_TOO_LONG_MESSAGE } : {})}
              onChange={(event) => setDraft((current) => ({ ...current, title: event.target.value }))}
            />
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-(--space-2)" data-testid="page-create-access">
          <span className="text-meta text-(--text-muted)">Who can see it</span>
          <div role="group" aria-label="Visibility" className="flex items-center gap-(--space-1)">
            {ACCESS_CHOICES.map(({ access, label, Icon }) => (
              <button
                key={access}
                type="button"
                aria-pressed={draft.access === access}
                className={`text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-control) border px-(--space-2) py-(--space-1) ${
                  draft.access === access
                    ? "border-(--accent) bg-(--subtle) text-(--text)"
                    : "border-(--border) text-(--text-muted) hover:bg-(--subtle)"
                }`}
                onClick={() => setDraft((current) => ({ ...current, access }))}
              >
                <Icon size={14} strokeWidth={1.5} aria-hidden="true" />
                {label}
              </button>
            ))}
          </div>
          <span className="text-meta text-(--text-muted)" data-testid="page-create-access-label">
            {accessLabel(draft.access)}
          </span>
        </div>

        <div className="flex items-center justify-end gap-(--space-2)">
          <Button
            variant="secondary"
            onClick={() => {
              close(false);
            }}
          >
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={pending} disabled={!submittable}>
            {PAGE_CREATE_SUBMIT_LABEL}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
