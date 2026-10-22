// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI install section (NEWFRONT-555, RUN-041). Three platform blocks, each
// with its own copy control that confirms for a couple of seconds and toasts
// when the clipboard refuses. The MSI block additionally links the download.

import { Button } from "@pidash/kit";
import { Check, Copy, Download } from "lucide-react";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import {
  COPIED_LABEL,
  COPY_COMMAND_LABEL,
  COPY_CONFIRM_MS,
  COPY_FAILED_BODY,
  COPY_FAILED_TITLE,
  INSTALL_BLOCKS,
  INSTALL_HEADING,
  INSTALL_INTRO,
  INSTALL_PREREQUISITE,
  type InstallBlock,
} from "./install.js";

export interface InstallSectionProps {
  /** Clipboard writer; the browser default is used when omitted. */
  writeClipboard?: ((text: string) => Promise<void>) | undefined;
  /** Confirmation dwell; shortened in tests. */
  confirmMs?: number | undefined;
}

function defaultWrite(text: string): Promise<void> {
  return navigator.clipboard.writeText(text);
}

function InstallCard({
  block,
  writeClipboard,
  confirmMs,
}: {
  block: InstallBlock;
  writeClipboard: (text: string) => Promise<void>;
  confirmMs: number;
}): React.ReactElement {
  const [copied, setCopied] = React.useState(false);
  const timer = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
    },
    []
  );

  async function copy(): Promise<void> {
    try {
      await writeClipboard(block.command);
      setCopied(true);
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), confirmMs);
    } catch {
      notifyError(COPY_FAILED_TITLE, COPY_FAILED_BODY);
    }
  }

  return (
    <div className="rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-3)">
      <div className="mb-(--space-2) flex flex-col gap-(--space-2) sm:flex-row sm:items-center sm:justify-between">
        <span className="text-meta font-medium text-(--text-muted) uppercase">{block.label}</span>
        <div className="flex flex-wrap items-center gap-(--space-2)">
          {block.download === null ? null : (
            <a
              href={block.download}
              target="_blank"
              rel="noreferrer noopener"
              className="text-meta inline-flex h-6 items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-3) text-(--text) hover:bg-(--subtle)"
            >
              <Download size={14} aria-hidden="true" />
              Download MSI
            </a>
          )}
          <Button size="small" variant="secondary" onClick={() => void copy()}>
            {copied ? <Check size={14} aria-hidden="true" /> : <Copy size={14} aria-hidden="true" />}
            {copied ? COPIED_LABEL : COPY_COMMAND_LABEL}
          </Button>
        </div>
      </div>
      <pre className="text-meta font-mono overflow-x-auto rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--text) select-all">
        {block.command}
      </pre>
    </div>
  );
}

export function InstallSection({ writeClipboard, confirmMs }: InstallSectionProps): React.ReactElement {
  const write = writeClipboard ?? defaultWrite;
  const dwell = confirmMs ?? COPY_CONFIRM_MS;
  return (
    <section aria-labelledby="dev-machines-install-heading" className="flex flex-col gap-(--space-3)">
      <h2 id="dev-machines-install-heading" className="text-emphasis font-semibold text-(--text)">
        {INSTALL_HEADING}
      </h2>
      <p className="text-body text-(--text-muted)">{INSTALL_INTRO}</p>
      {INSTALL_BLOCKS.map((block) => (
        <InstallCard key={block.label} block={block} writeClipboard={write} confirmMs={dwell} />
      ))}
      <p className="text-meta text-(--text-muted)">{INSTALL_PREREQUISITE}</p>
    </section>
  );
}
