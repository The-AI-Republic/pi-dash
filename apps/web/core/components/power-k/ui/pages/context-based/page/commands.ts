/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback } from "react";
import { useParams } from "next/navigation";
import { ArchiveIcon, ArchiveRestoreIcon, LockKeyhole, LockKeyholeOpen, Star, StarOff } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
// pi dash imports
import { LinkIcon, GlobeIcon, LockIcon } from "@pi-dash/propel/icons";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { EPageAccess } from "@pi-dash/types";
import { copyTextToClipboard } from "@pi-dash/utils";
// components
import type { TPowerKCommandConfig } from "@/components/power-k/core/types";
// pi dash web imports
import { EPageStoreType, usePageStore } from "@/pi-dash-web/hooks/store";

export const usePowerKPageContextBasedActions = (): TPowerKCommandConfig[] => {
  // navigation
  const { pageId } = useParams();
  // store hooks
  const { getPageById } = usePageStore(EPageStoreType.PROJECT);
  // derived values
  const page = pageId ? getPageById(pageId.toString()) : null;
  const {
    access,
    archived_at,
    canCurrentUserArchivePage,
    canCurrentUserChangeAccess,
    canCurrentUserFavoritePage,
    canCurrentUserLockPage,
    addToFavorites,
    removePageFromFavorites,
    lock,
    unlock,
    makePrivate,
    makePublic,
    archive,
    restore,
  } = page ?? {};
  const isFavorite = !!page?.is_favorite;
  const isLocked = !!page?.is_locked;
  // translation
  const { t } = useTranslation();

  const toggleFavorite = useCallback(() => {
    try {
      if (isFavorite) removePageFromFavorites?.();
      else addToFavorites?.();
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Some error occurred",
      });
    }
  }, [addToFavorites, removePageFromFavorites, isFavorite]);

  const copyPageUrlToClipboard = useCallback(() => {
    const url = new URL(window.location.href);
    copyTextToClipboard(url.href)
      .then(() => {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: t("Page URL copied to clipboard."),
        });
      })
      .catch(() => {
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("Some error occurred while copying the page URL to clipboard."),
        });
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return [
    {
      id: "toggle_page_lock",
      i18n_title: isLocked ? "Unlock" : "Lock",
      icon: isLocked ? LockKeyholeOpen : LockKeyhole,
      group: "contextual",
      contextType: "page",
      type: "action",
      action: () => {
        if (isLocked)
          unlock?.({
            shouldSync: true,
            recursive: true,
          });
        else
          lock?.({
            shouldSync: true,
            recursive: true,
          });
      },
      modifierShortcut: "shift+l",
      isEnabled: () => !!canCurrentUserLockPage,
      isVisible: () => !!canCurrentUserLockPage,
      closeOnSelect: true,
    },
    {
      id: "toggle_page_access",
      i18n_title:
        access === EPageAccess.PUBLIC
          ? "Make private"
          : "Make public",
      icon: access === EPageAccess.PUBLIC ? LockIcon : GlobeIcon,
      group: "contextual",
      contextType: "page",
      type: "action",
      action: () => {
        if (access === EPageAccess.PUBLIC)
          makePrivate?.({
            shouldSync: true,
          });
        else
          makePublic?.({
            shouldSync: true,
          });
      },
      modifierShortcut: "shift+a",
      isEnabled: () => !!canCurrentUserChangeAccess,
      isVisible: () => !!canCurrentUserChangeAccess,
      closeOnSelect: true,
    },
    {
      id: "toggle_page_archive",
      i18n_title: archived_at ? "Restore" : "Archive",
      icon: archived_at ? ArchiveRestoreIcon : ArchiveIcon,
      group: "contextual",
      contextType: "page",
      type: "action",
      action: () => {
        if (archived_at)
          restore?.({
            shouldSync: true,
          });
        else
          archive?.({
            shouldSync: true,
          });
      },
      modifierShortcut: "shift+r",
      isEnabled: () => !!canCurrentUserArchivePage,
      isVisible: () => !!canCurrentUserArchivePage,
      closeOnSelect: true,
    },
    {
      id: "toggle_page_favorite",
      i18n_title: isFavorite
        ? "Remove from favorites"
        : "Add to favorites",
      icon: isFavorite ? StarOff : Star,
      group: "contextual",
      contextType: "page",
      type: "action",
      action: () => toggleFavorite(),
      modifierShortcut: "shift+f",
      isEnabled: () => !!canCurrentUserFavoritePage,
      isVisible: () => !!canCurrentUserFavoritePage,
      closeOnSelect: true,
    },
    {
      id: "copy_page_url",
      i18n_title: "Copy URL",
      icon: LinkIcon,
      group: "contextual",
      contextType: "page",
      type: "action",
      action: copyPageUrlToClipboard,
      modifierShortcut: "cmd+shift+,",
      isEnabled: () => true,
      isVisible: () => true,
      closeOnSelect: true,
    },
  ];
};
