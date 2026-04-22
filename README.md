<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./pi-symbol-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="./pi-symbol-light.svg" />
    <img alt="Pi Dash" src="./pi-symbol-light.svg" width="120" />
  </picture>
</p>

<p align="center"><b>Pi Dash -- AI Agent Orchestration Platform</b></p>

<p align="center">
    <a href="https://airepublic.com/"><b>Website</b></a> •
    <a href="https://airepublic.com/docs"><b>Documentation</b></a> •
    <a href="https://airepublic.com/"><b>Community</b></a> •
    <a href="https://x.com/ai_republic"><b>X</b></a>
</p>

Pi Dash is an open-source AI agent orchestration platform built for **As Coding (asynchronous vibe coding)** — a workflow where you define what needs to be built, and coding agents handle the implementation in the background. Instead of babysitting agent runs and watching terminals scroll, Pi Dash lets you focus on the work that matters: scoping tasks, reviewing results, and shipping products.

> Pi Dash is evolving every day. Your suggestions, ideas, and reported bugs help us immensely. Do not hesitate to open a [GitHub discussion](https://github.com/The-AI-Republic/pi-dash/discussions) or [raise an issue](https://github.com/The-AI-Republic/pi-dash/issues). We read everything and respond to most.

## 🌟 Architecture

Pi Dash is composed of three major components:

### Pi Dash Platform

The web-based orchestration hub where you manage projects, define tasks, and monitor agent progress. Create work items, organize them into cycles and modules, review agent output, and track analytics — all from a single dashboard. This is where you spend your time instead of watching terminals.

### Pi Dash CLI & Runner Daemon

A local command-line tool and background daemon that runs on your development machine. The CLI connects to the Pi Dash platform, picks up assigned tasks, dispatches them to your configured AI agent, and reports results back. The runner daemon keeps this loop going continuously so you don't have to trigger each task manually.

### AI Agent (user-provided)

Pi Dash is agent-agnostic — bring your own coding agent. Whether you use Apple Pi, Claude Code, Codex, OpenClaw, or a custom solution, Pi Dash orchestrates the workflow around it. You configure which agent the runner invokes; Pi Dash handles the rest.

## 🚀 Installation

> **Pi Dash Cloud** is coming soon. For now, self-hosting is the way to go.

### 1. Pi Dash Platform (self-hosted)

#### Requirements

- Docker Engine installed and running
- Node.js version 20+ [LTS version](https://nodejs.org/en/about/previous-releases)
- Python version 3.8+
- Postgres version v14
- Redis version v6.2.7
- **Memory**: Minimum **12 GB RAM** recommended
  > Running the project on a system with only 8 GB RAM may lead to setup failures or memory crashes (especially during Docker container build/start or dependency install). Use cloud environments like GitHub Codespaces or upgrade local RAM if possible.

#### Setup

1. Clone the repo

```bash
git clone https://github.com/The-AI-Republic/pi-dash.git [folder-name]
cd [folder-name]
chmod +x setup.sh
```

2. Run setup.sh

```bash
./setup.sh
```

`setup.sh` copies every `.env.example` to its `.env` counterpart (the repo root plus `apps/web`, `apps/api`, `apps/space`, `apps/admin`, `apps/live`), generates a unique Django `SECRET_KEY` and appends it to `apps/api/.env`, then runs `pnpm install`. For the default loopback-dev setup you do not need to edit any `.env` file manually — the `.env.example` defaults (localhost URLs, `pi-dash` database credentials, a local MinIO endpoint, etc.) work out of the box. Edit them only if you're binding to a non-default host/port or wiring in external services.

3. Start the containers

```bash
docker compose -f docker-compose-local.yml up
```

4. Start web apps:

```bash
pnpm dev
```

5. Open your browser to http://localhost:3001/god-mode/ and register yourself as instance admin
6. Open your browser to http://localhost:3000 and log in using the same credentials

For production deployments, see the [`deployments/`](./deployments) directory for Docker Compose and Kubernetes configurations.

### 2. Pi Dash CLI & Runner Daemon

Install the CLI on any machine where you want agents to pick up and execute tasks. Currently supported platforms:

- **macOS** — Apple Silicon (arm64)
- **Linux** — arm64 and x86_64

> Intel Macs are not in the prebuilt matrix — GitHub retired the `macos-13` runner image. You can still build from source on Intel macOS with Rust 1.93+.

Run the following command in your dev machine terminal:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh
```

Then register with your Pi Dash platform, install the OS service, and start the daemon:

```bash
# Register with your Pi Dash instance (grab the one-time token from the web UI)
pidash configure --url https://your-pidash-instance.com --token <ONE_TIME_CODE>

# Install as a system service (systemd on Linux, launchd on macOS)
pidash install

# Start the daemon
pidash start
```

The runner daemon runs in the background, polls for assigned tasks, dispatches them to your AI agent, and reports results back to the platform.

Useful commands:

| Command         | Description                                                                |
| --------------- | -------------------------------------------------------------------------- |
| `pidash status` | Print service and daemon status                                            |
| `pidash tui`    | Open interactive terminal UI to monitor the daemon                         |
| `pidash doctor` | Run preflight checks (agent installed, git configured, platform reachable) |
| `pidash stop`   | Stop the daemon                                                            |

See `pidash --help` for all available commands.

### 3. AI Agent (user-provided)

Pi Dash does not ship an AI agent — you bring your own. Ensure your chosen agent is installed and accessible on the machine running the Pi Dash CLI. Supported agents include Apple Pi, Claude Code, Codex, OpenClaw, and any custom agent that conforms to the Pi Dash agent interface.

## ⚙️ Built with

[![React Router](https://img.shields.io/badge/-React%20Router-CA4245?logo=react-router&style=for-the-badge&logoColor=white)](https://reactrouter.com/)
[![Django](https://img.shields.io/badge/Django-092E20?style=for-the-badge&logo=django&logoColor=green)](https://www.djangoproject.com/)
[![Node JS](https://img.shields.io/badge/node.js-339933?style=for-the-badge&logo=Node.js&logoColor=white)](https://nodejs.org/en)

## 📝 Documentation

Explore the [Pi Dash documentation](https://airepublic.com/docs) to learn about features, setup, and usage.

## ❤️ Community

Join the conversation on [GitHub Discussions](https://github.com/The-AI-Republic/pi-dash/discussions), follow [@ai_republic](https://x.com/ai_republic) on X, or visit [airepublic.com](https://airepublic.com/) for updates. We follow a [Code of conduct](./CODE_OF_CONDUCT.md) in all our community channels.

Feel free to ask questions, report bugs, participate in discussions, share ideas, request features, or showcase your projects. We'd love to hear from you!

## 🛡️ Security

If you discover a security vulnerability in Pi Dash, please report it responsibly instead of opening a public issue. See [SECURITY.md](./SECURITY.md) for more info.

To disclose any security issues, please email us at [security@airepublic.com](mailto:security@airepublic.com).

## 🤝 Contributing

There are many ways you can contribute to Pi Dash:

- Report [bugs](https://github.com/The-AI-Republic/pi-dash/issues/new) or submit feature requests.
- Review the documentation and submit pull requests to improve it—whether it's fixing typos or adding new content.
- Show your support by upvoting [popular feature requests](https://github.com/The-AI-Republic/pi-dash/issues).

Please read [CONTRIBUTING.md](./CONTRIBUTING.md) for details on the process for submitting pull requests.

### We couldn't have done this without you.

<a href="https://github.com/The-AI-Republic/pi-dash/graphs/contributors">
  Our community contributors
</a>

## Acknowledgements

Pi Dash is built on top of [Plane](https://github.com/makeplane/plane), an open-source project management tool. We are grateful to the Plane team and its contributors for laying the foundation that made Pi Dash possible.

## License

This project is licensed under the [GNU Affero General Public License v3.0](./LICENSE.txt).
