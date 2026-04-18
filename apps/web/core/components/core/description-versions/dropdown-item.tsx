/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash imports
import { useTranslation } from "@apple-pi-dash/i18n";
import type { TDescriptionVersion } from "@apple-pi-dash/types";
import { Avatar, CustomMenu } from "@apple-pi-dash/ui";
import { calculateTimeAgo, getFileURL } from "@apple-pi-dash/utils";
// hooks
import { useMember } from "@/hooks/store/use-member";

type Props = {
  onClick: (versionId: string) => void;
  version: TDescriptionVersion;
};

export const DescriptionVersionsDropdownItem = observer(function DescriptionVersionsDropdownItem(props: Props) {
  const { onClick, version } = props;
  // store hooks
  const { getUserDetails } = useMember();
  // derived values
  const versionCreator = version.owned_by ? getUserDetails(version.owned_by) : null;
  // translation
  const { t } = useTranslation();

  return (
    <CustomMenu.MenuItem key={version.id} className="flex items-center gap-1" onClick={() => onClick(version.id)}>
      <span className="flex-shrink-0">
        <Avatar
          name={versionCreator?.display_name ?? t("common.deactivated_user")}
          size="sm"
          src={getFileURL(versionCreator?.avatar_url ?? "")}
        />
      </span>
      <p className="flex items-center gap-1.5 text-11 text-secondary">
        <span className="font-medium">{versionCreator?.display_name ?? t("common.deactivated_user")}</span>
        <span>{calculateTimeAgo(version.last_saved_at)}</span>
      </p>
    </CustomMenu.MenuItem>
  );
});
