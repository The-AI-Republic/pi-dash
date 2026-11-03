// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One preferences row (parity/specs/profile.md §7, PROF-016/017). A row is
// a labelled control plus, for the row that opens one, its panel below.
// The trigger carries the stored label so the row reads the server value
// without opening the list; the caller owns the panel's own contents.

import * as React from "react";

export interface PreferenceRowProps {
  /** Row heading and picker label. */
  title: string;
  /** One line under the heading. */
  description: string;
  /** The control, normally the picker trigger. */
  control: React.ReactNode;
  /** Panel shown under the row (the custom designer). */
  children?: React.ReactNode;
}

export function PreferenceRow({ title, description, control, children }: PreferenceRowProps): React.ReactElement {
  return (
    <section data-testid="preference-row" className="flex flex-col gap-(--space-3)">
      <div className="flex flex-col gap-(--space-3) sm:flex-row sm:items-center sm:justify-between">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h2 className="text-emphasis font-medium text-(--text)">{title}</h2>
          <p className="text-meta text-(--text-muted)">{description}</p>
        </div>
        <div className="flex shrink-0 items-center">{control}</div>
      </div>
      {children}
    </section>
  );
}
