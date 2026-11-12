// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Product-updates dialog (SHELL-060, from parity/specs/shell.md §4).
// The running version heads the dialog; the changelog embeds the
// instance's configured changelog page, falling back to a
// visit-the-changelog prompt when unconfigured, slow or broken. Docs,
// full-changelog and support links foot the dialog. Escape never
// dismisses: the close path is the overlay click or the close button.
import { Badge, Button, Dialog, Spinner } from "@pidash/kit";
import * as React from "react";

// Lazy-only catalog import: this dialog loads through React.lazy, so the
// catalog never joins a route's eager closure (which would collapse its
// TanStack component split). Keep this file lazily loaded (NEWFRONT-287).
import { DOCS_URL } from "./entries/account.js";

/** The public changelog the fallback and footer link to. */
export const CHANGELOG_URL = "https://airepublic.com/changelog";

/** The product support contact behind the footer support link. */
export const SUPPORT_EMAIL_URL = "mailto:support@airepublic.com";

/** How long the embedded changelog may load before the fallback takes over. */
export const CHANGELOG_LOAD_TIMEOUT_MS = 15_000;

/** The edition-flavoured changelog address the fallback opens. */
export function changelogCategoryUrl(selfManaged: boolean): string {
  return `${CHANGELOG_URL}?category=${selfManaged ? "self-hosted" : "cloud"}`;
}

function ChangelogFrame({ url, fallback }: { url: string; fallback: React.ReactNode }): React.ReactElement {
  const [loaded, setLoaded] = React.useState(false);
  const [failed, setFailed] = React.useState(false);
  const frameRef = React.useRef<HTMLIFrameElement | null>(null);
  const settled = React.useRef(false);

  React.useEffect(() => {
    // Native listeners: React does not bind synthetic load/error
    // handlers on iframes, so the embed settles through the node.
    const node = frameRef.current!;
    settled.current = false;
    setLoaded(false);
    setFailed(false);
    const timer = setTimeout(() => {
      if (!settled.current) setFailed(true);
    }, CHANGELOG_LOAD_TIMEOUT_MS);
    const onLoad = (): void => {
      settled.current = true;
      setLoaded(true);
    };
    const onError = (): void => {
      settled.current = true;
      setFailed(true);
    };
    node.addEventListener("load", onLoad);
    node.addEventListener("error", onError);
    return () => {
      clearTimeout(timer);
      node.removeEventListener("load", onLoad);
      node.removeEventListener("error", onError);
    };
  }, [url]);

  if (failed) return <>{fallback}</>;
  return (
    <div className="relative min-h-64">
      {loaded ? null : (
        <p className="text-body absolute inset-0 flex items-center justify-center gap-(--space-2) text-(--text-muted)">
          <Spinner size="small" label="Loading updates" />
          Loading updates…
        </p>
      )}
      <iframe
        ref={frameRef}
        src={url}
        title="Product changelog"
        className="h-96 w-full rounded-(--radius-control) border border-(--border)"
      />
    </div>
  );
}

export interface UpdatesDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  version: string;
  /**
   * The instance's configured changelog page. Empty or null renders the
   * visit-the-changelog fallback instead of the embed.
   */
  changelogUrl: string | null;
  /** True on self-managed instances: picks the fallback changelog address. */
  selfManaged: boolean;
  /** Opens a URL outside the app (system browser on desktop). */
  onOpenExternal: (url: string) => void;
}

export function UpdatesDialog({
  open,
  onOpenChange,
  version,
  changelogUrl,
  selfManaged,
  onOpenExternal,
}: UpdatesDialogProps): React.ReactElement {
  const effectiveUrl = changelogUrl === null || changelogUrl === "" ? null : changelogUrl;
  const fallback = (
    <div className="flex flex-col items-start gap-(--space-4) py-(--space-4)">
      <p className="text-body text-(--text-muted)">
        The updates feed did not load. Read the latest changes in the changelog.
      </p>
      <Button variant="primary" onClick={() => onOpenExternal(changelogCategoryUrl(selfManaged))}>
        Go to changelog
      </Button>
    </div>
  );
  return (
    <Dialog title="What's new?" open={open} onOpenChange={onOpenChange} ignoreEscapeKey>
      <div className="flex flex-col gap-(--space-4)">
        <p>
          <Badge tone="accent">Version: v{version}</Badge>
        </p>
        {effectiveUrl === null ? fallback : <ChangelogFrame url={effectiveUrl} fallback={fallback} />}
        <div className="flex items-center gap-(--space-4) border-t border-(--border) pt-(--space-4)">
          <button
            type="button"
            onClick={() => onOpenExternal(DOCS_URL)}
            className="text-body cursor-pointer text-(--text-muted) underline-offset-2 hover:text-(--text) hover:underline"
          >
            Docs
          </button>
          <button
            type="button"
            onClick={() => onOpenExternal(CHANGELOG_URL)}
            className="text-body cursor-pointer text-(--text-muted) underline-offset-2 hover:text-(--text) hover:underline"
          >
            Full changelog
          </button>
          <button
            type="button"
            onClick={() => onOpenExternal(SUPPORT_EMAIL_URL)}
            className="text-body cursor-pointer text-(--text-muted) underline-offset-2 hover:text-(--text) hover:underline"
          >
            Support
          </button>
        </div>
      </div>
    </Dialog>
  );
}
