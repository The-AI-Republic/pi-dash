/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Disclosure, Transition } from "@headlessui/react";
// pi dash imports
import {
  WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS,
  WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS,
} from "@pi-dash/constants";
import { ChevronRightIcon } from "@pi-dash/propel/icons";
import { cn } from "@pi-dash/utils";
// hooks
import useLocalStorage from "@/hooks/use-local-storage";
// pi-dash-web imports
import { SidebarItem } from "@/pi-dash-web/components/workspace/sidebar/sidebar-item";

const MORE_ITEM_KEYS = ["projects", "prompts", "schedulers", "analytics", "archives"];

const MORE_STATIC_KEYS = ["analytics", "archives"];

export const SidebarMoreSection = observer(function SidebarMoreSection() {
  const { setValue: toggleMoreMenu, storedValue: isMoreMenuOpen } = useLocalStorage<boolean>(
    "is_sidebar_more_menu_open",
    true
  );

  const items = MORE_ITEM_KEYS.map(
    (key) => WORKSPACE_SIDEBAR_STATIC_NAVIGATION_ITEMS[key] ?? WORKSPACE_SIDEBAR_DYNAMIC_NAVIGATION_ITEMS[key]
  ).filter((item): item is NonNullable<typeof item> => Boolean(item));

  const toggleListDisclosure = (isOpen: boolean) => {
    toggleMoreMenu(isOpen);
  };

  return (
    <Disclosure as="div" className="flex flex-col" defaultOpen={!!isMoreMenuOpen}>
      <div className="group flex w-full items-center justify-between rounded-sm px-2 py-1.5 text-placeholder hover:bg-layer-transparent-hover">
        <Disclosure.Button
          as="button"
          type="button"
          className="flex w-full items-center gap-1 text-left text-13 font-semibold whitespace-nowrap text-placeholder"
          onClick={() => toggleListDisclosure(!isMoreMenuOpen)}
        >
          <span className="text-13 font-semibold">More</span>
        </Disclosure.Button>
        <div className="pointer-events-none flex items-center opacity-0 group-hover:pointer-events-auto group-hover:opacity-100">
          <Disclosure.Button
            as="button"
            type="button"
            className="flex-shrink-0 rounded-sm p-0.5 hover:bg-layer-1"
            onClick={() => toggleListDisclosure(!isMoreMenuOpen)}
          >
            <ChevronRightIcon
              className={cn("size-3 flex-shrink-0 transition-all", {
                "rotate-90": isMoreMenuOpen,
              })}
            />
          </Disclosure.Button>
        </div>
      </div>
      <Transition
        show={!!isMoreMenuOpen}
        enter="transition duration-100 ease-out"
        enterFrom="transform scale-95 opacity-0"
        enterTo="transform scale-100 opacity-100"
        leave="transition duration-75 ease-out"
        leaveFrom="transform scale-100 opacity-100"
        leaveTo="transform scale-95 opacity-0"
      >
        {isMoreMenuOpen && (
          <Disclosure.Panel as="div" className="flex flex-col gap-0.5" static>
            {items.map((item) => (
              <SidebarItem key={item.key} item={item} additionalStaticItems={MORE_STATIC_KEYS} />
            ))}
          </Disclosure.Panel>
        )}
      </Transition>
    </Disclosure>
  );
});
