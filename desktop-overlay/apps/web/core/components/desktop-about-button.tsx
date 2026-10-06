/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { Info } from "lucide-react";
import { Button } from "@pi-dash/propel/button";
import { IconButton } from "@pi-dash/propel/icon-button";
import { Tooltip } from "@pi-dash/propel/tooltip";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { isDesktop } from "@/services/agent-runtime";

/** One licence or notice file bundled with the app (`about.rs`). */
type BundledNotice = { file: string; component: string; license: string; text: string | null };
type About = { version: string; notices: BundledNotice[] };

interface TauriGlobal {
  core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
}

function tauri(): TauriGlobal {
  return (window as unknown as { __TAURI__: TauriGlobal }).__TAURI__;
}

/**
 * Corner button beside the sidebar's user menu that opens the About dialog:
 * the app version and the licence texts bundled with it. This is where the
 * app meets the notice obligations in `desktop/src-tauri/licenses/README.md`.
 */
export function DesktopAboutButton() {
  const [isOpen, setIsOpen] = useState(false);
  const [about, setAbout] = useState<About | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!isDesktop()) return null;

  const open = async () => {
    setIsOpen(true);
    setError(null);
    try {
      setAbout(await tauri().core.invoke<About>("desktop_about"));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };
  const close = () => setIsOpen(false);

  return (
    <>
      <Tooltip tooltipContent="About Pi Dash" position="top">
        <IconButton
          size="lg"
          variant="ghost"
          icon={Info}
          onClick={() => void open()}
          aria-label="About Pi Dash"
          className="shrink-0"
        />
      </Tooltip>
      <ModalCore isOpen={isOpen} handleClose={close} position={EModalPosition.CENTER} width={EModalWidth.XXXL}>
        <div className="flex flex-col gap-4 p-5">
          <div className="flex flex-col gap-1">
            <h3 className="text-16 font-medium text-primary">About Pi Dash</h3>
            {about && <p className="text-13 text-tertiary">Version {about.version}</p>}
          </div>
          <p className="text-13 text-secondary">
            Pi Dash is free software, licensed under AGPL-3.0-only. It includes an agent engine licensed under
            Apache-2.0. The engine is an upstream release binary that has been modified: it is renamed to
            pidash-agent-engine. The notice below has the details.
          </p>
          {error && <p className="text-13 text-danger-primary">Could not load the licence texts: {error}</p>}
          {about?.notices.map((notice) => (
            <section key={notice.file} className="flex flex-col gap-1.5">
              <h4 className="text-13 font-medium text-primary">
                {notice.component} ({notice.license})
              </h4>
              {notice.text === null ? (
                <p className="text-13 text-tertiary">This build does not include {notice.file}.</p>
              ) : (
                <pre className="max-h-48 overflow-auto rounded-md border border-subtle bg-layer-1 p-3 text-11 whitespace-pre-wrap text-secondary">
                  {notice.text}
                </pre>
              )}
            </section>
          ))}
          <div className="flex justify-end">
            <Button variant="secondary" onClick={close}>
              Close
            </Button>
          </div>
        </div>
      </ModalCore>
    </>
  );
}
