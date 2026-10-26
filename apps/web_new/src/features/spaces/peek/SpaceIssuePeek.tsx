// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public issue overlay (parity/specs/space.md §3.7, SPC-040–046). The
// shared peek panel supplies the presentation shell, focus guard and
// dismissal paths; this layer owns the public data: one detail read
// through the public runtime, the read-only key/title/description block,
// the engagement section below the title, the three-field property panel,
// the clipboard-gated copy action and the loading/error branches. The
// activity section belongs to a later slice and is deliberately absent.
//
// On load the address's `peekId` is the source of truth; a malformed id
// or a 404/403 is reported through onInvalid so the board can drop the
// parameter instead of crashing. Closing is the host's address rewrite.

import { isApiError, spaceAssetReadPath } from "@pidash/api-client";
import type { SpaceBoardIssue, SpaceBoardIssueDetail, SpaceBoardState } from "@pidash/api-client";
import { Button, Skeleton } from "@pidash/kit";
import { Link2 } from "lucide-react";
import * as React from "react";

import { useQuery } from "@tanstack/react-query";

import { RichTextView, hasStoredDescriptionContent } from "../../../shared/editor/index.js";
import { PeekPanel } from "../../../shared/shell/PeekPanel.js";
import { spaceRuntime } from "../api/client.js";
import { spaceIssueDetailQueryOptions, spaceViewerQueryOptions } from "../api/queries.js";
import type { SpaceBoardSearch } from "../lib/address.js";
import { useClipboardWritePermission } from "../lib/clipboard.js";
import { isSpaceIssueId } from "../lib/peek.js";
import { notifySpaceCopyFailure, notifySpaceCopySuccess } from "../lib/spaceNotices.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { issueKey } from "../components/BoardShell.js";
import { SpaceIssueEngagement } from "../engagement/SpaceIssueEngagement.js";
import { SpaceCommentsSection } from "../comments/SpaceCommentsSection.js";
import { SpaceOverlayProperties } from "./SpaceOverlayProperties.js";
import { SpacePeekModeSwitch, type SpacePeekPresentation } from "./SpacePeekModeSwitch.js";

export interface SpaceIssuePeekProps {
  anchor: string;
  issueId: string;
  projectIdentifier: string;
  /** Undefined until the board's states fetch settles (SPC-046). */
  states: readonly SpaceBoardState[] | undefined;
  /**
   * The cached list row for this issue, when the board had one. It seeds
   * only the header's key and title while detail resolves (SPC-046).
   */
  seed?: Pick<SpaceBoardIssue, "name" | "sequence_id"> | undefined;
  /** Close the overlay: the host drops peekId and keeps every other key. */
  onClose: () => void;
  /** The address names an issue this board cannot show; drop the key. */
  onInvalid: () => void;
  /** Embedded hosts lose both Escape and outside-click dismissal. */
  embedded?: boolean | undefined;
  /** First presentation; the side dock unless the host says otherwise. */
  initialPresentation?: SpacePeekPresentation | undefined;
  /** Publisher engagement flags; each half renders only while its flag is on. */
  votesEnabled?: boolean | undefined;
  reactionsEnabled?: boolean | undefined;
  /** Board position the anonymous sign-in redirect carries back to. */
  search?: SpaceBoardSearch | undefined;
  /** Anonymous activations leave here; without one the page navigates. */
  onRequireSignIn?: (() => void) | undefined;
  /** Publisher flag for the comments section (SPC-049). */
  commentsEnabled?: boolean | undefined;
}

/** Status code of a detail refusal that means "not this board's issue". */
export function isGoneError(error: unknown): boolean {
  if (!isApiError(error)) return false;
  return error.status === 403 || error.status === 404;
}

/** Return focus to the control that opened the overlay (SPC-041/042). */
function useRestoreOrigin(): void {
  const originRef = React.useRef<HTMLElement | null | undefined>(undefined);
  if (originRef.current === undefined) {
    const active = document.activeElement;
    originRef.current = active instanceof HTMLElement ? active : null;
  }
  React.useEffect(
    () => () => {
      const origin = originRef.current;
      if (origin !== null && origin !== undefined && origin.isConnected) {
        origin.focus({ preventScroll: true });
      }
    },
    []
  );
}

