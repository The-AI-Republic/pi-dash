# Pi Dash MCP connector

Pi Dash provides an MCP connector for Claude, ChatGPT, and other compatible AI
clients. It lets an authenticated user review and coordinate work items in the
Pi Dash workspaces they can already access.

## Connection

- MCP server: `https://apipidash.airepublic.com/api/mcp`
- Authentication: OAuth 2.0 authorization code flow with PKCE and a Client ID
  Metadata Document
- Sign-in: the user signs in to Pi Dash and explicitly authorizes the MCP client

The connector uses the signed-in user's existing Pi Dash workspace and project
permissions. It does not grant access to other workspaces or company data.

## Available operations

Read-only tools can:

- return the authenticated Pi Dash user;
- list and retrieve projects, workflow states, and work items;
- search work items;
- read issue comments, workpads, and linked GitHub pull requests.

Write tools can:

- create, update, and move work items;
- add or update issue comments;
- replace an issue's durable agent workpad;
- attach a GitHub pull-request URL to an issue.

Write operations modify only Pi Dash records in a workspace the user can
access. Attaching a pull request stores its public GitHub URL in Pi Dash; the
connector does not call GitHub or change the pull request.

## Example prompts

- “List the open high-priority issues in my Pi Dash project.”
- “Create a bug report in Pi Dash from these reproduction steps.”
- “Add this implementation plan to the issue workpad and post a progress
  comment.”
- “Attach this GitHub pull request to the issue.”

## Review and support

New users receive an isolated Personal workspace with a populated demo project,
which can be used to evaluate read and write tools without company data. For
support, email [business@airepublic.com](mailto:business@airepublic.com).
