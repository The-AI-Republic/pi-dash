// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing home (CLOUD-003, parity/specs/cloud.md §1). Section order is
// the contract: a two-column hero with its three onward actions and a
// run illustration, then a four-item value strip, the four-step delivery
// narrative, four capability cards, the open-source panel, and the closing
// call to sign-in. The hero owns the page's only top-level heading
// (CLOUD-005). Wording lives in content.ts.
import * as React from "react";
import {
  ArrowRight,
  ArrowUpRight,
  Bot,
  CheckCircle2,
  GitPullRequest,
  ListChecks,
  MessageSquare,
  ShieldCheck,
  SquareTerminal,
  Workflow,
} from "lucide-react";

import {
  CAPABILITIES,
  DELIVERY_STEPS,
  HERO_ACTIONS,
  HERO_BADGE,
  HERO_HEADLINES,
  HERO_INTRO,
  HERO_NOTE,
  HOME_SECTIONS,
  PRODUCT_LOOP,
  REPOSITORY_URL,
  RUN_PREVIEW,
  SIGN_IN_PATH,
  type MarketingLink,
} from "../content.js";

const FOCUS_RING =
  "focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";

const PRIMARY_ACTION =
  `inline-flex min-h-11 items-center justify-center gap-(--space-2) rounded-(--radius-control) ` +
  `bg-(--accent) px-(--space-6) py-(--space-3) text-body font-medium text-(--on-accent) ` +
  `hover:bg-(--accent-hover) ${FOCUS_RING}`;

const SECONDARY_ACTION =
  `inline-flex min-h-11 items-center justify-center gap-(--space-2) rounded-(--radius-control) ` +
  `border border-(--border) bg-(--surface) px-(--space-6) py-(--space-3) text-body font-medium ` +
  `text-(--text) hover:bg-(--subtle) ${FOCUS_RING}`;

const CAPABILITY_ICONS = [Workflow, Bot, SquareTerminal, ShieldCheck] as const;

export function MarketingHome(): React.ReactElement {
  return (
    <>
      <Hero />
      <ValueStrip />
      <DeliverySection />
      <CapabilitySection />
      <OpenSourceSection />
      <ClosingSection />
    </>
  );
}

function ActionLink({ link }: { link: MarketingLink }): React.ReactElement {
  const primary = HERO_ACTIONS.find((action) => action.href === link.href)?.emphasis === "primary";
  return (
    <a href={link.href} className={primary ? PRIMARY_ACTION : SECONDARY_ACTION}>
      {link.label}
      {primary ? <ArrowRight aria-hidden="true" className="size-4" /> : null}
    </a>
  );
}

function Hero(): React.ReactElement {
  return (
    <section className="grid w-full gap-(--space-12) pb-(--space-20) lg:grid-cols-[1.02fr_0.98fr] lg:items-center lg:gap-(--space-16)">
      <div className="min-w-0">
        <p className="text-caption mb-(--space-7) inline-flex items-center gap-(--space-2) rounded-(--radius-full) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) font-semibold tracking-[0.16em] text-(--text-muted) uppercase">
          <span aria-hidden="true" className="size-2 rounded-full bg-(--success)" />
          {HERO_BADGE}
        </p>
        <h1 className="text-[clamp(3rem,7vw,5.8rem)] leading-[0.97] font-semibold tracking-[-0.055em] text-(--text)">
          {HERO_HEADLINES.map((line) => (
            <React.Fragment key={line}>
              <span className="block">{line}</span>
            </React.Fragment>
          ))}
        </h1>
        <p className="text-lg mt-(--space-8) max-w-2xl leading-8 text-(--text-muted)">{HERO_INTRO}</p>
        <div className="mt-(--space-10) flex flex-col gap-(--space-3) sm:flex-row sm:flex-wrap">
          {HERO_ACTIONS.map((action) => (
            <ActionLink key={action.href} link={action} />
          ))}
        </div>
        <p className="text-caption mt-(--space-4) text-(--text-muted)">{HERO_NOTE}</p>
      </div>
      <RunPreview />
    </section>
  );
}

