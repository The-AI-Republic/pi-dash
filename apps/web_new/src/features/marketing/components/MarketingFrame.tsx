// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared public frame (CLOUD-004, parity/specs/cloud.md §1): brand header,
// content, footer and the decorative backdrop. Responsive hiding follows
// the oracle: docs, pricing and the start action leave first, the
// repository link follows, downloads and sign-in stay reachable. The links
// stay in the markup and hide through CSS so the frame never reflows into
// two rows on cramped widths.
import * as React from "react";

import { FOOTER_LINKS, HEADER_LINKS, POSITIONING_LINE, type MarketingLink } from "../content.js";
import { MarketingBackdrop } from "./Backdrop.js";

/** Focus ring shared by every header/footer anchor. */
const FOCUS_RING =
  "rounded-(--radius-control) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";

export interface MarketingFrameProps {
  children: React.ReactNode;
}

export function MarketingFrame({ children }: MarketingFrameProps): React.ReactElement {
  return (
    <div className="relative isolate flex min-h-screen flex-col bg-(--bg) text-(--text)">
      <MarketingBackdrop />
      <MarketingHeader />
      <main className="relative z-10 mx-auto w-full max-w-6xl flex-1 px-(--space-6) py-(--space-10)">{children}</main>
      <MarketingFooter />
    </div>
  );
}

export function MarketingHeader(): React.ReactElement {
  return (
    <header className="relative z-10 border-b border-(--border) bg-(--bg)/80 backdrop-blur">
      <div className="mx-auto flex w-full max-w-6xl items-center gap-(--space-6) px-(--space-6) py-(--space-4)">
        <a
          href="/home"
          aria-label="Pi Dash home"
          className={`text-body flex shrink-0 items-center gap-(--space-2) font-semibold text-(--text) ${FOCUS_RING}`}
        >
          <span
            aria-hidden="true"
            className="text-caption flex size-7 items-center justify-center rounded-full bg-(--accent) font-bold text-(--on-accent)"
          >
            π
          </span>
          Pi Dash
        </a>
        <nav aria-label="Primary" className="ml-auto flex min-w-0 items-center gap-(--space-6)">
          {HEADER_LINKS.map((link) => (
            <HeaderLink key={link.label} link={link} />
          ))}
        </nav>
      </div>
    </header>
  );
}

const HIDDEN_BELOW_SM = "hidden sm:inline-flex";
const HIDDEN_BELOW_MD = "hidden md:inline-flex";

/** First layer drops below the small breakpoint, second below medium. */
const RESPONSIVE_CLASS: Readonly<Record<string, string | undefined>> = {
  Docs: HIDDEN_BELOW_SM,
  Pricing: HIDDEN_BELOW_SM,
  "Start free": HIDDEN_BELOW_SM,
  GitHub: HIDDEN_BELOW_MD,
};

function HeaderLink({ link }: { link: MarketingLink }): React.ReactElement {
  const responsive = RESPONSIVE_CLASS[link.label] ?? "inline-flex";
  const emphasis = link.label === "Start free" || link.label === "Sign in";
  return (
    <a
      href={link.href}
      {...(link.external ? { target: "_blank", rel: "noreferrer noopener" } : {})}
      className={[
        "items-center text-body",
        emphasis ? "font-medium text-(--text)" : "text-(--text-muted) hover:text-(--text)",
        responsive,
        FOCUS_RING,
      ].join(" ")}
    >
      {link.label}
    </a>
  );
}

export function MarketingFooter(): React.ReactElement {
  return (
    <footer className="relative z-10 border-t border-(--border) bg-(--bg)">
      <div className="mx-auto flex w-full max-w-6xl flex-col gap-(--space-6) px-(--space-6) py-(--space-8) sm:flex-row sm:items-start sm:justify-between">
        <div className="flex flex-col gap-(--space-2)">
          <span className="text-body flex items-center gap-(--space-2) font-semibold text-(--text)">
            <span
              aria-hidden="true"
              className="text-caption flex size-6 items-center justify-center rounded-full bg-(--accent) font-bold text-(--on-accent)"
            >
              π
            </span>
            Pi Dash
          </span>
          <p className="text-caption max-w-sm text-(--text-muted)">{POSITIONING_LINE}</p>
        </div>
        <nav aria-label="Footer" className="flex flex-wrap gap-(--space-4)">
          {FOOTER_LINKS.map((link) => (
            <a
              key={link.label}
              href={link.href}
              {...(link.external ? { target: "_blank", rel: "noreferrer noopener" } : {})}
              className={`text-caption text-(--text-muted) hover:text-(--text) ${FOCUS_RING}`}
            >
              {link.label}
            </a>
          ))}
        </nav>
      </div>
    </footer>
  );
}
