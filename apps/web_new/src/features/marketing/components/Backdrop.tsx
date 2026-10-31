// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Decorative marketing backdrop (CLOUD-004): a faint globe behind the
// page, hidden from assistive technology and carrying no behavior.
import * as React from "react";

export function MarketingBackdrop(): React.ReactElement {
  return (
    <svg
      aria-hidden="true"
      data-marketing-backdrop=""
      className="pointer-events-none absolute inset-0 z-0 h-full w-full text-(--text) opacity-[0.06]"
      viewBox="0 0 1200 800"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <g stroke="currentColor" strokeWidth="1">
        <circle cx="600" cy="400" r="340" />
        <ellipse cx="600" cy="400" rx="160" ry="340" />
        <ellipse cx="600" cy="400" rx="280" ry="340" />
        <path d="M260 400h680" />
        <path d="M300 250h600" />
        <path d="M300 550h600" />
        <path d="M330 180h540" />
        <path d="M330 620h540" />
        <path d="M600 60v680" />
      </g>
    </svg>
  );
}
