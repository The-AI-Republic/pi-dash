// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Frame-mounted content pages for /docs, /downloads and /pricing (CLOUD-004
// requires the shared frame on every public page). The page bodies are rows
// CLOUD-006–009 (NEWFRONT-648); this component gives those routes a real,
// crawlable landing page now and 648 replaces the body without touching the
// route, the frame or the metadata convention.
import * as React from "react";

import { metaFor, REPOSITORY_URL, SIGN_IN_PATH } from "../content.js";

export interface MarketingIntroProps {
  path: "/docs" | "/downloads" | "/pricing";
}

export function MarketingIntro({ path }: MarketingIntroProps): React.ReactElement {
  const meta = metaFor(path);
  return (
    <section aria-labelledby="page-heading" className="flex flex-col gap-(--space-6) py-(--space-8)">
      <h1 id="page-heading" className="text-h1 font-semibold text-(--text)">
        {meta.title}
      </h1>
      <p className="text-body max-w-2xl text-(--text-muted)">{meta.description}</p>
      <div className="flex flex-wrap gap-(--space-4)">
        <a
          href={SIGN_IN_PATH}
          className="text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--accent-contrast) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none"
        >
          Sign in
        </a>
        <a
          href={REPOSITORY_URL}
          target="_blank"
          rel="noreferrer noopener"
          className="text-body inline-flex items-center rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-3) font-medium text-(--text) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none"
        >
          Source code
        </a>
      </div>
    </section>
  );
}
