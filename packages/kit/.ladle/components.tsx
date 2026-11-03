// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Ladle preview wrapper for @pidash/kit stories (NEWFRONT-16). Applies the
// design tokens and the compiled component styles, and renders stories on
// the app background in the UI font. Run `pnpm stories` (builds first so
// dist/kit.css exists) to review.
import * as React from "react";
import "../dist/kit.css";
import "../src/tokens.css";

export const Provider = ({ children }: { children: React.ReactNode }): React.ReactElement => (
  <div
    style={{
      background: "var(--bg)",
      color: "var(--text)",
      fontFamily: "var(--font-ui)",
      fontSize: "var(--text-base-size)",
      lineHeight: "var(--text-base-height)",
      minHeight: "100vh",
      padding: 24,
    }}
  >
    {children}
  </div>
);
