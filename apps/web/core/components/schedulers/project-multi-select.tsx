/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { usePopper } from "react-popper";
import { useOutsideClickDetector } from "@pi-dash/hooks";
import { useTranslation } from "@pi-dash/i18n";
import { Logo } from "@pi-dash/propel/emoji-icon-picker";
import { ChevronDownIcon } from "@pi-dash/propel/icons";
import type { TLogoProps } from "@pi-dash/types";
import { Badge, Checkbox, Input, Loader } from "@pi-dash/ui";
import { filterProjects } from "@/components/schedulers/install-scheduler-helpers";

/** Minimal project shape the picker needs; `TProject` is structurally assignable. */
export type ProjectOption = {
  id: string;
  name: string;
  identifier?: string | null;
  logo_props?: TLogoProps;
};

type Props = {
  projects: ProjectOption[];
  /** Eligible (not-yet-installed) project ids the user has picked. */
  selectedIds: Set<string>;
  /** Ids already bound to this scheduler. `null` = detection still in flight. */
  installedIds: Set<string> | null;
  /** Receives the next selection set. Installed ids are never added by the picker. */
  onChange: (next: Set<string>) => void;
  disabled?: boolean;
};

/**
 * Collapsed multi-select dropdown of projects. Each row is a checkbox; projects
 * that already have this scheduler installed render checked-and-disabled (you
 * can't uncheck to uninstall here). Search + Select-all operate on the eligible,
 * not-yet-installed rows. The panel is portaled so it can overflow the modal and
 * so its inputs don't submit the surrounding form.
 */
export function ProjectMultiSelect({ projects, selectedIds, installedIds, onChange, disabled = false }: Props) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [isOpen, setIsOpen] = useState(false);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const [referenceElement, setReferenceElement] = useState<HTMLButtonElement | null>(null);
  const [popperElement, setPopperElement] = useState<HTMLDivElement | null>(null);
  const { styles, attributes } = usePopper(referenceElement, popperElement, { placement: "bottom-start" });

  useOutsideClickDetector(containerRef, () => setIsOpen(false));

  // Move focus to the search box when the panel opens, without the static
  // autoFocus attribute (which trips jsx-a11y/no-autofocus).
  useEffect(() => {
    if (isOpen) searchRef.current?.focus();
  }, [isOpen]);

  const isInstalled = (id: string) => installedIds?.has(id) ?? false;

  const filtered = useMemo(() => filterProjects(projects, query), [projects, query]);

  // Eligible = visible (post-filter) and not already installed.
  const eligibleFilteredIds = useMemo(
    () => filtered.filter((p) => !isInstalled(p.id)).map((p) => p.id),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [filtered, installedIds]
  );
  const allEligibleSelected = eligibleFilteredIds.length > 0 && eligibleFilteredIds.every((id) => selectedIds.has(id));

  const selectedCount = useMemo(
    () => [...selectedIds].filter((id) => !isInstalled(id)).length,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selectedIds, installedIds]
  );

  const toggleProject = (id: string) => {
    const next = new Set(selectedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onChange(next);
  };

  const toggleSelectAll = () => {
    const next = new Set(selectedIds);
    if (allEligibleSelected) eligibleFilteredIds.forEach((id) => next.delete(id));
    else eligibleFilteredIds.forEach((id) => next.add(id));
    onChange(next);
  };

  const summary =
    selectedCount > 0
      ? t("{count, plural, one {# project selected} other {# projects selected}}", { count: selectedCount })
      : t("Select projects");

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        ref={setReferenceElement}
        onClick={() => !disabled && setIsOpen((o) => !o)}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        className={`flex w-full items-center justify-between gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-13 ${
          disabled ? "cursor-not-allowed text-secondary" : "cursor-pointer hover:bg-layer-1"
        }`}
      >
        <span className={selectedCount > 0 ? "text-primary" : "text-secondary"}>{summary}</span>
        <ChevronDownIcon
          className={`h-4 w-4 flex-shrink-0 text-secondary transition-transform ${isOpen ? "rotate-180" : ""}`}
          aria-hidden="true"
        />
      </button>

      {isOpen &&
        createPortal(
          <div
            ref={setPopperElement}
            data-prevent-outside-click
            style={{ ...styles.popper, width: referenceElement?.offsetWidth }}
            {...attributes.popper}
            className="shadow-lg z-30 my-1 flex flex-col overflow-hidden rounded-md border border-subtle bg-surface-1"
          >
            <div className="p-2">
              <Input
                ref={searchRef}
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("Search projects…")}
                className="w-full"
              />
            </div>

            <div className="flex items-center justify-between gap-2 px-3 pb-1.5">
              <span className="text-12 text-secondary">
                {selectedCount > 0
                  ? t("{count, plural, one {# project selected} other {# projects selected}}", { count: selectedCount })
                  : t("None selected")}
              </span>
              <button
                type="button"
                onClick={toggleSelectAll}
                disabled={eligibleFilteredIds.length === 0}
                className="text-12 text-accent-primary hover:underline disabled:cursor-not-allowed disabled:text-secondary disabled:no-underline"
              >
                {allEligibleSelected ? t("Clear selection") : t("Select all")}
              </button>
            </div>

            <div className="max-h-64 overflow-y-auto border-t border-subtle">
              {installedIds === null ? (
                <Loader className="flex flex-col gap-2 p-3">
                  <Loader.Item height="36px" />
                  <Loader.Item height="36px" />
                  <Loader.Item height="36px" />
                </Loader>
              ) : filtered.length === 0 ? (
                <p className="px-3 py-6 text-center text-13 text-secondary">{t("No projects match your search.")}</p>
              ) : (
                <ul>
                  {filtered.map((project) => {
                    const installed = isInstalled(project.id);
                    const checked = installed || selectedIds.has(project.id);
                    return (
                      <li key={project.id} className="border-b border-subtle last:border-b-0">
                        <label
                          className={`flex items-center gap-3 px-3 py-2 ${
                            installed ? "cursor-not-allowed opacity-60" : "cursor-pointer hover:bg-layer-1"
                          }`}
                        >
                          <Checkbox checked={checked} disabled={installed} onChange={() => toggleProject(project.id)} />
                          <span className="flex-shrink-0">
                            <Logo logo={project.logo_props} size={14} />
                          </span>
                          <span className="flex min-w-0 flex-col">
                            <span className="truncate text-13 font-medium text-primary">{project.name}</span>
                            {project.identifier && (
                              <span className="truncate text-12 text-secondary">{project.identifier}</span>
                            )}
                          </span>
                          {installed && (
                            <span className="ml-auto flex-shrink-0">
                              <Badge variant="accent-neutral" size="sm">
                                {t("Installed")}
                              </Badge>
                            </span>
                          )}
                        </label>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </div>,
          document.body
        )}
    </div>
  );
}
