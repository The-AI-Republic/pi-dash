/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useRef, useState } from "react";
import { usePopper } from "react-popper";
import { Combobox } from "@headlessui/react";
import { Check, Container, Server } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { SearchIcon, ChevronDownIcon } from "@pi-dash/propel/icons";
import type { IPod, IRunner } from "@pi-dash/types";
import { ComboDropDown, Spinner } from "@pi-dash/ui";
import { cn } from "@pi-dash/utils";
// components
import { DropdownButton } from "@/components/dropdowns/buttons";
import { BUTTON_VARIANTS_WITH_TEXT } from "@/components/dropdowns/constants";
import type { TDropdownProps } from "@/components/dropdowns/types";
// hooks
import { useDropdown } from "@/hooks/use-dropdown";
// local imports
import {
  POD_PREFIX,
  RUNNER_PREFIX,
  runnerDisplayLabel,
  runnerReasonMessage,
  runnerStatusLine,
} from "./execution-target";

const matchesQuery = (label: string, query: string) =>
  query === "" || label.toLowerCase().includes(query.toLowerCase());

export type TPodDropdownBaseProps = TDropdownProps & {
  dropdownArrow?: boolean;
  dropdownArrowClassName?: string;
  isInitializing?: boolean;
  onChange: (val: string) => void;
  onClose?: () => void;
  onDropdownOpen?: () => void;
  pods: IPod[];
  /** Runners in the project, each with eligibility fields when read with assignable_for. */
  runners?: IRunner[];
  renderByDefault?: boolean;
  value: string | undefined | null;
};

