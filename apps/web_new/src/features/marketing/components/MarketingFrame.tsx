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
      <main className="relative z-10 flex-1">{children}</main>
      <MarketingFooter />
    </div>
  );
}

/**
 * The shared content column: one centred gutter on every page, so a page
 * can let a banded section run edge to edge and still align its text to
 * the header and footer.
 */
export function MarketingColumn({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}): React.ReactElement {
  return <div className={["mx-auto w-full max-w-7xl px-5 sm:px-8", className].join(" ")}>{children}</div>;
}

export function MarketingHeader(): React.ReactElement {
  return (
    <header className="sticky top-0 z-30 border-b border-(--border) bg-(--bg)/90 backdrop-blur-xl">
      <div className="mx-auto flex h-16 w-full max-w-7xl items-center justify-between gap-(--space-3) px-5 sm:px-8">
        <BrandMark />
        <nav aria-label="Primary" className="ml-auto flex min-w-0 items-center gap-(--space-1) sm:gap-(--space-2)">
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

/** Which fill each header action takes; plain links are quiet until hover. */
const HEADER_TONE: Readonly<Record<string, string>> = {
  "Start free":
    "rounded-(--radius-card) bg-(--accent) px-(--space-8) py-(--space-4) font-semibold text-(--on-accent) hover:bg-(--accent-hover)",
  "Sign in":
    "rounded-(--radius-card) border border-(--border) bg-(--surface) px-(--space-8) py-(--space-4) font-semibold text-(--text) hover:bg-(--subtle)",
};

function HeaderLink({ link }: { link: MarketingLink }): React.ReactElement {
  const responsive = RESPONSIVE_CLASS[link.label] ?? "inline-flex";
  const tone =
    HEADER_TONE[link.label] ??
    "rounded-(--radius-card) px-(--space-3) py-(--space-4) text-(--text-muted) hover:bg-(--surface) hover:text-(--text) sm:px-(--space-6)";
  return (
    <a
      href={link.href}
      {...(link.external ? { target: "_blank", rel: "noreferrer noopener" } : {})}
      className={[
        "items-center text-[0.875rem] leading-[1.4] font-medium transition-colors",
        tone,
        responsive,
        FOCUS_RING,
      ].join(" ")}
    >
      {link.label}
    </a>
  );
}

/** Four rounded bars: a dot, a pair of pills and a dot, spelling the mark. */
function BrandMark(): React.ReactElement {
  return (
    <a
      href="/home"
      aria-label="Pi Dash home"
      className={`flex shrink-0 items-center gap-(--space-2) rounded-(--radius-card) text-(--text) ${FOCUS_RING}`}
    >
      <span aria-hidden="true" className="flex items-center gap-(--space-1) text-(--accent)">
        <span className="block size-3 rounded-full bg-current" />
        <span className="block h-3 w-7 rounded-full bg-current" />
        <span className="block h-3 w-7 rounded-full bg-current" />
        <span className="block size-3 rounded-full bg-current" />
      </span>
      <span className="hidden text-[1rem] leading-[1.4] font-semibold min-[400px]:inline">Pi Dash</span>
    </a>
  );
}

export function MarketingFooter(): React.ReactElement {
  return (
    <footer className="relative z-10 border-t border-(--border) bg-(--bg)">
      <div className="mx-auto flex w-full max-w-7xl flex-col gap-(--space-6) px-5 py-10 sm:flex-row sm:items-center sm:justify-between sm:px-8">
        <div className="flex flex-col gap-(--space-2)">
          <span className="flex items-center gap-(--space-2) text-[1rem] leading-[1.4] font-semibold text-(--text)">
            <span aria-hidden="true" className="flex items-center gap-(--space-1) text-(--accent)">
              <span className="block size-3 rounded-full bg-current" />
              <span className="block h-3 w-7 rounded-full bg-current" />
              <span className="block h-3 w-7 rounded-full bg-current" />
              <span className="block size-3 rounded-full bg-current" />
            </span>
            Pi Dash
          </span>
          <p className="max-w-sm text-[0.875rem] leading-[1.4] text-(--text-muted)">{POSITIONING_LINE}</p>
        </div>
        <nav aria-label="Footer" className="flex flex-wrap gap-(--space-4)">
          {FOOTER_LINKS.map((link) => (
            <a
              key={link.label}
              href={link.href}
              {...(link.external ? { target: "_blank", rel: "noreferrer noopener" } : {})}
              className={`text-[0.875rem] leading-[1.4] font-medium text-(--text-muted) hover:text-(--text) ${FOCUS_RING}`}
            >
              {link.label}
            </a>
          ))}
        </nav>
      </div>
    </footer>
  );
}
