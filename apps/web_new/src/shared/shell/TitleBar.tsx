// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Title bar area (Architecture > shared/shell). Presentational: brand and
// workspace crumb on the left, the palette trigger in the middle, the
// signed-in user on the right. Desktop drag regions arrive with NEWFRONT-77.

import { Avatar, Button, Kbd } from "@pidash/kit";
import { Search } from "lucide-react";
import * as React from "react";

export interface TitleBarProps {
  workspaceName: string;
  section: string;
  userLabel: string;
  onOpenPalette: () => void;
}

export function TitleBar({ workspaceName, section, userLabel, onOpenPalette }: TitleBarProps): React.ReactElement {
  return (
    <header className="flex h-12 shrink-0 items-center gap-(--space-4) border-b border-(--border) bg-(--bg) px-(--space-4)">
      <div className="flex min-w-0 items-center gap-(--space-2)" aria-label="Location">
        <span className="text-emphasis text-(--text)">Pi Dash</span>
        <span aria-hidden="true" className="text-(--text-muted)">
          /
        </span>
        <span className="text-body truncate text-(--text-muted)">{workspaceName}</span>
        {section ? (
          <>
            <span aria-hidden="true" className="text-(--text-muted)">
              /
            </span>
            <span className="text-body truncate text-(--text)">{section}</span>
          </>
        ) : null}
      </div>
      <div className="flex flex-1 justify-center">
        <Button variant="secondary" size="small" onClick={onOpenPalette} aria-label="Open command palette">
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="text-(--text-muted)">Commands</span>
          <Kbd keys={["mod", "K"]} />
        </Button>
      </div>
      <div className="flex items-center gap-(--space-2)">
        <Avatar name={userLabel} size="small" />
        <span className="text-body hidden max-w-40 truncate text-(--text-muted) sm:inline">{userLabel}</span>
      </div>
    </header>
  );
}
