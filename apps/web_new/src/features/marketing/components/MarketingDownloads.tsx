// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Downloads page (CLOUD-007/008, parity/specs/cloud.md §2): one desktop
// installer card per OS through the stable latest-release address, the
// unsigned-installer notice, the standalone CLI card and the web
// fallback. Structure is what the oracle pins (3 OS articles in OS
// order with exact hrefs, the notice, the CLI section, the closing
// sign-in link); copy is ours.
import * as React from "react";

import {
  CLI_MSI_URL,
  CLI_SIGNIN_HINT,
  DOWNLOAD_CARDS,
  DOWNLOADS_SIGNING_NOTICE,
  metaFor,
  SIGN_IN_PATH,
  type DownloadCard,
} from "../content.js";

const PRIMARY_ACTION =
  "text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--on-accent) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";
const SECONDARY_ACTION =
  "text-body inline-flex items-center rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-3) font-medium text-(--text) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";
const TEXT_LINK =
  "text-body text-(--accent) underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none rounded-(--radius-control)";

export function MarketingDownloads(): React.ReactElement {
  return (
    <>
      <DownloadsHero />
      <OsCardsSection />
      <CliSection />
      <ClosingSection />
    </>
  );
}

function DownloadsHero(): React.ReactElement {
  return (
    <section className="flex flex-col gap-(--space-4) py-(--space-8)">
      <h1 className="text-h1 font-semibold text-balance text-(--text)">Download Pi Dash.</h1>
      <p className="text-body max-w-2xl text-(--text-muted)">{metaFor("/downloads").description}</p>
    </section>
  );
}

function SectionHeading({ id, children }: { id: string; children: React.ReactNode }): React.ReactElement {
  return (
    <h2 id={id} className="text-h2 font-semibold text-(--text)">
      {children}
    </h2>
  );
}

function OsCardsSection(): React.ReactElement {
  return (
    <section aria-labelledby="downloads-os-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading id="downloads-os-heading">Desktop apps</SectionHeading>
        <div className="grid gap-(--space-6) md:grid-cols-3">
          {DOWNLOAD_CARDS.map((card) => (
            <OsCard key={card.os} card={card} />
          ))}
        </div>
        <p role="note" className="text-caption max-w-2xl text-(--text-muted)">
          {DOWNLOADS_SIGNING_NOTICE}
        </p>
      </div>
    </section>
  );
}

function OsCard({ card }: { card: DownloadCard }): React.ReactElement {
  return (
    <article className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) p-(--space-6)">
      <h3 className="text-body font-medium text-(--text)">{card.os}</h3>
      <p className="text-caption text-(--text-muted)">{card.arch}</p>
      <div className="mt-auto flex flex-col gap-(--space-3)">
        <a href={card.primary.href} className={SECONDARY_ACTION}>
          {card.primary.label}
        </a>
        {card.secondary !== null && (
          <a href={card.secondary.href} className={TEXT_LINK}>
            {card.secondary.label}
          </a>
        )}
      </div>
    </article>
  );
}

function CliSection(): React.ReactElement {
  // A section, not an article: the OS-card read counts every article on
  // the page, so the CLI card must not be one.
  return (
    <section aria-labelledby="cli-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) p-(--space-8)">
        <h2 id="cli-heading" className="text-h2 font-semibold text-(--text)">
          Command line only
        </h2>
        <p className="text-body max-w-2xl text-(--text-muted)">
          Runners need just the CLI. Take the standalone installer, then follow the setup guide.
        </p>
        <div className="flex flex-wrap gap-(--space-4)">
          <a href={CLI_MSI_URL} className={PRIMARY_ACTION}>
            Windows CLI installer
          </a>
          <a href="/docs" className={SECONDARY_ACTION}>
            Read the CLI setup guide
          </a>
        </div>
        <p className="text-caption text-(--text-muted)">{CLI_SIGNIN_HINT}</p>
      </div>
    </section>
  );
}

function ClosingSection(): React.ReactElement {
  return (
    <section aria-labelledby="downloads-closing-heading" className="border-t border-(--border) py-(--space-10)">
      <div className="flex flex-col items-start gap-(--space-4)">
        <SectionHeading id="downloads-closing-heading">Prefer the browser?</SectionHeading>
        <p className="text-body max-w-2xl text-(--text-muted)">
          Nothing to install: the web app runs the same product from any browser.
        </p>
        <a href={SIGN_IN_PATH} className={SECONDARY_ACTION}>
          Sign in on the web
        </a>
      </div>
    </section>
  );
}
