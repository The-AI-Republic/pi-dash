# Contributing to Pi Dash

Thank you for your interest in contributing to Pi Dash — an open-source AI agent orchestration platform built for **As Coding (asynchronous vibe coding)**. All kinds of contributions are valuable to us. This guide covers how to get started quickly.

## Submitting an issue

Before submitting a new issue, please search the [issues](https://github.com/The-AI-Republic/pi-dash/issues) tab or [discussions](https://github.com/The-AI-Republic/pi-dash/discussions). An existing thread might already address your question or inform you of workarounds.

When reporting a bug, please provide a minimal reproduction scenario using a repository or [Gist](https://gist.github.com/). A reproducible case lets us investigate without lengthy back-and-forth. Include:

- Steps to reproduce
- Expected vs actual behavior
- Relevant versions (Node.js, Python, Docker, browser, etc.)
- 3rd-party libraries or integrations involved

Without a minimal reproduction, we may not be able to investigate, and the issue might not be resolved.

You can open a new issue [here](https://github.com/The-AI-Republic/pi-dash/issues/new).

### Naming conventions for issues

Use a clear, concise title following this format:

- For bugs: `Bug: [short description]`
- For features: `Feature: [short description]`
- For improvements: `Improvement: [short description]`
- For documentation: `Docs: [short description]`

**Examples:**

- `Bug: Agent task status not updating after completion`
- `Docs: Clarify RAM requirement for local setup`
- `Feature: Support custom agent timeout per task`

## Local development

If you haven't already set up Pi Dash locally, follow the [Installation](./README.md#-installation) steps in README.md first.

Once Pi Dash is running, here's what you need to know for development:

### Project structure

The project is a monorepo with the backend API (Django, in `apps/api`) and frontend in a single repo.

### Development workflow

1. Create a feature branch from `main`
2. Make your changes
3. Run linting and formatting before committing:

```bash
pnpm lint
pnpm format
```

4. Ensure tests pass
5. Submit a pull request

## Missing a feature?

If a feature is missing, you can request one by opening a [new issue](https://github.com/The-AI-Republic/pi-dash/issues/new). If you'd like to implement it yourself, please submit a proposal issue first so we can discuss the approach.

## Coding guidelines

To ensure consistency throughout the source code:

- All features or bug fixes must include tests (unit or integration).
- We lint with [OxLint](https://oxc.rs/docs/guide/usage/linter) using the shared `.oxlintrc.json` and format with [oxfmt](https://oxc.rs/docs/guide/usage/formatter) using `.oxfmtrc.json`.

## Ways to contribute

- Try Pi Dash (cloud or self-hosted) and give feedback
- Add new agent integrations or connectors
- Add or update translations
- Help with open [issues](https://github.com/The-AI-Republic/pi-dash/issues) or [create your own](https://github.com/The-AI-Republic/pi-dash/issues/new/choose)
- Share your thoughts and suggestions with us
- Help create tutorials and blog posts
- Request a feature by submitting a proposal
- Report a bug
- **Improve documentation** — fix incomplete or missing [docs](https://airepublic.com/docs), bad wording, examples or explanations

## Contributing to language support

The open-source build ships **English only**. UI copy is wrapped in
`t("Source English text")`, using the English string itself as the message id —
there is no separate key catalogue to keep in sync, and no per-string entry to
add when you introduce new copy.

Self-hosters who want to run Pi Dash in another language can add their own
locale files; the package is built for it, with no code changes required. The
full how-to (locale file shape, registering the language so it appears in the
picker, ICU MessageFormat) lives next to the code in
[`packages/i18n/README.md`](packages/i18n/README.md).

> The upstream multi-language locales and the key-sync / machine-translation
> tooling that maintains them are part of Pi Dash Cloud, layered on top at build
> time, and are not maintained in this repository.

## Need help?

Questions, suggestions, and thoughts are most welcome. Reach us on [GitHub Discussions](https://github.com/The-AI-Republic/pi-dash/discussions) or visit [airepublic.com](https://airepublic.com/).
