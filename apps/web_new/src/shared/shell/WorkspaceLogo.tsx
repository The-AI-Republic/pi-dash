// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace logo mark (SHELL-061, from parity/specs/shell.md §4): the
// uploaded image when one resolves, else the workspace initial. The
// label names the workspace in both states; a broken image falls back
// to the initial instead of rendering a broken frame.
import * as React from "react";

export interface WorkspaceLogoProps {
  /** Resolved image URL; null renders the initial. */
  src: string | null;
  /** Workspace name: the initial source and the accessible label. */
  name: string;
}

export function WorkspaceLogo({ src, name }: WorkspaceLogoProps): React.ReactElement {
  const [broken, setBroken] = React.useState(false);
  const showImage = src !== null && !broken;
  return (
    <span
      role="img"
      aria-label={showImage ? "Workspace logo" : `${name} workspace`}
      data-testid="workspace-logo"
      className="inline-flex size-6 shrink-0 items-center justify-center overflow-hidden rounded-(--radius-control) bg-(--subtle) text-(--text-muted) select-none"
    >
      {showImage ? (
        <img
          src={src}
          alt=""
          aria-hidden="true"
          className="size-full object-cover"
          onError={() => {
            setBroken(true);
          }}
        />
      ) : (
        <span aria-hidden="true" className="text-caption font-medium uppercase">
          {name.slice(0, 1)}
        </span>
      )}
    </span>
  );
}
