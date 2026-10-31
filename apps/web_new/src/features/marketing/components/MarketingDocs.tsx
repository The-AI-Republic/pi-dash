// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Docs page (CLOUD-006, parity/specs/cloud.md §2): the zero-to-runner
// path. Hero, three-part model, four quick-start steps, the optional
// agent skill, the operating loop and the trust notes. Structure is what
// the oracle pins (3 concept articles, 4 ordered steps, 5 command
// blocks, 3 closing notes); copy is ours and the installer commands are
// transcribed from the runner install guide.
import * as React from "react";

import {
  CLI_MSI_URL,
  DOCS_CONCEPTS,
  DOCS_DEVICE_LOGIN_COMMAND,
  DOCS_ENROLL_COMMAND,
  DOCS_INSTALL_COMMANDS,
  DOCS_LOOP_PHASES,
  DOCS_MSI_NOTE,
  DOCS_SKILL_COMMAND,
  DOCS_STEPS,
  DOCS_TRUST_NOTES,
  DOCS_WSL_NOTE,
  metaFor,
  REPOSITORY_URL,
  SIGN_IN_PATH,
} from "../content.js";

const PRIMARY_ACTION =
  "text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--on-accent) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";
const SECONDARY_ACTION =
  "text-body inline-flex items-center rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-3) font-medium text-(--text) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";
const FOCUS_RING =
  "rounded-(--radius-control) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";

export function MarketingDocs(): React.ReactElement {
  return (
    <>
      <DocsHero />
      <ConceptsSection />
      <StepsSection />
      <SkillSection />
      <LoopSection />
      <TrustSection />
    </>
  );
}

function DocsHero(): React.ReactElement {
  return (
    <section className="flex flex-col gap-(--space-6) py-(--space-8)">
      <h1 className="text-h1 font-semibold text-balance text-(--text)">From zero to your first runner.</h1>
      <p className="text-body max-w-2xl text-(--text-muted)">{metaFor("/docs").description}</p>
      <div className="flex flex-wrap gap-(--space-4)">
        <a href={SIGN_IN_PATH} className={PRIMARY_ACTION}>
          Sign in
        </a>
        <a href={REPOSITORY_URL} target="_blank" rel="noreferrer noopener" className={SECONDARY_ACTION}>
          Source code
        </a>
      </div>
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

function ConceptsSection(): React.ReactElement {
  return (
    <section aria-labelledby="docs-concepts-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading id="docs-concepts-heading">Three parts, one loop</SectionHeading>
        <div className="grid gap-(--space-6) md:grid-cols-3">
          {DOCS_CONCEPTS.map((concept) => (
            <article
              key={concept.title}
              className="flex flex-col gap-(--space-2) rounded-(--radius-card) border border-(--border) p-(--space-6)"
            >
              <h3 className="text-body font-medium text-(--text)">{concept.title}</h3>
              <p className="text-caption text-(--text-muted)">{concept.body}</p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}

function CommandBlock({ label, command }: { label: string; command: string }): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2)">
      <p className="text-caption font-medium text-(--text-muted)">{label}</p>
      <pre className="text-caption font-mono overflow-x-auto rounded-(--radius-card) border border-(--border) bg-(--subtle) p-(--space-4) text-(--text)">
        <code>{command}</code>
      </pre>
    </div>
  );
}

function StepsSection(): React.ReactElement {
  return (
    <section aria-labelledby="docs-steps-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading id="docs-steps-heading">Get running in four steps</SectionHeading>
        <ol className="flex flex-col gap-(--space-6)">
          {DOCS_STEPS.map((step, index) => (
            <li
              key={step.title}
              className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) p-(--space-6)"
            >
              <span className="text-caption font-medium text-(--accent)">Step {index + 1}</span>
              <h3 className="text-body font-medium text-(--text)">{step.title}</h3>
              <p className="text-caption text-(--text-muted)">{step.body}</p>
              {index === 1 && (
                <>
                  {DOCS_INSTALL_COMMANDS.map((entry) => (
                    <CommandBlock key={entry.label} label={entry.label} command={entry.command} />
                  ))}
                  <p className="text-caption text-(--text-muted)">
                    {DOCS_MSI_NOTE}{" "}
                    <a href={CLI_MSI_URL} className={`text-(--accent) underline ${FOCUS_RING}`}>
                      MSI installer
                    </a>
                  </p>
                  <p className="text-caption text-(--text-muted)">{DOCS_WSL_NOTE}</p>
                </>
              )}
              {index === 2 && <CommandBlock label="Device login" command={DOCS_DEVICE_LOGIN_COMMAND} />}
              {index === 3 && <CommandBlock label="Enrollment" command={DOCS_ENROLL_COMMAND} />}
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}

function SkillSection(): React.ReactElement {
  return (
    <section aria-labelledby="docs-skill-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-4)">
        <SectionHeading id="docs-skill-heading">Give coding sessions the same access</SectionHeading>
        <p className="text-body max-w-2xl text-(--text-muted)">
          The agent skill is optional: one command gives coding sessions product access through the CLI you already
          signed in.
        </p>
        <CommandBlock label="Agent skill" command={DOCS_SKILL_COMMAND} />
      </div>
    </section>
  );
}

function LoopSection(): React.ReactElement {
  return (
    <section aria-labelledby="docs-loop-heading" className="border-y border-(--border) py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading id="docs-loop-heading">The operating loop</SectionHeading>
        <ul className="grid gap-(--space-6) sm:grid-cols-2 lg:grid-cols-4">
          {DOCS_LOOP_PHASES.map((phase, index) => (
            <li key={phase.title} className="flex flex-col gap-(--space-2)">
              <span aria-hidden="true" className="text-caption font-mono text-(--accent)">
                {String(index + 1).padStart(2, "0")}
              </span>
              <span className="text-body font-medium text-(--text)">{phase.title}</span>
              <span className="text-caption text-(--text-muted)">{phase.body}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}

function TrustSection(): React.ReactElement {
  // The notes grid is a direct child of the section: the parity read
  // counts the closing band as the last section's `:scope > div > div`.
  return (
    <section aria-labelledby="docs-trust-heading" className="flex flex-col gap-(--space-6) py-(--space-10)">
      <SectionHeading id="docs-trust-heading">Built to trust</SectionHeading>
      <div className="grid gap-(--space-6) md:grid-cols-3">
        {DOCS_TRUST_NOTES.map((note) => (
          <div key={note.title} className="flex flex-col gap-(--space-2)">
            <span className="text-body font-medium text-(--text)">{note.title}</span>
            <span className="text-caption text-(--text-muted)">{note.body}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