/** Resolve a stored image asset id to the public anchor asset route. */
function usePublicAssetResolver(anchor: string, baseUrl: string): (src: string) => string {
  return React.useCallback(
    (src: string) => {
      const path = spaceAssetReadPath(anchor, src);
      if (baseUrl === "") return path;
      return `${baseUrl.replace(/\/+$/, "")}${path}`;
    },
    [anchor, baseUrl]
  );
}

export function SpaceIssuePeek({
  anchor,
  issueId,
  projectIdentifier,
  states,
  seed,
  onClose,
  onInvalid,
  embedded = false,
  initialPresentation = "side",
  votesEnabled = false,
  reactionsEnabled = false,
  search = {},
  onRequireSignIn,
  commentsEnabled = false,
}: SpaceIssuePeekProps): React.ReactElement {
  const [presentation, setPresentation] = React.useState<SpacePeekPresentation>(initialPresentation);
  const client = spaceRuntime.client;
  const clipboardAllowed = useClipboardWritePermission();
  const detail = useQuery(spaceIssueDetailQueryOptions(client, anchor, issueId));
  const viewer = useQuery(spaceViewerQueryOptions(client));
  const resolveAsset = usePublicAssetResolver(anchor, client.baseUrl);
  useRestoreOrigin();

  // A malformed id never becomes a request, and a refusal means the issue
  // is gone or private: both drop peekId and leave the board standing.
  const invalid = !isSpaceIssueId(issueId) || (detail.isError && isGoneError(detail.error));
  React.useEffect(() => {
    if (invalid) onInvalid();
  }, [invalid, onInvalid]);

  const issue: SpaceBoardIssueDetail | undefined = detail.data;
  // Until the detail lands the header still carries an identity: the
  // cached row's key when the board had one, and a placeholder otherwise
  // (SPC-046) — never an empty header with only the controls.
  const headerIssue = issue ?? seed;
  const key = headerIssue === undefined ? "" : issueKey(projectIdentifier, headerIssue);
  const title = headerIssue?.name ?? "";
  const copyLink = React.useCallback((): void => {
    const writeText = globalThis.navigator?.clipboard?.writeText;
    if (typeof writeText !== "function" || typeof globalThis.window === "undefined") {
      notifySpaceCopyFailure();
      return;
    }
    writeText
      .call(globalThis.navigator.clipboard, globalThis.window.location.href)
      .then(() => notifySpaceCopySuccess())
      .catch(() => notifySpaceCopyFailure());
  }, []);

  // Copy needs a known address, so it waits for the detail to resolve
  // instead of sitting over the skeleton (SPC-046).
  const copyOffered = clipboardAllowed && presentation !== "full" && issue !== undefined;

  return (
    <PeekPanel
      layout={presentation}
      role="dialog"
      label={issue === undefined ? spaceMessage("peek.loading") : key}
      testId="space-peek"
      eyebrow={key === "" ? null : <span data-testid="space-peek-key">{key}</span>}
      title={title}
      titleLabel={spaceMessage("peek.titleLabel")}
      titleAsHeading
      hideTitle={headerIssue === undefined && detail.isError}
      titleSkeleton={headerIssue === undefined && !detail.isError}
      onClose={onClose}
      suppressEscape={embedded}
      trapFocus={presentation !== "side"}
      scrollBody
      {...(embedded ? {} : { onOutsideClick: onClose })}
      actions={
        <>
          <SpacePeekModeSwitch value={presentation} onChange={setPresentation} />
          {copyOffered ? (
            <button
              type="button"
              aria-label={spaceMessage("peek.copyLink")}
              data-testid="space-peek-copy"
              onClick={copyLink}
              className="text-meta inline-flex size-6 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
            >
              <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />
            </button>
          ) : null}
        </>
      }
    >
      {issue === undefined ? (
        detail.isError ? (
          <div data-testid="space-peek-error" className="flex flex-col items-start gap-(--space-3)">
            <p className="text-body text-(--text)">{spaceMessage("peek.detailFailure")}</p>
            <Button
              variant="secondary"
              size="small"
              onClick={() => {
                void detail.refetch();
              }}
            >
              {spaceMessage("peek.retry")}
            </Button>
          </div>
        ) : (
          <SpacePeekSkeleton full={presentation === "full"} />
        )
      ) : (
        <SpaceIssuePeekBody
          anchor={anchor}
          search={search}
          issue={issue}
          states={states}
          full={presentation === "full"}
          resolveAsset={resolveAsset}
          votesEnabled={votesEnabled}
          reactionsEnabled={reactionsEnabled}
          embedded={embedded}
          onRequireSignIn={onRequireSignIn}
          comments={{ anchor, issueId, search, enabled: commentsEnabled, embedded, viewerId: viewer.data?.id ?? null }}
        />
      )}
    </PeekPanel>
  );
}