export function PodDropdownBase(props: TPodDropdownBaseProps) {
  const {
    buttonClassName,
    buttonContainerClassName,
    buttonVariant,
    className = "",
    disabled = false,
    dropdownArrow = false,
    dropdownArrowClassName = "",
    hideIcon = false,
    isInitializing = false,
    onChange,
    onClose,
    onDropdownOpen,
    placement,
    pods,
    runners = [],
    renderByDefault = true,
    showTooltip = false,
    tabIndex,
    value,
  } = props;
  // refs
  const dropdownRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  // popper-js refs
  const [referenceElement, setReferenceElement] = useState<HTMLButtonElement | null>(null);
  const [popperElement, setPopperElement] = useState<HTMLDivElement | null>(null);
  // states
  const [query, setQuery] = useState("");
  const [isOpen, setIsOpen] = useState(false);
  // i18n
  const { t } = useTranslation();
  // popper-js init
  const { styles, attributes } = usePopper(referenceElement, popperElement, {
    placement: placement ?? "bottom-start",
    modifiers: [{ name: "preventOverflow", options: { padding: 12 } }],
  });
  // dropdown init
  const { handleClose, handleKeyDown, handleOnClick, searchInputKeyDown } = useDropdown({
    dropdownRef,
    inputRef,
    isOpen,
    onClose,
    onOpen: onDropdownOpen,
    query,
    setIsOpen,
    setQuery,
  });

  // derived values — tagged values pod:<id> | runner:<id>
  const isRunnerSelected = value?.startsWith(RUNNER_PREFIX) === true;
  const selectedRunnerId = isRunnerSelected ? value!.slice(RUNNER_PREFIX.length) : null;
  const selectedPodId = value?.startsWith(POD_PREFIX)
    ? value.slice(POD_PREFIX.length)
    : isRunnerSelected
      ? null
      : value;
  const selectedPod = selectedPodId ? pods.find((pod) => pod.id === selectedPodId) : undefined;
  const selectedRunner = selectedRunnerId ? runners.find((r) => r.id === selectedRunnerId) : undefined;
  const fieldLabel = t("Runs on");
  const selectedLabel = selectedRunner
    ? runnerDisplayLabel(selectedRunner)
    : selectedPod
      ? selectedPod.name
      : value
        ? value.startsWith(POD_PREFIX)
          ? pods.find((p) => p.id === value.slice(POD_PREFIX.length))?.name
          : value
        : undefined;
  const selectedPodLabel = selectedRunner
    ? (selectedRunner.pod_detail?.name ?? pods.find((p) => p.id === selectedRunner.pod)?.name)
    : undefined;

  const grouped = useMemo(() => {
    // Group runners by pod in server order; keep pods in server order.
    const byPod = new Map<string, IRunner[]>();
    for (const r of runners) {
      const list = byPod.get(r.pod) ?? [];
      list.push(r);
      byPod.set(r.pod, list);
    }
    return pods.map((pod) => ({ pod, runners: byPod.get(pod.id) ?? [] }));
  }, [pods, runners]);

  const filteredGrouped = useMemo(() => {
    if (query === "") return grouped;
    return grouped
      .map(({ pod, runners: podRunners }) => ({
        pod,
        runners: podRunners.filter((r) => matchesQuery(runnerDisplayLabel(r), query)),
      }))
      .filter(({ pod, runners: podRunners }) => matchesQuery(pod.name, query) || podRunners.length > 0);
  }, [grouped, query]);

  const hasResults = filteredGrouped.length > 0;

  const dropdownOnChange = (val: string) => {
    onChange(val);
    handleClose();
  };

  const comboButton = (
    <button
      tabIndex={tabIndex}
      ref={setReferenceElement}
      type="button"
      className={cn(
        "clickable block h-full max-w-full outline-none",
        {
          "cursor-not-allowed text-secondary": disabled,
          "cursor-pointer": !disabled,
        },
        buttonContainerClassName
      )}
      onClick={handleOnClick}
      disabled={disabled}
    >
      <DropdownButton
        className={buttonClassName}
        isActive={isOpen}
        tooltipHeading={fieldLabel}
        tooltipContent={selectedLabel ?? fieldLabel}
        showTooltip={showTooltip}
        variant={buttonVariant}
        renderToolTipByDefault={renderByDefault}
      >
        {isInitializing ? (
          <Spinner className="h-3.5 w-3.5" />
        ) : (
          <>
            {!hideIcon &&
              (isRunnerSelected ? (
                <Server className="size-3.5 flex-shrink-0" />
              ) : (
                <Container className="size-3.5 flex-shrink-0" />
              ))}
            {BUTTON_VARIANTS_WITH_TEXT.includes(buttonVariant) && (
              <span className="flex-grow truncate text-left">{selectedLabel ?? fieldLabel}</span>
            )}
            {selectedPodLabel && BUTTON_VARIANTS_WITH_TEXT.includes(buttonVariant) && (
              <span className="flex-shrink-0 truncate text-9 text-placeholder">{selectedPodLabel}</span>
            )}
            {dropdownArrow && (
              <ChevronDownIcon className={cn("h-2.5 w-2.5 flex-shrink-0", dropdownArrowClassName)} aria-hidden="true" />
            )}
          </>
        )}
      </DropdownButton>
    </button>
  );

  return (
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- keyboard handling lives on the headless Combobox; matches sibling dropdowns (e.g. state).
    <ComboDropDown
      as="div"
      ref={dropdownRef}
      className={cn("h-full", className)}
      value={value}
      onChange={dropdownOnChange}
      disabled={disabled}
      onKeyDown={handleKeyDown}
      button={comboButton}
      renderByDefault={renderByDefault}
    >
      {isOpen && (
        <Combobox.Options className="fixed z-10" static>
          <div
            className="my-1 w-72 rounded-sm border-[0.5px] border-strong bg-surface-1 px-2 py-2.5 text-11 shadow-raised-200 focus:outline-none"
            ref={setPopperElement}
            style={styles.popper}
            {...attributes.popper}
          >
            <div className="flex items-center gap-1.5 rounded-sm border border-subtle bg-surface-2 px-2">
              <SearchIcon className="h-3.5 w-3.5 text-placeholder" strokeWidth={1.5} />
              <Combobox.Input
                as="input"
                ref={inputRef}
                className="w-full bg-transparent py-1 text-11 text-secondary placeholder:text-placeholder focus:outline-none"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("Search")}
                onKeyDown={searchInputKeyDown}
              />
            </div>
            <div className="mt-2 max-h-64 space-y-1 overflow-y-scroll">
              {hasResults ? (
                filteredGrouped.map(({ pod, runners: podRunners }) => (
                  <div key={pod.id} className="space-y-1">
                    <p className="px-1 pt-1 text-9 font-medium tracking-wide text-placeholder uppercase">
                      {pod.name}
                      {pod.is_default ? ` · ${t("Default")}` : ""}
                    </p>
                    {/* Pod assignment row */}
                    <Combobox.Option
                      value={`${POD_PREFIX}${pod.id}`}
                      className={({ active, selected }) =>
                        cn(
                          "flex w-full cursor-pointer items-center justify-between gap-2 truncate rounded-sm px-1 py-1.5 select-none",
                          { "bg-surface-2": active, "text-primary": selected, "text-secondary": !selected }
                        )
                      }
                    >
                      {({ selected }) => (
                        <>
                          <div className="flex items-center gap-2 truncate">
                            <Container className="size-3.5 flex-shrink-0 text-tertiary" />
                            <span className="flex-grow truncate">{t("Any runner in {pod}", { pod: pod.name })}</span>
                          </div>
                          {selected && <Check className="size-3.5 flex-shrink-0" />}
                        </>
                      )}
                    </Combobox.Option>
                    {/* Runner rows */}
                    {podRunners.map((runner) => {
                      const assignable = runner.assignable ?? true;
                      const code = assignable ? "" : (runner.not_assignable_code ?? "");
                      const label = runnerDisplayLabel(runner);
                      if (!matchesQuery(label, query)) return null;
                      return (
                        <Combobox.Option
                          key={runner.id}
                          value={`${RUNNER_PREFIX}${runner.id}`}
                          disabled={!assignable}
                          className={({ active, selected }) =>
                            cn(
                              "flex w-full items-center justify-between gap-2 rounded-sm px-1 py-1.5 select-none",
                              { "bg-surface-2": active, "text-primary": selected, "text-secondary": !selected },
                              assignable ? "cursor-pointer" : "cursor-not-allowed opacity-50"
                            )
                          }
                        >
                          {({ selected }) => (
                            <>
                              <div className="flex min-w-0 items-center gap-2">
                                <Server className="size-3.5 flex-shrink-0 text-tertiary" />
                                <div className="min-w-0 flex-1">
                                  <p className="truncate">{label}</p>
                                  <p className="truncate text-9 text-placeholder">
                                    {!assignable && code ? runnerReasonMessage(code) : runnerStatusLine(runner)}
                                  </p>
                                </div>
                              </div>
                              {selected && <Check className="size-3.5 flex-shrink-0" />}
                            </>
                          )}
                        </Combobox.Option>
                      );
                    })}
                    {podRunners.length === 0 && (
                      <p className="px-1 py-0.5 text-9 text-placeholder italic">{t("No runners in this pod")}</p>
                    )}
                  </div>
                ))
              ) : (
                <p className="px-1.5 py-1 text-placeholder italic">{t("No matching results")}</p>
              )}
            </div>
          </div>
        </Combobox.Options>
      )}
    </ComboDropDown>
  );
}
