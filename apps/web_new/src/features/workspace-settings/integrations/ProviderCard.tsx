// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared provider-card chrome (parity/specs/workspace-settings.md §15–§17).
// One card per git provider: the mark, the provider name, a status line
// that always names the provider, and the card body underneath. Cards
// never frame themselves as integrations settings beyond this page.

import * as React from "react";

export interface ProviderCardProps {
  provider: "github" | "gitlab";
  name: string;
  /** First line of the card: loading, disconnected explainer or connected state. */
  status: string;
  /** Where the card's status message comes from: what the oracle reads. */
  statusDetail?: string | undefined;
  children?: React.ReactNode;
}

const MARKS: Record<ProviderCardProps["provider"], { label: string; className: string }> = {
  github: {
    label: "GH",
    className: "bg-(--subtle) text-(--text)",
  },
  gitlab: {
    label: "GL",
    className: "bg-(--accent) text-(--on-accent)",
  },
};

export function ProviderCard({
  provider,
  name,
  status,
  statusDetail,
  children,
}: ProviderCardProps): React.ReactElement {
  const mark = MARKS[provider];
  const headingId = `integration-card-${provider}`;
  return (
    <section
      aria-labelledby={headingId}
      data-testid={`integration-card-${provider}`}
      className="flex flex-col gap-(--space-4) rounded-(--radius-control) border border-(--border) p-(--space-6)"
    >
      <header className="flex items-start gap-(--space-3)">
        <span
          aria-hidden="true"
          className={`text-meta flex size-9 shrink-0 items-center justify-center rounded-(--radius-control) font-semibold ${mark.className}`}
        >
          {mark.label}
        </span>
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h2 id={headingId} className="text-h3 text-(--text)">
            {name}
          </h2>
          <p className="text-body text-(--text-muted)">{status}</p>
          {statusDetail === undefined ? null : (
            <p className="text-meta text-(--text-muted)" data-testid={`integration-card-${provider}-detail`}>
              {statusDetail}
            </p>
          )}
        </div>
      </header>
      <div className="flex flex-col gap-(--space-3)">{children}</div>
    </section>
  );
}
