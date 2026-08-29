/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useRef, useState } from "react";
import { usePopper } from "react-popper";
import { Combobox } from "@headlessui/react";
import { Check, Cloud, Container } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { SearchIcon, ChevronDownIcon } from "@pi-dash/propel/icons";
import type { IPod } from "@pi-dash/types";
import { ComboDropDown, Spinner } from "@pi-dash/ui";
import { cn } from "@pi-dash/utils";
// components
import { DropdownButton } from "@/components/dropdowns/buttons";
import { BUTTON_VARIANTS_WITH_TEXT } from "@/components/dropdowns/constants";
import type { TDropdownProps } from "@/components/dropdowns/types";
// hooks
import { useDropdown } from "@/hooks/use-dropdown";
// local imports
import { CLOUD_AGENT_VALUE } from "./execution-target";

const matchesQuery = (label: string, query: string) =>
  query === "" || label.toLowerCase().includes(query.toLowerCase());

export type TPodDropdownBaseProps = TDropdownProps & {
  /** When given, the Cloud Agent is offered as a peer of the pods — the issue's
   * execution target rather than just its pod. Disabled (with a reason) when
   * the instance or the viewer cannot run it. */
  cloudAgent?: { available: boolean; reasonCode: string };
  dropdownArrow?: boolean;
  dropdownArrowClassName?: string;
  isInitializing?: boolean;
  onChange: (val: string) => void;
  onClose?: () => void;
  onDropdownOpen?: () => void;
  pods: IPod[];
  renderByDefault?: boolean;
  value: string | undefined | null;
};

export function PodDropdownBase(props: TPodDropdownBaseProps) {
  const {
    buttonClassName,
    buttonContainerClassName,
    buttonVariant,
    cloudAgent,
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

  // derived values
  const isCloudSelected = value === CLOUD_AGENT_VALUE;
  const selectedPod = value && !isCloudSelected ? pods.find((pod) => pod.id === value) : undefined;
  // The field names an execution target once the Cloud Agent can appear in it;
  // "Pod" would be a category error (the Cloud Agent has no pod).
  const fieldLabel = cloudAgent ? t("Runs on") : t("Pod");
  const selectedLabel = isCloudSelected ? t("Pi Dash Cloud Agent") : selectedPod?.name;
  const cloudUnavailableReason =
    cloudAgent?.reasonCode === "llm_config_missing"
      ? t("Configure your AI provider in Pi Dash AI settings to use the Cloud Agent.")
      : t("Unavailable on this instance. Ask an administrator to enable the Cloud Agent.");
  const filteredPods = query === "" ? pods : pods.filter((pod) => pod.name.toLowerCase().includes(query.toLowerCase()));

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
              (isCloudSelected ? (
                <Cloud className="size-3.5 flex-shrink-0" />
              ) : (
                <Container className="size-3.5 flex-shrink-0" />
              ))}
            {BUTTON_VARIANTS_WITH_TEXT.includes(buttonVariant) && (
              <span className="flex-grow truncate text-left">{selectedLabel ?? fieldLabel}</span>
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
            className="my-1 w-48 rounded-sm border-[0.5px] border-strong bg-surface-1 px-2 py-2.5 text-11 shadow-raised-200 focus:outline-none"
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
            <div className="mt-2 max-h-48 space-y-1 overflow-y-scroll">
              {cloudAgent && matchesQuery(t("Pi Dash Cloud Agent"), query) && (
                <Combobox.Option
                  value={CLOUD_AGENT_VALUE}
                  disabled={!cloudAgent.available}
                  className={({ active, selected }) =>
                    cn(
                      "flex w-full items-center justify-between gap-2 truncate rounded-sm px-1 py-1.5 select-none",
                      { "bg-surface-2": active, "text-primary": selected, "text-secondary": !selected },
                      cloudAgent.available ? "cursor-pointer" : "cursor-not-allowed text-placeholder"
                    )
                  }
                >
                  {({ selected }) => (
                    <>
                      <div className="flex min-w-0 items-center gap-2">
                        <Cloud className="size-3.5 flex-shrink-0" />
                        <div className="min-w-0">
                          <p className="truncate">{t("Pi Dash Cloud Agent")}</p>
                          {!cloudAgent.available && (
                            <p className="truncate text-9 text-placeholder">{cloudUnavailableReason}</p>
                          )}
                        </div>
                      </div>
                      {selected && <Check className="size-3.5 flex-shrink-0" />}
                    </>
                  )}
                </Combobox.Option>
              )}
              {filteredPods.length > 0
                ? filteredPods.map((pod) => (
                    <Combobox.Option
                      key={pod.id}
                      value={pod.id}
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
                            <Container className="size-3.5 flex-shrink-0" />
                            <span className="flex-grow truncate">{pod.name}</span>
                            {pod.is_default && (
                              <span className="flex-shrink-0 rounded-sm bg-surface-2 px-1 py-0.5 text-9 text-tertiary uppercase">
                                {t("Default")}
                              </span>
                            )}
                          </div>
                          {selected && <Check className="size-3.5 flex-shrink-0" />}
                        </>
                      )}
                    </Combobox.Option>
                  ))
                : !cloudAgent && <p className="px-1.5 py-1 text-placeholder italic">{t("No matching results")}</p>}
            </div>
          </div>
        </Combobox.Options>
      )}
    </ComboDropDown>
  );
}
