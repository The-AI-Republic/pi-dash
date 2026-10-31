// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing home (CLOUD-003, parity/specs/cloud.md §1). Section order is
// the contract: a two-column hero with its three onward actions and a
// run illustration, then a four-item value strip, the four-step delivery
// narrative, four capability cards, the open-source panel, and the closing
// call to sign-in. The hero owns the page's only top-level heading
// (CLOUD-005). Wording lives in content.ts.
//
// Scale and rhythm follow the old cloud landing page (NEWFRONT-1041): its
// rendered sizes were measured in the live product and restated here as
// prose before this file was written. Card and strip padding sit on the
// 24px step, body copy on 16px labels and 14/13px supporting lines, and the
// banded sections run edge to edge while their content stays on the shared
// column. The old page's own section headings render at 16px only because
// its heading utilities never reached its stylesheet; the size it asked for
// (36px, 48px from the small breakpoint) is kept here.
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
import { MarketingColumn } from "./MarketingFrame.js";

const FOCUS_RING =
  "focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";

const ACTION_BASE =
  `inline-flex min-h-11 items-center justify-center gap-2 rounded-xl ` +
  `px-5 py-3 text-base font-semibold ${FOCUS_RING}`;

const PRIMARY_ACTION = `${ACTION_BASE} bg-(--accent) text-(--on-accent) hover:bg-(--accent-hover)`;

const SECONDARY_ACTION = `${ACTION_BASE} border border-(--border) bg-(--surface) text-(--text) hover:bg-(--subtle)`;

/** Eyebrow above a section headline. */
const EYEBROW = "text-xs font-semibold tracking-[0.18em] text-(--accent) uppercase";

/** In-page section headline: the size the old page asked its headings for. */
const SECTION_TITLE = "text-4xl leading-tight font-semibold tracking-[-0.035em] text-(--text) sm:text-5xl";

const CARD_TITLE = "text-2xl leading-[1.2] font-semibold text-(--text)";
const CARD_BODY = "text-base leading-7 text-(--text-muted)";

/** Supporting copy inside the run illustration, one step under body size. */
const PREVIEW_LABEL = "text-sm leading-[1.4]";
const PREVIEW_NOTE = "text-[0.8125rem] leading-[1.4]";

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
  const primary = HERO_ACTIONS.find((action) => action.label === link.label)?.emphasis === "primary";
  return (
    <a href={link.href} className={primary ? PRIMARY_ACTION : SECONDARY_ACTION}>
      {link.label}
      {primary ? <ArrowRight aria-hidden="true" className="size-4" /> : null}
    </a>
  );
}

