// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
