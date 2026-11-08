// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop About dialog (DESK-028). The sidebar's account area hosts an
// About button on the desktop bundle; it opens a dialog naming the running
// version and printing every licence and notice text the shell bundles.
// This is where the app meets the notice obligations recorded in
// desktop/src-tauri/licenses/README.md, so a notice the build did not stage
// is listed as missing rather than skipped. The web bundle has no About
// capability, and the control renders nothing there.

import { Info } from "lucide-react";
import { Button, Dialog, IconButton, Spinner, Tooltip } from "@pidash/kit";
import * as React from "react";

import { platform, type AboutApi, type AboutInfo, type BundledNotice } from "../../core/platform/index.js";

/** Accessible name of the sidebar button, and the dialog title. */
export const ABOUT_CONTROL_LABEL = "About Pi Dash";

/** Retry action shown after the texts failed to load. */
export const ABOUT_RETRY_LABEL = "Try again";

/**
 * The licensing summary above the texts. The agent engine is not bundled
 * (PIDESKAPP-226): the command-line tool installs it with its own licence
 * and notice, so the dialog says where those live instead of showing them.
 */
export const ABOUT_LICENSING_SUMMARY =
  "Pi Dash is free software under the GNU Affero General Public License, version 3 only. " +
  "The agent engine is not part of this app: the Pi Dash command-line tool installs it " +
  "together with its own licence and notice. The full licence texts bundled with this app follow.";

export function aboutVersionText(version: string): string {
  return version.length > 0 ? `Version ${version}` : "Version unknown";
}

export function noticeHeading(notice: BundledNotice): string {
  return notice.license.length > 0 ? `${notice.component} — ${notice.license}` : notice.component;
}

export function noticeMissingText(file: string): string {
  return `${file} is not included in this build.`;
}

export function aboutLoadFailureText(error: unknown): string {
  const reason = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const trimmed = reason.trim();
  return trimmed.length > 0
    ? `The licence texts could not be loaded: ${trimmed}`
    : "The licence texts could not be loaded.";
}

export type AboutLoadState =
  | { status: "loading" }
  | { status: "ready"; info: AboutInfo }
  | { status: "failed"; message: string };

export interface AboutDialogViewProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  state: AboutLoadState;
  onRetry: () => void;
}

/** Presentational dialog; every load outcome arrives through `state`. */
export function AboutDialogView({ open, onOpenChange, state, onRetry }: AboutDialogViewProps): React.ReactElement {
  return (
    <Dialog title={ABOUT_CONTROL_LABEL} open={open} onOpenChange={(next) => onOpenChange(next)}>
      <div data-testid="desktop-about-dialog" className="flex flex-col gap-(--space-4)">
        {state.status === "ready" ? (
          <p className="text-meta text-(--text-muted)">{aboutVersionText(state.info.version)}</p>
        ) : null}
        <p className="text-body">{ABOUT_LICENSING_SUMMARY}</p>
        {state.status === "loading" ? (
          <p className="text-body flex items-center gap-(--space-2) text-(--text-muted)">
            <Spinner size="small" label="Loading licence texts" />
            Loading licence texts…
          </p>
        ) : null}
        {state.status === "failed" ? (
          <div role="alert" className="flex flex-col items-start gap-(--space-2)">
            <span className="text-body text-(--danger)">{state.message}</span>
            <Button variant="secondary" size="small" onClick={onRetry}>
              {ABOUT_RETRY_LABEL}
            </Button>
          </div>
        ) : null}
        {state.status === "ready"
          ? state.info.notices.map((notice) => <NoticeSection key={notice.file} notice={notice} />)
          : null}
      </div>
    </Dialog>
  );
}

function NoticeSection({ notice }: { notice: BundledNotice }): React.ReactElement {
  const headingId = React.useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-(--space-2)">
      <h3 id={headingId} className="text-body font-medium">
        {noticeHeading(notice)}
      </h3>
      {notice.text === null ? (
        <p className="text-meta text-(--text-muted)">{noticeMissingText(notice.file)}</p>
      ) : (
        // Licence texts are plain text: keep their line breaks, never parse them.
        <pre
          tabIndex={0}
          className="text-caption max-h-48 overflow-auto rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-3) whitespace-pre-wrap"
        >
          {notice.text}
        </pre>
      )}
    </section>
  );
}

/**
 * Load the About payload each time the dialog opens, so a reopened dialog
 * never shows a stale failure. A load that settles after a newer one
 * started (or after unmount) is ignored.
 */
export function useAboutLoader(about: AboutApi | undefined): {
  state: AboutLoadState;
  load: () => void;
} {
  const [state, setState] = React.useState<AboutLoadState>({ status: "loading" });
  const attempt = React.useRef(0);
  React.useEffect(
    () => () => {
      attempt.current += 1;
    },
    []
  );
  const load = React.useCallback(() => {
    if (!about) return;
    attempt.current += 1;
    const mine = attempt.current;
    setState({ status: "loading" });
    about.load().then(
      (info) => {
        if (attempt.current === mine) setState({ status: "ready", info });
      },
      (error: unknown) => {
        if (attempt.current === mine) setState({ status: "failed", message: aboutLoadFailureText(error) });
      }
    );
  }, [about]);
  return { state, load };
}

export interface DesktopAboutControlViewProps {
  about: AboutApi | undefined;
}

/** Sidebar button plus dialog, driven by an explicit capability (tests). */
export function DesktopAboutControlView({ about }: DesktopAboutControlViewProps): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  const { state, load } = useAboutLoader(about);
  if (!about) return null;
  return (
    <>
      <Tooltip content={ABOUT_CONTROL_LABEL}>
        <IconButton
          label={ABOUT_CONTROL_LABEL}
          size="small"
          data-testid="desktop-about-control"
          onClick={() => {
            setOpen(true);
            load();
          }}
        >
          <Info size={14} strokeWidth={1.75} aria-hidden="true" />
        </IconButton>
      </Tooltip>
      {open ? <AboutDialogView open={open} onOpenChange={setOpen} state={state} onRetry={load} /> : null}
    </>
  );
}

/** The control mounted by the workspace shell; wired to the desktop platform. */
export function DesktopAboutControl(): React.ReactElement | null {
  return <DesktopAboutControlView about={platform.about} />;
}