const STEP_MARK = {
  done: CheckCircle2,
  active: null,
  queued: null,
} as const;

/** A representative agent run: illustrative only, no data and no behavior. */
function RunPreview(): React.ReactElement {
  return (
    <div
      role="img"
      aria-label="Illustration of an agent run driving an issue through its checks"
      className="shadow-popover relative w-full overflow-hidden rounded-2xl border border-(--border) bg-(--surface)"
    >
      <div className="flex items-center justify-between gap-(--space-4) border-b border-(--border) px-(--space-4) py-(--space-3)">
        <div className="flex min-w-0 items-center gap-(--space-3)">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-(--radius-card) bg-(--accent-subtle) text-(--accent)">
            <Bot aria-hidden="true" className="size-4" />
          </span>
          <div className="min-w-0">
            <p className="text-meta font-semibold text-(--text)">{RUN_PREVIEW.issue}</p>
            <p className="text-caption truncate text-(--text-muted)">{RUN_PREVIEW.title}</p>
          </div>
        </div>
        <span className="text-caption shrink-0 rounded-(--radius-full) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-1) font-semibold text-(--success)">
          {RUN_PREVIEW.status}
        </span>
      </div>

      <div className="grid min-h-[330px] md:grid-cols-[0.9fr_1.1fr]">
        <div className="flex flex-col gap-(--space-3) border-b border-(--border) p-(--space-5) md:border-r md:border-b-0">
          <p className="text-caption font-semibold tracking-[0.18em] text-(--text-muted) uppercase">
            {RUN_PREVIEW.heading}
          </p>
          {RUN_PREVIEW.steps.map((step) => {
            const DoneIcon = STEP_MARK[step.state];
            return (
              <div key={step.label} className="flex items-center gap-(--space-3)">
                {DoneIcon !== null ? (
                  <DoneIcon aria-hidden="true" className="size-4 shrink-0 text-(--success)" />
                ) : (
                  <span
                    aria-hidden="true"
                    className={
                      step.state === "active"
                        ? "size-4 shrink-0 rounded-full border-2 border-(--accent) bg-(--accent-subtle)"
                        : "size-4 shrink-0 rounded-full border border-(--border)"
                    }
                  />
                )}
                <span
                  className={`text-meta font-medium ${step.state === "queued" ? "text-(--text-muted)" : "text-(--text)"}`}
                >
                  {step.label}
                </span>
              </div>
            );
          })}
          <div className="mt-auto rounded-(--radius-card) border border-(--border) bg-(--bg) p-(--space-3)">
            <p className="text-caption mb-(--space-2) flex items-center gap-(--space-2) font-semibold text-(--text-muted)">
              <span aria-hidden="true" className="size-2 rounded-full bg-(--success)" />
              {RUN_PREVIEW.runner.name}
            </p>
            <p className="text-caption text-(--text-muted)">{RUN_PREVIEW.runner.note}</p>
          </div>
        </div>

        <div className="flex flex-col gap-(--space-4) bg-(--bg)/60 p-(--space-5)">
          <p className="text-caption flex items-center gap-(--space-2) font-semibold tracking-[0.18em] text-(--text-muted) uppercase">
            <MessageSquare aria-hidden="true" className="size-4" />
            {RUN_PREVIEW.activity.heading}
          </p>
          <div className="rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)">
            <p className="text-meta font-medium text-(--text)">{RUN_PREVIEW.activity.headline}</p>
            <p className="text-caption mt-(--space-1) text-(--text-muted)">{RUN_PREVIEW.activity.body}</p>
          </div>
          <div className="rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)">
            <p className="text-meta mb-(--space-2) flex items-center gap-(--space-2) font-semibold text-(--text)">
              <ListChecks aria-hidden="true" className="size-4 text-(--accent)" />
              {RUN_PREVIEW.checks.heading}
            </p>
            {RUN_PREVIEW.checks.rows.map((row) => (
              <div
                key={row.label}
                className="text-caption mt-(--space-2) flex items-center justify-between gap-(--space-4) text-(--text-muted)"
              >
                <span>{row.label}</span>
                <span className={row.state === "done" ? "text-(--success)" : "text-(--accent)"}>{row.value}</span>
              </div>
            ))}
          </div>
          <div className="mt-auto flex items-center gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)">
            <GitPullRequest aria-hidden="true" className="size-5 shrink-0 text-(--text-muted)" />
            <div className="min-w-0">
              <p className="text-meta font-semibold text-(--text)">{RUN_PREVIEW.review.title}</p>
              <p className="text-caption truncate text-(--text-muted)">{RUN_PREVIEW.review.body}</p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function ValueStrip(): React.ReactElement {
  return (
    <section aria-label="What Pi Dash tracks" className="border-y border-(--border) bg-(--surface)">
      <div className="grid w-full grid-cols-2 gap-px py-(--space-8) md:grid-cols-4">
        {PRODUCT_LOOP.map((item) => (
          <div key={item.title} className="px-(--space-4) py-(--space-4) md:px-(--space-6)">
            <p className="text-meta font-semibold text-(--text)">{item.title}</p>
            <p className="text-caption mt-(--space-1) text-(--text-muted)">{item.body}</p>
          </div>
        ))}
      </div>
    </section>
  );
}

function SectionIntro({
  eyebrow,
  title,
  intro,
}: {
  eyebrow: string;
  title: string;
  intro: string;
}): React.ReactElement {
  return (
    <div className="max-w-3xl">
      <p className="text-caption mb-(--space-4) font-semibold tracking-[0.18em] text-(--accent) uppercase">{eyebrow}</p>
      <h2 className="text-[clamp(2rem,4vw,3rem)] leading-tight font-semibold tracking-[-0.035em] text-(--text)">
        {title}
      </h2>
      <p className="text-lg mt-(--space-5) leading-8 text-(--text-muted)">{intro}</p>
    </div>
  );
}

function DeliverySection(): React.ReactElement {
  return (
    <section aria-labelledby="delivery-heading" className="w-full py-(--space-20)">
      <div className="mb-(--space-12)">
        <SectionIntro
          eyebrow={HOME_SECTIONS.steps.eyebrow}
          title={HOME_SECTIONS.steps.title}
          intro={HOME_SECTIONS.steps.intro}
        />
        <h3 id="delivery-heading" className="sr-only">
          Delivery steps
        </h3>
      </div>
      <ol className="grid gap-(--space-4) md:grid-cols-2 lg:grid-cols-4">
        {DELIVERY_STEPS.map((step, index) => (
          <li
            key={step.title}
            className="flex min-h-64 flex-col rounded-2xl border border-(--border) bg-(--surface) p-(--space-6) transition-transform hover:-translate-y-1"
          >
            <span className="font-mono text-caption font-semibold tracking-[0.18em] text-(--accent)">
              {String(index + 1).padStart(2, "0")}
            </span>
            <span className="text-h3 mt-(--space-10) text-(--text)">{step.title}</span>
            <span className="text-meta mt-(--space-3) leading-7 text-(--text-muted)">{step.body}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function CapabilitySection(): React.ReactElement {
  return (
    <section aria-labelledby="capabilities-heading" className="w-full border-y border-(--border) bg-(--surface)">
      <div className="grid gap-(--space-12) py-(--space-20) lg:grid-cols-[0.8fr_1.2fr]">
        <div className="max-w-xl">
          <p className="text-caption mb-(--space-4) font-semibold tracking-[0.18em] text-(--accent) uppercase">
            {HOME_SECTIONS.capabilities.eyebrow}
          </p>
          <h2
            id="capabilities-heading"
            className="text-[clamp(2rem,4vw,3rem)] leading-tight font-semibold tracking-[-0.035em] text-(--text)"
          >
            {HOME_SECTIONS.capabilities.title}
          </h2>
        </div>
        <div className="grid gap-(--space-4) sm:grid-cols-2">
          {CAPABILITIES.map((capability, index) => {
            const Icon = CAPABILITY_ICONS[index % CAPABILITY_ICONS.length]!;
            return (
              <article key={capability.title} className="rounded-2xl border border-(--border) bg-(--bg) p-(--space-6)">
                <div className="mb-(--space-5) flex size-10 items-center justify-center rounded-(--radius-card) bg-(--accent-subtle) text-(--accent)">
                  <Icon aria-hidden="true" className="size-5" />
                </div>
                <h3 className="text-h3 text-(--text)">{capability.title}</h3>
                <p className="text-meta mt-(--space-3) leading-7 text-(--text-muted)">{capability.body}</p>
              </article>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function OpenSourceSection(): React.ReactElement {
  return (
    <section aria-labelledby="open-source-heading" className="w-full py-(--space-20)">
      <div className="grid gap-(--space-10) rounded-3xl border border-(--border) bg-(--surface) p-(--space-8) sm:p-(--space-12) lg:grid-cols-[1.15fr_0.85fr] lg:items-center">
        <div className="max-w-3xl">
          <p className="text-caption mb-(--space-4) font-semibold tracking-[0.18em] text-(--accent) uppercase">
            {HOME_SECTIONS.openSource.eyebrow}
          </p>
          <h2
            id="open-source-heading"
            className="text-[clamp(2rem,4vw,3rem)] leading-tight font-semibold tracking-[-0.035em] text-(--text)"
          >
            {HOME_SECTIONS.openSource.title}
          </h2>
          <p className="text-lg mt-(--space-5) max-w-2xl leading-8 text-(--text-muted)">
            {HOME_SECTIONS.openSource.body}
          </p>
        </div>
        <div className="flex lg:justify-end">
          <a
            href={REPOSITORY_URL}
            target="_blank"
            rel="noopener noreferrer"
            className={`${SECONDARY_ACTION} bg-(--bg)`}
          >
            {HOME_SECTIONS.openSource.action}
            <ArrowUpRight aria-hidden="true" className="size-4" />
          </a>
        </div>
      </div>
    </section>
  );
}

function ClosingSection(): React.ReactElement {
  return (
    <section aria-labelledby="closing-heading" className="w-full pb-(--space-20)">
      <div className="relative overflow-hidden rounded-3xl border border-(--border) bg-(--surface) px-(--space-6) py-(--space-12) text-center sm:px-(--space-12) sm:py-(--space-20)">
        <div
          aria-hidden="true"
          className="absolute top-1/2 left-1/2 size-[28rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-(--accent-subtle) blur-3xl"
        />
        <div className="relative mx-auto max-w-3xl">
          <p className="text-caption mb-(--space-4) font-semibold tracking-[0.18em] text-(--accent) uppercase">
            {HOME_SECTIONS.closing.eyebrow}
          </p>
          <h2
            id="closing-heading"
            className="text-[clamp(2rem,5vw,3.75rem)] leading-tight font-semibold tracking-[-0.04em] text-(--text)"
          >
            {HOME_SECTIONS.closing.title}
          </h2>
          <p className="text-lg mx-auto mt-(--space-5) max-w-2xl leading-8 text-(--text-muted)">
            {HOME_SECTIONS.closing.body}
          </p>
          <div className="mt-(--space-8) flex justify-center">
            <a href={SIGN_IN_PATH} className={PRIMARY_ACTION}>
              {HOME_SECTIONS.closing.action}
              <ArrowRight aria-hidden="true" className="size-4" />
            </a>
          </div>
        </div>
      </div>
    </section>
  );
}
