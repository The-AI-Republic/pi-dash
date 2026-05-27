/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { observer } from "mobx-react";
// pi dash helpers
import { useOutsideClickDetector } from "@pi-dash/hooks";
import { PreferencesIcon } from "@pi-dash/propel/icons";
import { ScrollArea } from "@pi-dash/propel/scrollarea";
// components
import { CustomizeNavigationDialog } from "@/components/navigation/customize-navigation-dialog";
// hooks
import { useAppTheme } from "@/hooks/store/use-app-theme";
import useSize from "@/hooks/use-window-size";
// pi dash web components
import { UserMenuRoot } from "@/components/workspace/sidebar/user-menu-root";
import { IconButton } from "@pi-dash/propel/icon-button";
// assets
import piSymbolDark from "@/app/assets/pi-dash-logos/pi-symbol-dark.svg?url";
import piSymbolLight from "@/app/assets/pi-dash-logos/pi-symbol-light.svg?url";

type TSidebarWrapperProps = {
  title: string;
  children: React.ReactNode;
  quickActions?: React.ReactNode;
};

export const SidebarWrapper = observer(function SidebarWrapper(props: TSidebarWrapperProps) {
  const { title, children, quickActions } = props;
  // state
  const [isCustomizeNavDialogOpen, setIsCustomizeNavDialogOpen] = useState(false);
  // store hooks
  const { toggleSidebar, sidebarCollapsed } = useAppTheme();
  const windowSize = useSize();
  // refs
  const ref = useRef<HTMLDivElement>(null);

  useOutsideClickDetector(ref, () => {
    if (sidebarCollapsed === false && window.innerWidth < 768) {
      toggleSidebar();
    }
  });

  useEffect(() => {
    if (windowSize[0] < 768 && !sidebarCollapsed) toggleSidebar();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [windowSize]);

  return (
    <>
      <CustomizeNavigationDialog isOpen={isCustomizeNavDialogOpen} onClose={() => setIsCustomizeNavDialogOpen(false)} />
      <div ref={ref} className="flex h-full w-full animate-fade-in flex-col">
        <div className="flex flex-col gap-3 px-3">
          {/* Workspace switcher and settings */}

          <div className="flex items-center justify-between gap-2 px-2">
            <div className="flex items-center gap-2">
              {title === "Pi Dash" && (
                <>
                  <img
                    src={piSymbolLight}
                    alt="Pi Dash"
                    className="h-5 w-auto object-contain dark:hidden"
                    aria-hidden="true"
                  />
                  <img
                    src={piSymbolDark}
                    alt="Pi Dash"
                    className="hidden h-5 w-auto object-contain dark:block"
                    aria-hidden="true"
                  />
                </>
              )}
              <span className="pt-1 text-16 font-medium text-primary">{title}</span>
            </div>
            <div className="flex items-center gap-2">
              {title === "Pi Dash" && (
                <IconButton
                  size="base"
                  variant="ghost"
                  icon={PreferencesIcon}
                  onClick={() => setIsCustomizeNavDialogOpen(true)}
                />
              )}
            </div>
          </div>
          {/* Quick actions */}
          {quickActions}
        </div>

        <ScrollArea
          orientation="vertical"
          scrollType="hover"
          size="sm"
          rootClassName="size-full overflow-x-hidden overflow-y-auto"
          viewportClassName="flex flex-col gap-3 overflow-x-hidden h-full w-full overflow-y-auto px-3 pt-3 pb-0.5"
        >
          {children}
        </ScrollArea>
        {/* User menu (folds Community in as a sub-item). Only mounted when the
            sidebar is fully visible — when collapsed, TopNavigationRoot renders
            the compact fallback instead, so there is exactly one UserMenuRoot
            on screen at any time. */}
        {!sidebarCollapsed && (
          <div className="flex items-center border-t border-subtle bg-surface-1 px-2 py-2">
            <UserMenuRoot variant="sidebar" />
          </div>
        )}
      </div>
    </>
  );
});
