/** Edition seam for preparing a user-triggered agent run. */
export type AgentRunTarget = { workspaceSlug: string; projectId: string; issueId: string };

export async function prepareAgentRun(_target: AgentRunTarget): Promise<void> {}

export async function disposeAgentRuntime(): Promise<void> {}
