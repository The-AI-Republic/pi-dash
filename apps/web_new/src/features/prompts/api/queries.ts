// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompt section reads (parity/specs/agents.md §6, AGT-023). One query
// per kind and scope: the member lists always load, the workspace
// baseline loads for admins only (members never fetch it, so they never
// see its warning — AGT-032). The hook merges the resolved lists into
// browser cards and reports the page-level load state.

import { useQueries, queryOptions } from "@tanstack/react-query";
import { getCompiledPrompt, listPromptSections, type ApiClient, type CompiledPrompt } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import {
  mergePromptSections,
  PROMPT_KINDS,
  type MergedPromptSection,
  type PromptKind,
  type PromptKindSections,
} from "../lib/sections.js";
import { promptReceiptKeys, promptSectionKeys, type PromptSectionScope } from "./keys.js";

export function promptSectionsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  kind: PromptKind,
  scope: PromptSectionScope
) {
  return queryOptions({
    queryKey: promptSectionKeys.list(workspaceSlug, kind, scope),
    queryFn: async (): Promise<PromptKindSections> => {
      const data = await listPromptSections(client, workspaceSlug, { kind, scope });
      return { kind, sections: data.sections };
    },
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** The assembled template for one kind, at the viewer's own scope. */
export function compiledPromptQueryOptions(client: ApiClient, workspaceSlug: string, kind: PromptKind) {
  return queryOptions({
    queryKey: promptReceiptKeys.compiled(workspaceSlug, kind),
    queryFn: () => getCompiledPrompt(client, workspaceSlug, kind, { scope: "user" }),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** The resolved kind lists, skipping queries that have not settled. */
function settledSections(queries: { data: PromptKindSections | undefined }[]): PromptKindSections[] {
  return queries.map((query) => query.data).filter((data) => data !== undefined);
}

export interface PromptSectionLists {
  /** Merged browser cards, in first-seen order. */
  sections: MergedPromptSection[];
  /** A member list still resolving. */
  memberPending: boolean;
  /** The admin baseline still resolving. */
  baselinePending: boolean;
  /** A member list failed: the page banners instead of rendering cards. */
  memberFailed: boolean;
  /** The admin baseline failed: cards render without workspace editing. */
  baselineFailed: boolean;
  /** The baseline resolved and workspace editing may show. */
  baselineLoaded: boolean;
  /** Re-run the member lists (the error banner's retry). */
  refetchMember: () => void;
}

/**
 * Load the per-kind per-scope section lists and merge them into cards.
 * Pass baselineEnabled only for admins: the baseline queries stay
 * disabled for everyone else and never fire.
 */
export function usePromptSectionLists(workspaceSlug: string, baselineEnabled: boolean): PromptSectionLists {
  const client = getAppClient();
  const member = useQueries({
    queries: PROMPT_KINDS.map((kind) => promptSectionsQueryOptions(client, workspaceSlug, kind, "user")),
  });
  const baseline = useQueries({
    queries: PROMPT_KINDS.map((kind) => ({
      ...promptSectionsQueryOptions(client, workspaceSlug, kind, "workspace"),
      enabled: baselineEnabled,
    })),
  });
  const memberFailed = member.some((query) => query.isError);
  const baselineFailed = baselineEnabled && baseline.some((query) => query.isError);
  const baselineLoaded = baselineEnabled && !baselineFailed && baseline.every((query) => query.isSuccess);
  const memberPending = member.some((query) => query.isPending);
  const baselinePending = baselineEnabled && baseline.some((query) => query.isPending);
  const memberSections = settledSections(member);
  const baselineSections: PromptKindSections[] | null = baselineLoaded ? settledSections(baseline) : null;
  return {
    sections: mergePromptSections(memberSections, baselineSections),
    memberPending,
    baselinePending,
    memberFailed,
    baselineFailed,
    baselineLoaded,
    refetchMember: () => {
      for (const query of member) void query.refetch();
    },
  };
}

/** One receipt: the kind's resolved sections plus its assembled template. */
export interface PromptReceiptEntry {
  kind: PromptKind;
  sections: PromptKindSections;
  compiled: CompiledPrompt;
}

export interface PromptReceipts {
  /** Ready receipts in page order; kinds still resolving are absent. */
  entries: PromptReceiptEntry[];
  /** A compiled read or section list is still resolving. */
  pending: boolean;
  /** Any receipt read failed: the page banners instead of rendering. */
  failed: boolean;
  /** Re-run every receipt read (the error banner's retry). */
  refetch: () => void;
}

/**
 * Load the receipt view: for each kind the viewer's resolved sections and
 * the compiled assembled template (which carries the automatic-run variant
 * when overrides exist). Both reads always fire; unlike the sections
 * baseline there is no admin-only half.
 */
export function usePromptReceipts(workspaceSlug: string): PromptReceipts {
  const client = getAppClient();
  const compiled = useQueries({
    queries: PROMPT_KINDS.map((kind) => compiledPromptQueryOptions(client, workspaceSlug, kind)),
  });
  const sections = useQueries({
    queries: PROMPT_KINDS.map((kind) => promptSectionsQueryOptions(client, workspaceSlug, kind, "user")),
  });
  const queries = [...compiled, ...sections];
  const entries: PromptReceiptEntry[] = [];
  PROMPT_KINDS.forEach((kind, index) => {
    const template = compiled[index]?.data;
    const resolved = sections[index]?.data;
    if (template !== undefined && resolved !== undefined) {
      entries.push({ kind, sections: resolved, compiled: template });
    }
  });
  return {
    entries,
    pending: queries.some((query) => query.isPending),
    failed: queries.some((query) => query.isError),
    refetch: () => {
      for (const query of queries) void query.refetch();
    },
  };
}