function Hero(): React.ReactElement {
  return (
    <MarketingColumn>
      <section className="grid w-full gap-12 pt-16 pb-20 sm:pt-24 lg:grid-cols-[1.02fr_0.98fr] lg:items-center lg:gap-16 lg:pt-28 lg:pb-28">
        <div className="min-w-0 lg:self-start">
          <p className="mb-7 inline-flex items-center gap-2 rounded-(--radius-full) border border-(--border) bg-(--surface) px-3 py-1.5 text-[0.8125rem] leading-[1.4] font-semibold tracking-[0.16em] text-(--text-muted) uppercase">
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
          <p className="text-base mt-7 max-w-2xl leading-8 text-(--text-muted)">{HERO_INTRO}</p>
          <div className="mt-9 flex flex-col gap-3 sm:flex-row sm:flex-wrap">
            {HERO_ACTIONS.map((action) => (
              <ActionLink key={action.href} link={action} />
            ))}
          </div>
          <p className="text-sm mt-4 text-(--text-muted)">{HERO_NOTE}</p>
        </div>
        <RunPreview />
      </section>
    </MarketingColumn>
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
      <div className="flex items-center justify-between gap-4 border-b border-(--border) px-4 py-3 sm:px-5">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-(--radius-card) bg-(--accent-subtle) text-(--accent)">
            <Bot aria-hidden="true" className="size-4" />
          </span>
          <div className="min-w-0">
            <p className={`${PREVIEW_LABEL} font-semibold text-(--text)`}>{RUN_PREVIEW.issue}</p>
            <p className={`${PREVIEW_NOTE} truncate text-(--text-muted)`}>{RUN_PREVIEW.title}</p>
          </div>
        </div>
        <span
          className={`${PREVIEW_NOTE} shrink-0 rounded-(--radius-full) border border-(--border) bg-(--subtle) px-2.5 py-1 font-semibold text-(--success)`}
        >
          {RUN_PREVIEW.status}
        </span>
      </div>

      <div className="grid min-h-[330px] grid-cols-1 md:grid-cols-[0.9fr_1.1fr]">
        <div className="flex min-w-0 flex-col gap-3 border-b border-(--border) p-4 sm:p-5 md:border-r md:border-b-0">
          <p className={`${PREVIEW_NOTE} font-semibold tracking-[0.18em] text-(--text-muted) uppercase`}>
            {RUN_PREVIEW.heading}
          </p>
          {RUN_PREVIEW.steps.map((step) => {
            const DoneIcon = STEP_MARK[step.state];
            return (
              <div key={step.label} className="flex items-center gap-(--space-6)">
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
                  className={`${PREVIEW_LABEL} font-medium ${
                    step.state === "queued" ? "text-(--text-muted)" : "text-(--text)"
                  }`}
                >
                  {step.label}
                </span>
              </div>
            );
          })}
          <div className="mt-auto rounded-(--radius-card) border border-(--border) bg-(--bg) p-3">
            <p className={`${PREVIEW_NOTE} mb-2 flex items-center gap-2 font-semibold text-(--text-muted)`}>
              <span aria-hidden="true" className="size-2 rounded-full bg-(--success)" />
              {RUN_PREVIEW.runner.name}
            </p>
            <p className={`${PREVIEW_NOTE} text-(--text-muted)`}>{RUN_PREVIEW.runner.note}</p>
          </div>
        </div>

        <div className="flex min-w-0 flex-col gap-4 bg-(--bg)/60 p-4 sm:p-5">
          <p
            className={`${PREVIEW_NOTE} flex items-center gap-2 font-semibold tracking-[0.18em] text-(--text-muted) uppercase`}
          >
            <MessageSquare aria-hidden="true" className="size-4" />
            {RUN_PREVIEW.activity.heading}
          </p>
          <div className="rounded-(--radius-card) border border-(--border) bg-(--surface) p-4">
            <p className={`${PREVIEW_LABEL} font-medium text-(--text)`}>{RUN_PREVIEW.activity.headline}</p>
            <p className={`${PREVIEW_LABEL} mt-1 text-(--text-muted)`}>{RUN_PREVIEW.activity.body}</p>
          </div>
          <div className="rounded-(--radius-card) border border-(--border) bg-(--surface) p-4">
            <p className={`${PREVIEW_LABEL} mb-2 flex items-center gap-2 font-semibold text-(--text)`}>
              <ListChecks aria-hidden="true" className="size-4 text-(--accent)" />
              {RUN_PREVIEW.checks.heading}
            </p>
            {RUN_PREVIEW.checks.rows.map((row) => (
              <div
                key={row.label}
                className={`${PREVIEW_NOTE} mt-2 flex items-center justify-between gap-4 text-(--text-muted)`}
              >
                <span>{row.label}</span>
                <span className={row.state === "done" ? "text-(--success)" : "text-(--accent)"}>{row.value}</span>
              </div>
            ))}
          </div>
          <div className="mt-auto flex items-center gap-3 rounded-(--radius-card) border border-(--border) bg-(--surface) p-4">
            <GitPullRequest aria-hidden="true" className="size-5 shrink-0 text-(--text-muted)" />
            <div className="min-w-0">
              <p className={`${PREVIEW_LABEL} font-semibold text-(--text)`}>{RUN_PREVIEW.review.title}</p>
              <p className={`${PREVIEW_NOTE} truncate text-(--text-muted)`}>{RUN_PREVIEW.review.body}</p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function ValueStrip(): React.ReactElement {
  return (
    <section aria-label="What Pi Dash tracks" className="border-y border-(--border) bg-(--surface)/70">
      {/* The band fills the viewport; the grid itself carries the shared
          column so each highlight lines up with the chrome above it. */}
      <MarketingColumn className="grid grid-cols-2 gap-px py-8 md:grid-cols-4">
        {PRODUCT_LOOP.map((item, index) => (
          <div key={item.title} className={`px-3 py-4 md:px-6 ${index === 0 ? "pl-0" : ""}`}>
            <p className="text-sm leading-[1.4] font-semibold text-(--text)">{item.title}</p>
            <p className="mt-1 text-[0.8125rem] leading-[1.4] text-(--text-muted)">{item.body}</p>
          </div>
        ))}
      </MarketingColumn>
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
      <p className={`${EYEBROW} mb-4`}>{eyebrow}</p>
      <h2 className={SECTION_TITLE}>{title}</h2>
      <p className="text-base mt-5 leading-8 text-(--text-muted)">{intro}</p>
    </div>
  );
}

function DeliverySection(): React.ReactElement {
  return (
    <section aria-labelledby="delivery-heading" className="w-full py-20 sm:py-28">
      <MarketingColumn>
        <div className="mb-14">
          <SectionIntro
            eyebrow={HOME_SECTIONS.steps.eyebrow}
            title={HOME_SECTIONS.steps.title}
            intro={HOME_SECTIONS.steps.intro}
          />
          <h3 id="delivery-heading" className="sr-only">
            Delivery steps
          </h3>
        </div>
        <ol className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          {DELIVERY_STEPS.map((step, index) => (
            <li
              key={step.title}
              className="flex min-h-64 flex-col rounded-2xl border border-(--border) bg-(--surface) p-6 transition-transform hover:-translate-y-1"
            >
              <span className="font-mono text-xs font-semibold tracking-[0.18em] text-(--accent)">
                {String(index + 1).padStart(2, "0")}
              </span>
              <span className={`${CARD_TITLE} mt-10`}>{step.title}</span>
              <span className={`${CARD_BODY} mt-3`}>{step.body}</span>
            </li>
          ))}
        </ol>
      </MarketingColumn>
    </section>
  );
}

function CapabilitySection(): React.ReactElement {
  return (
    <section aria-labelledby="capabilities-heading" className="w-full border-y border-(--border) bg-(--surface)/70">
      <MarketingColumn>
        <div className="grid gap-12 py-20 sm:py-28 lg:grid-cols-[0.8fr_1.2fr]">
          <div className="max-w-xl">
            <p className={`${EYEBROW} mb-4`}>{HOME_SECTIONS.capabilities.eyebrow}</p>
            <h2 id="capabilities-heading" className={SECTION_TITLE}>
              {HOME_SECTIONS.capabilities.title}
            </h2>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            {CAPABILITIES.map((capability, index) => {
              const Icon = CAPABILITY_ICONS[index % CAPABILITY_ICONS.length]!;
              return (
                <article key={capability.title} className="rounded-2xl border border-(--border) bg-(--bg) p-6">
                  <div className="mb-5 flex size-10 items-center justify-center rounded-xl bg-(--accent-subtle) text-(--accent)">
                    <Icon aria-hidden="true" className="size-5" />
                  </div>
                  <h3 className={CARD_TITLE}>{capability.title}</h3>
                  <p className={`${CARD_BODY} mt-3`}>{capability.body}</p>
                </article>
              );
            })}
          </div>
        </div>
      </MarketingColumn>
    </section>
  );
}

function OpenSourceSection(): React.ReactElement {
  return (
    <section aria-labelledby="open-source-heading" className="w-full py-20 sm:py-28">
      <MarketingColumn>
        <div className="grid gap-10 rounded-3xl border border-(--border) bg-(--surface) p-8 sm:p-12 lg:grid-cols-[1.15fr_0.85fr] lg:items-center">
          <div className="max-w-3xl">
            <p className={`${EYEBROW} mb-4`}>{HOME_SECTIONS.openSource.eyebrow}</p>
            <h2 id="open-source-heading" className={SECTION_TITLE}>
              {HOME_SECTIONS.openSource.title}
            </h2>
            <p className="text-base mt-5 max-w-2xl leading-8 text-(--text-muted)">{HOME_SECTIONS.openSource.body}</p>
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
      </MarketingColumn>
    </section>
  );
}

function ClosingSection(): React.ReactElement {
  return (
    <section aria-labelledby="closing-heading" className="w-full pb-20 sm:pb-28">
      <MarketingColumn>
        <div className="relative overflow-hidden rounded-3xl border border-(--border) bg-(--surface) px-6 py-14 text-center sm:px-12 sm:py-20">
          <div
            aria-hidden="true"
            className="absolute top-1/2 left-1/2 size-[28rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-(--accent-subtle) blur-3xl"
          />
          <div className="relative mx-auto max-w-3xl">
            <p className={`${EYEBROW} mb-4`}>{HOME_SECTIONS.closing.eyebrow}</p>
            <h2
              id="closing-heading"
              className="text-4xl sm:text-6xl leading-tight font-semibold tracking-[-0.04em] text-(--text)"
            >
              {HOME_SECTIONS.closing.title}
            </h2>
            <p className="text-base mx-auto mt-5 max-w-2xl leading-8 text-(--text-muted)">
              {HOME_SECTIONS.closing.body}
            </p>
            <div className="mt-8 flex justify-center">
              <a href={SIGN_IN_PATH} className={PRIMARY_ACTION}>
                {HOME_SECTIONS.closing.action}
                <ArrowRight aria-hidden="true" className="size-4" />
              </a>
            </div>
          </div>
        </div>
      </MarketingColumn>
    </section>
  );
}
