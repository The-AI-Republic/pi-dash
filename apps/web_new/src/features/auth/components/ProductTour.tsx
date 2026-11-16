// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-run product tour overlay (AUTH-042). The welcome stop (concept
// video, personalized greeting, take-the-tour plus skip-and-explore) and
// the five content stops, each with an illustration, Back/Next stepping,
// a stop navigator, and an always-available close; the last stop ends in
// the project-creation action. Pure and props-driven: the home seam
// (WorkspaceHomeTour) gates it on the profile flag and wires completion.
// Written from parity/specs/auth.md.

import { Button, IconButton } from "@pidash/kit";
import { Bot, FileText, ListTodo, MessagesSquare, StickyNote, X } from "lucide-react";
import * as React from "react";

import {
  nextTourStep,
  previousTourStep,
  TOUR_CONTENT_STOPS,
  TOUR_VIDEO_EMBED_URL,
  tourContentStop,
  tourWelcomeGreeting,
  type TourStepKey,
} from "../tour.js";

export const TOUR_TAKE_TOUR_LABEL = "Take the tour";
export const TOUR_SKIP_LABEL = "Explore on my own";
export const TOUR_CLOSE_LABEL = "Close the tour";
export const TOUR_START_PROJECT_LABEL = "Start your first project";

const STOP_ICONS = {
  "work-items": ListTodo,
  runners: Bot,
  chat: MessagesSquare,
  workpads: StickyNote,
  pages: FileText,
} as const;

function StopArt({ stopKey }: { stopKey: keyof typeof STOP_ICONS }): React.ReactElement {
  const StopIcon = STOP_ICONS[stopKey];
  return <StopIcon size={64} strokeWidth={1} className="text-(--text-muted)" />;
}

export interface ProductTourProps {
  /** The user's first name for the welcome greeting; empty greets plainly. */
  firstName: string;
  /** Completes or skips the tour: the seam flips the profile flag. */
  onComplete: () => void;
  /**
   * Opens the create-project dialog. Owned by the projects area and wired
   * through the home seam; until it lands, finishing only completes the
   * tour.
   */
  onCreateProject?: (() => void) | undefined;
}

export function ProductTour({ firstName, onComplete, onCreateProject }: ProductTourProps): React.ReactElement {
  const [step, setStep] = React.useState<TourStepKey>("welcome");
  const stop = tourContentStop(step);
  const previous = previousTourStep(step);
  const next = nextTourStep(step);
  const last = stop !== null && next === null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="product-tour-heading"
      className="fixed inset-0 z-50 grid place-items-center overflow-y-auto bg-black/40 p-(--space-6)"
    >
      {stop === null ? (
        <section
          aria-labelledby="product-tour-heading"
          className="w-full max-w-lg overflow-hidden rounded-(--radius-dialog) border border-(--border) bg-(--bg)"
        >
          <div className="aspect-video w-full bg-(--subtle)">
            <iframe
              src={TOUR_VIDEO_EMBED_URL}
              title="Pi Dash core concept video"
              className="h-full w-full border-0"
              allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
              sandbox="allow-scripts allow-same-origin allow-presentation"
              allowFullScreen
            />
          </div>
          <div className="flex flex-col gap-(--space-4) p-(--space-8)">
            <h2 id="product-tour-heading" className="text-h3 text-(--text)">
              {tourWelcomeGreeting(firstName)}
            </h2>
            <p className="text-body text-(--text-muted)">
              See how work flows here: describe it, dispatch it to an agent, and stay in the loop until it lands. Watch
              the concept video, then walk the short tour — or dive straight in.
            </p>
            <div className="flex items-center gap-(--space-4)">
              <Button type="button" variant="primary" size="large" onClick={() => setStep("work-items")}>
                {TOUR_TAKE_TOUR_LABEL}
              </Button>
              <Button type="button" variant="ghost" size="large" onClick={onComplete}>
                {TOUR_SKIP_LABEL}
              </Button>
            </div>
          </div>
        </section>
      ) : (
        <section
          aria-labelledby="product-tour-heading"
          className="relative flex w-full max-w-3xl overflow-hidden rounded-(--radius-dialog) border border-(--border) bg-(--bg)"
        >
          <div className="absolute top-(--space-4) right-(--space-4)">
            <IconButton label={TOUR_CLOSE_LABEL} onClick={onComplete}>
              <X size={16} aria-hidden="true" />
            </IconButton>
          </div>
          <nav
            aria-label="Tour stops"
            className="hidden w-56 shrink-0 flex-col gap-(--space-6) bg-(--subtle) p-(--space-6) lg:flex"
          >
            <h2 className="text-emphasis text-(--text)">What you will see</h2>
            <ol className="flex flex-col gap-(--space-1)">
              {TOUR_CONTENT_STOPS.map((entry) => {
                const EntryIcon = STOP_ICONS[entry.key];
                const current = entry.key === stop.key;
                return (
                  <li key={entry.key}>
                    <button
                      type="button"
                      {...(current ? { "aria-current": "step" } : {})}
                      onClick={() => setStep(entry.key)}
                      className={`text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left ${current ? "bg-(--surface) font-medium text-(--text)" : "text-(--text-muted)"}`}
                    >
                      <EntryIcon size={16} aria-hidden="true" />
                      {entry.navLabel}
                    </button>
                  </li>
                );
              })}
            </ol>
          </nav>
          <div className="flex min-w-0 flex-1 flex-col">
            <div aria-hidden="true" className="grid h-44 place-items-center bg-(--subtle)">
              <StopArt stopKey={stop.key} />
            </div>
            <div className="flex flex-col gap-(--space-4) p-(--space-8)">
              <h2 id="product-tour-heading" className="text-h3 text-(--text)">
                {stop.title}
              </h2>
              <p className="text-body text-(--text-muted)">{stop.body}</p>
              <div className="flex items-center justify-between gap-(--space-4)">
                <div className="flex items-center gap-(--space-2)">
                  {previous === null ? null : (
                    <Button type="button" variant="secondary" onClick={() => setStep(previous)}>
                      Back
                    </Button>
                  )}
                  {next === null ? null : (
                    <Button type="button" variant="primary" onClick={() => setStep(next)}>
                      Next
                    </Button>
                  )}
                </div>
                {last ? (
                  <Button
                    type="button"
                    variant="primary"
                    onClick={() => {
                      onComplete();
                      onCreateProject?.();
                    }}
                  >
                    {TOUR_START_PROJECT_LABEL}
                  </Button>
                ) : null}
              </div>
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
