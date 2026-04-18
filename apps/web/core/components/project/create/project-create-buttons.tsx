/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useFormContext } from "react-hook-form";
// pi dash imports
import { ETabIndices } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import type { IProject } from "@pi-dash/types";
// ui
// helpers
import { getTabIndex } from "@pi-dash/utils";

type Props = {
  handleClose: () => void;
  isMobile?: boolean;
};

function ProjectCreateButtons(props: Props) {
  const { t } = useTranslation();
  const { handleClose, isMobile = false } = props;
  const {
    formState: { isSubmitting },
  } = useFormContext<IProject>();

  const { getIndex } = getTabIndex(ETabIndices.PROJECT_CREATE, isMobile);

  return (
    <div className="flex justify-end gap-2 border-t border-subtle py-4">
      <Button variant="secondary" size="lg" onClick={handleClose} tabIndex={getIndex("cancel")}>
        {t("common.cancel")}
      </Button>
      <Button variant="primary" size="lg" type="submit" loading={isSubmitting} tabIndex={getIndex("submit")}>
        {isSubmitting ? t("creating") : t("create_project")}
      </Button>
    </div>
  );
}

export default ProjectCreateButtons;