/** The detail and property columns; full gives properties their own lane. */
function SpaceIssuePeekBody({
  anchor,
  search,
  issue,
  states,
  full,
  resolveAsset,
  votesEnabled,
  reactionsEnabled,
  embedded,
  onRequireSignIn,
  comments,
}: {
  anchor: string;
  search: SpaceBoardSearch | undefined;
  issue: SpaceBoardIssueDetail;
  states: readonly SpaceBoardState[] | undefined;
  full: boolean;
  resolveAsset: (src: string) => string;
  votesEnabled: boolean;
  reactionsEnabled: boolean;
  embedded: boolean;
  onRequireSignIn: (() => void) | undefined;
  comments: {
    anchor: string;
    issueId: string;
    search: SpaceBoardSearch;
    enabled: boolean;
    embedded: boolean;
    viewerId: string | null;
  };
}): React.ReactElement {
  const hasDescription = hasStoredDescriptionContent(issue.description_html);
  return (
    <div
      className={full ? "flex min-h-0 flex-1 flex-col gap-(--space-6) md:flex-row" : "flex flex-col gap-(--space-6)"}
    >
      <div data-testid="space-peek-detail" className="flex min-w-0 flex-1 flex-col gap-(--space-4)">
        <SpaceIssueEngagement
          anchor={anchor}
          search={search ?? {}}
          issue={issue}
          votesEnabled={votesEnabled}
          reactionsEnabled={reactionsEnabled}
          embedded={embedded}
          onRequireSignIn={onRequireSignIn}
        />
        {hasDescription ? (
          <RichTextView
            html={issue.description_html}
            label={spaceMessage("peek.descriptionLabel")}
            resolveAsset={resolveAsset}
          />
        ) : null}
        {full ? null : <SpaceOverlayProperties issue={issue} states={states} />}
      </div>
      {full ? (
        <aside
          data-testid="space-peek-property-column"
          className="flex min-w-0 flex-col gap-(--space-4) md:w-80 md:shrink-0"
        >
          <SpaceOverlayProperties issue={issue} states={states} />
        </aside>
      ) : null}
      <SpaceCommentsSection
        anchor={comments.anchor}
        issueId={comments.issueId}
        search={comments.search}
        enabled={comments.enabled}
        viewerId={comments.viewerId}
        embedded={comments.embedded}
        resolveAsset={resolveAsset}
      />
    </div>
  );
}

/** Skeleton placeholder that holds the overlay's shape while data loads. */
export function SpacePeekSkeleton({ full = false }: { full?: boolean }): React.ReactElement {
  return (
    <div
      data-testid="space-peek-skeleton"
      role="status"
      aria-label={spaceMessage("peek.loading")}
      className={full ? "flex flex-col gap-(--space-6) md:flex-row" : "flex flex-col gap-(--space-4)"}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-3)">
        <Skeleton width="70%" height="20px" />
        <Skeleton width="100%" height="12px" />
        <Skeleton width="92%" height="12px" />
        <Skeleton width="74%" height="12px" />
      </div>
      <div className="flex flex-col gap-(--space-2)">
        <Skeleton width="12rem" height="14px" />
        <Skeleton width="10rem" height="14px" />
        <Skeleton width="11rem" height="14px" />
      </div>
    </div>
  );
}
