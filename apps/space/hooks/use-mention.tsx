/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useRef, useEffect } from "react";
import useSWR from "swr";
// pi dash imports
import { UserService } from "@pi-dash/services";
import type { IUser } from "@pi-dash/types";

export const useMention = () => {
  const userService = new UserService();
  const { data: user, isLoading: userDataLoading } = useSWR("currentUser", async () => userService.me());

  const userRef = useRef<IUser | undefined>();

  useEffect(() => {
    if (userRef) {
      userRef.current = user;
    }
  }, [user]);

  const waitForUserDate = async () =>
    new Promise<IUser>((resolve) => {
      const checkData = () => {
        if (userRef.current) {
          resolve(userRef.current);
        } else {
          setTimeout(checkData, 100);
        }
      };
      checkData();
    });

  const mentionHighlights = async () => {
    if (!userDataLoading && userRef.current) {
      return [userRef.current.id];
    } else {
      const user = await waitForUserDate();
      return [user.id];
    }
  };

  return {
    mentionHighlights,
  };
};
