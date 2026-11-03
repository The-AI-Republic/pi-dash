// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar personalization dialog (SHELL-070/071, from
// parity/specs/shell.md §6/11). The personal group toggles and reorders
// personal entries with server persistence (target: Drafts only; a
// stored Your-work pin is ignored, with no migration); the projects
// group switches accordion versus tabbed rendering and caps the listed
// count, where zero means uncapped. The route resolves the personal
// view-model from sidebar preferences (the same query feeds the
// sidebar); display preferences read their own query. Every control
// writes immediately; failures roll back with an error toast. Lazy:
// the route mounts this chunk on first open (size budget).
import { Dialog, Input, Skeleton } from "@pidash/kit";
import * as React from "react";

import {
  isTabbedMode,
  selectProjectListCap,
  useUpdateProjectLimit,
  useUpdateProjectNavMode,
  useWorkspaceProperties,
} from "../../projects/index.js";
import { reorderPatches, useUpdateSidebarPreferences, type PersonalGroupState } from "../api/preferences.js";
import { PersonalEntriesList } from "./PersonalEntriesList.js";

export interface PersonalizeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  /** Personal group view-model; the route resolves it from preferences. */
  personal: PersonalGroupState;
}

/** Below-minimum count flag (SHELL-071); the write still clamps to one. */
export const PROJECT_CAP_MIN_ERROR = "Use at least 1.";

const PERSONAL_LOAD_FAILED = "Couldn't load personal entries.";
const DISPLAY_LOAD_FAILED = "Couldn't load display preferences.";

export function PersonalizeDialog({
  open,
  onOpenChange,
  workspaceSlug,
  personal,
}: PersonalizeDialogProps): React.ReactElement {
  const prefsMutation = useUpdateSidebarPreferences(workspaceSlug);
  const display = useWorkspaceProperties(workspaceSlug);
  const navMode = useUpdateProjectNavMode(workspaceSlug);
  const limit = useUpdateProjectLimit(workspaceSlug);

  const serverLimit = display.data?.navigation_project_limit ?? 0;
  const tabbed = isTabbedMode(display.data?.navigation_control_preference ?? "ACCORDION");
  const capEnabled = selectProjectListCap(display.data?.navigation_project_limit) !== null;

  // In-flight count text. Null follows the server; set while typing so
  // clearing the field does not snap back to the stored count.
  const [draft, setDraft] = React.useState<string | null>(null);
  // Last valid count, for re-enabling the cap after it was switched off.
  const [lastCount, setLastCount] = React.useState(1);

  React.useEffect(() => {
    // Follow the server once it echoes the typed count; a clamped or
    // invalid draft stays put so its error keeps showing.
    setDraft((current) => (current !== null && Number(current) === serverLimit ? null : current));
  }, [serverLimit]);

  React.useEffect(() => {
    if (serverLimit >= 1) setLastCount(serverLimit);
  }, [serverLimit]);

  const minError = draft !== null && (draft === "" || Number(draft) < 1) ? PROJECT_CAP_MIN_ERROR : undefined;

  const togglePersonal = (key: string, enabled: boolean): void => {
    prefsMutation.mutate([{ key, is_pinned: enabled }]);
  };

  const movePersonal = (draggedKey: string, targetKey: string): void => {
    const patches = reorderPatches(
      personal.entries.map((entry) => entry.key),
      draggedKey,
      targetKey
    );
    if (patches !== null) prefsMutation.mutate(patches);
  };

  const toggleCap = (enabled: boolean): void => {
    limit.setLimit(enabled ? lastCount : 0);
  };

  const changeCount = (value: string): void => {
    // Plain digits flow to the server; anything else never survives the
    // field (pastes included). Below-minimum counts flag inline and the
    // write clamps to one; an emptied field flags without writing.
    const digits = value.replace(/\D/g, "");
    setDraft(digits);
    if (digits === "") return;
    const count = Number(digits);
    limit.setLimit(count < 1 ? 1 : count);
  };

  const guardCountKey = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.ctrlKey || event.metaKey) return;
    if (event.key.length === 1 && !/\d/.test(event.key)) event.preventDefault();
  };

  const loadingBody = (
    <div className="flex flex-col gap-(--space-2)" aria-hidden="true">
      <Skeleton height="20px" />
      <Skeleton height="20px" width="60%" />
    </div>
  );

  let personalBody: React.ReactNode;
  if (personal.status === "pending") {
    personalBody = loadingBody;
  } else if (personal.status === "error") {
    personalBody = (
      <p role="alert" className="text-body text-(--text-muted)">
        {PERSONAL_LOAD_FAILED}
      </p>
    );
  } else {
    personalBody = <PersonalEntriesList entries={personal.entries} onToggle={togglePersonal} onMove={movePersonal} />;
  }

  let displayBody: React.ReactNode;
  if (display.isPending) {
    displayBody = loadingBody;
  } else if (display.isError) {
    displayBody = (
      <p role="alert" className="text-body text-(--text-muted)">
        {DISPLAY_LOAD_FAILED}
      </p>
    );
  } else {
    displayBody = (
      <div className="flex flex-col gap-(--space-4)">
        <fieldset>
          <legend className="text-body px-0 text-(--text-muted)">Project navigation</legend>
          <div className="mt-(--space-2) flex flex-col gap-(--space-2)">
            <label className="text-body flex cursor-pointer items-center gap-(--space-2) text-(--text)">
              <input
                type="radio"
                name="project-nav-mode"
                data-testid="project-nav-accordion"
                checked={!tabbed}
                onChange={() => navMode.setMode("ACCORDION")}
                className="size-4 shrink-0 accent-(--accent)"
              />
              Accordion
            </label>
            <label className="text-body flex cursor-pointer items-center gap-(--space-2) text-(--text)">
              <input
                type="radio"
                name="project-nav-mode"
                data-testid="project-nav-tabbed"
                checked={tabbed}
                onChange={() => navMode.setMode("TABBED")}
                className="size-4 shrink-0 accent-(--accent)"
              />
              Tabbed
            </label>
          </div>
        </fieldset>
        <label className="text-body flex cursor-pointer items-center gap-(--space-2) text-(--text)">
          <input
            type="checkbox"
            data-testid="project-cap-toggle"
            checked={capEnabled}
            onChange={(event) => toggleCap(event.target.checked)}
            className="size-4 shrink-0 accent-(--accent)"
          />
          Limit listed projects
        </label>
        {capEnabled ? (
          <Input
            label="Project limit"
            data-testid="project-cap-input"
            type="text"
            inputMode="numeric"
            autoComplete="off"
            value={draft ?? String(serverLimit)}
            {...(minError === undefined ? {} : { error: minError })}
            onChange={(event) => changeCount(event.target.value)}
            onKeyDown={guardCountKey}
          />
        ) : null}
      </div>
    );
  }

  return (
    <Dialog
      title="Personalize sidebar"
      description="Choose which entries appear in your sidebar and how the project list behaves."
      open={open}
      onOpenChange={onOpenChange}
    >
      <div className="flex flex-col gap-(--space-8)">
        <section aria-labelledby="personalize-personal-heading">
          <h3 id="personalize-personal-heading" className="text-emphasis mb-(--space-2) text-(--text)">
            Personal
          </h3>
          {personalBody}
        </section>
        <section aria-labelledby="personalize-projects-heading">
          <h3 id="personalize-projects-heading" className="text-emphasis mb-(--space-2) text-(--text)">
            Projects
          </h3>
          {displayBody}
        </section>
      </div>
    </Dialog>
  );
}
