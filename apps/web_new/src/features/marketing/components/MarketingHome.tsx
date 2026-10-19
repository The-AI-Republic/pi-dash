// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing home (CLOUD-003, parity/specs/cloud.md §1). Written from the
// spec's section order: hero, product-loop strip, delivery steps,
// capability grid, open-source panel, closing action. The hero owns the
// page's only top-level heading, so the prerendered document has exactly
// one (CLOUD-005). Destinations come from content.ts; none of the copy is
// carried over from the old app.
import * as React from "react";

import {
  CAPABILITIES,
  DELIVERY_STEPS,
  HERO_ACTIONS,
  POSITIONING_LINE,
  PRODUCT_LOOP,
  REPOSITORY_URL,
  SIGN_IN_PATH,
} from "../content.js";

export function MarketingHome(): React.ReactElement {
  return (
    <>
      <HeroSection />
      <LoopSection />
      <StepsSection />
      <CapabilitiesSection />
      <OpenSourceSection />
      <ClosingSection />
    </>
  );
}

function HeroSection(): React.ReactElement {
  return (
    <section className="grid gap-(--space-10) py-(--space-8) lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-center">
      <div className="flex flex-col gap-(--space-6)">
        <p className="text-caption font-medium tracking-wide text-(--accent) uppercase">
          Issue-driven agent orchestration
        </p>
        <h1 className="text-h1 font-semibold text-balance text-(--text)">Ship the work, not the status meetings.</h1>
        <p className="text-body max-w-xl text-(--text-muted)">{POSITIONING_LINE}</p>
        <div className="flex flex-wrap gap-(--space-4)">
          {HERO_ACTIONS.map((action) => (
            <a
              key={action.href}
              href={action.href}
              className={
                action.emphasis === "primary"
                  ? "text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--accent-contrast) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none"
                  : "text-body inline-flex items-center rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-3) font-medium text-(--text) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none"
              }
            >
              {action.label}
            </a>
          ))}
        </div>
      </div>
      <RunPanel />
    </section>
  );
}

/** A representative agent run: illustrative, no data and no behavior. */
function RunPanel(): React.ReactElement {
  const rows = [
    { label: "Issue", value: "NEWFRONT-645 · Cloud marketing surface" },
    { label: "Runner", value: "research-runner · up 4h" },
    { label: "Agent", value: "working · validation" },
    { label: "Checks", value: "typecheck ✓  lint ✓  unit 284/284 ✓" },
  ];
  return (
    <div
      aria-hidden="false"
      role="img"
      aria-label="Illustration of an agent run driving an issue through validation"
      className="shadow-sm rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-5)"
    >
      <div className="flex items-center gap-(--space-2) pb-(--space-4)">
        <span aria-hidden="true" className="size-2 rounded-full bg-(--accent)" />
        <span aria-hidden="true" className="size-2 rounded-full bg-(--border)" />
        <span aria-hidden="true" className="size-2 rounded-full bg-(--border)" />
        <span className="text-caption ml-auto text-(--text-muted)">In progress</span>
      </div>
      <dl className="flex flex-col gap-(--space-3)">
        {rows.map((row) => (
          <div
            key={row.label}
            className="flex items-baseline justify-between gap-(--space-4) border-b border-(--border) pb-(--space-3) last:border-b-0 last:pb-0"
          >
            <dt className="text-caption text-(--text-muted)">{row.label}</dt>
            <dd className="text-body text-right text-(--text)">{row.value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

function SectionHeading({ children }: { children: React.ReactNode }): React.ReactElement {
  return <h2 className="text-h2 font-semibold text-(--text)">{children}</h2>;
}

function LoopSection(): React.ReactElement {
  return (
    <section aria-labelledby="loop-heading" className="border-y border-(--border) py-(--space-8)">
      <h2 id="loop-heading" className="sr-only">
        The product loop
      </h2>
      <div className="grid gap-(--space-6) sm:grid-cols-2 lg:grid-cols-4">
        {PRODUCT_LOOP.map((item, index) => (
          <div key={item.title} className="flex flex-col gap-(--space-2)">
            <span aria-hidden="true" className="text-caption font-mono text-(--accent)">
              {String(index + 1).padStart(2, "0")}
            </span>
            <span className="text-body font-medium text-(--text)">{item.title}</span>
            <span className="text-caption text-(--text-muted)">{item.body}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

function StepsSection(): React.ReactElement {
  return (
    <section aria-labelledby="steps-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading>From scoped issue to merged change</SectionHeading>
        <ol className="grid gap-(--space-6) md:grid-cols-2 xl:grid-cols-4">
          {DELIVERY_STEPS.map((step, index) => (
            <li
              key={step.title}
              className="flex flex-col gap-(--space-2) rounded-(--radius-card) border border-(--border) p-(--space-5)"
            >
              <span className="text-caption font-medium text-(--accent)">Step {index + 1}</span>
              <span className="text-body font-medium text-(--text)">{step.title}</span>
              <span className="text-caption text-(--text-muted)">{step.body}</span>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}

function CapabilitiesSection(): React.ReactElement {
  return (
    <section aria-labelledby="capabilities-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading>Built for work that finishes</SectionHeading>
        <div className="grid gap-(--space-6) sm:grid-cols-2">
          {CAPABILITIES.map((capability) => (
            <article
              key={capability.title}
              className="flex flex-col gap-(--space-2) rounded-(--radius-card) bg-(--subtle) p-(--space-6)"
            >
              <h3 className="text-body font-medium text-(--text)">{capability.title}</h3>
              <p className="text-caption text-(--text-muted)">{capability.body}</p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}

function OpenSourceSection(): React.ReactElement {
  return (
    <section aria-labelledby="open-source-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) p-(--space-8) md:flex-row md:items-center md:justify-between">
        <div className="flex flex-col gap-(--space-2)">
          <SectionHeading>Written in the open</SectionHeading>
          <p className="text-body max-w-xl text-(--text-muted)">
            The application, the runners and their integrations are public. Read the code, file issues, or run the whole
            stack yourself.
          </p>
        </div>
        <a
          href={REPOSITORY_URL}
          target="_blank"
          rel="noreferrer noopener"
          className="text-body inline-flex shrink-0 items-center rounded-(--radius-control) border border-(--border) px-(--space-5) py-(--space-3) font-medium text-(--text) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none"
        >
          View the repository
        </a>
      </div>
    </section>
  );
}

function ClosingSection(): React.ReactElement {
  return (
    <section aria-labelledby="closing-heading" className="border-t border-(--border) py-(--space-12)">
      <div className="flex flex-col items-center gap-(--space-5) text-center">
        <SectionHeading>Give the next issue to an agent</SectionHeading>
        <p className="text-body max-w-xl text-(--text-muted)">
          Start with the free tier, connect a runner, and watch the first issue go from scoped to done.
        </p>
        <a
          href={SIGN_IN_PATH}
          className="text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--accent-contrast) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none"
        >
          Start with Pi Dash
        </a>
      </div>
    </section>
  );
}
