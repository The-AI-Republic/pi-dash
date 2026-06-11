<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./pi-symbol-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="./pi-symbol-light.svg" />
    <img alt="Pi Dash" src="./pi-symbol-light.svg" width="120" />
  </picture>
</p>

<p align="center"><b>Pi Dash -- AI 에이전트 오케스트레이션 플랫폼</b></p>

<p align="center">
    <a href="https://pidash.airepublic.com"><b>Live</b></a> •
    <a href="https://airepublic.com/"><b>Website</b></a> •
    <a href="https://airepublic.com/docs"><b>Documentation</b></a> •
    <a href="https://airepublic.com/"><b>Community</b></a> •
    <a href="https://x.com/ai_republic"><b>X</b></a>
</p>

<p align="center">
    <a href="./README.md"><b>English</b></a> •
    <a href="./README.zh-CN.md"><b>简体中文</b></a> •
    <a href="./README.ja.md"><b>日本語</b></a> •
    <a href="./README.pt-BR.md"><b>Português (BR)</b></a> •
    <a href="./README.es.md"><b>Español</b></a> •
    <b>한국어</b>
</p>

Pi Dash는 **As Coding(비동기 바이브 코딩)** 을 위해 만들어진 오픈소스 AI 에이전트 오케스트레이션 플랫폼입니다. As Coding은 무엇을 만들어야 하는지 정의하면 코딩 에이전트가 백그라운드에서 구현을 처리하는 워크플로입니다. 에이전트 실행을 일일이 지켜보거나 터미널이 스크롤되는 것을 바라보는 대신, Pi Dash를 사용하면 정말 중요한 일, 즉 작업 범위 설정, 결과 검토, 그리고 제품 출시에 집중할 수 있습니다.

지금 바로 **[pidash.airepublic.com](https://pidash.airepublic.com)** 에서 사용해 보세요. 별도의 설치가 필요 없습니다.

> Pi Dash는 매일 발전하고 있습니다. 여러분의 제안, 아이디어, 그리고 버그 제보는 저희에게 큰 도움이 됩니다. 언제든지 [GitHub 토론](https://github.com/The-AI-Republic/pi-dash/discussions)을 열거나 [이슈를 등록](https://github.com/The-AI-Republic/pi-dash/issues)해 주세요. 저희는 모든 내용을 읽고 대부분에 답변합니다.

## 🌟 아키텍처

Pi Dash는 세 가지 주요 구성 요소로 이루어져 있습니다:

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./images/pidash_diagram_white.png" />
    <source media="(prefers-color-scheme: light)" srcset="./images/pidash_diagram_black.png" />
    <img alt="Pi Dash architecture diagram" src="./images/pidash_diagram_black.png" width="358" />
  </picture>
</p>

### Pi Dash Platform

프로젝트를 관리하고, 작업을 정의하며, 에이전트 진행 상황을 모니터링하는 웹 기반 오케스트레이션 허브입니다. 작업 항목을 생성하고, 이를 사이클과 모듈로 구성하며, 에이전트 출력물을 검토하고, 분석 데이터를 추적하는 모든 작업을 단일 대시보드에서 수행할 수 있습니다. 터미널을 지켜보는 대신 바로 이곳에서 시간을 보내게 됩니다.

### Pi Dash CLI 및 Runner Daemon

개발 머신에서 실행되는 로컬 커맨드라인 도구이자 백그라운드 데몬입니다. CLI는 Pi Dash 플랫폼에 연결하여 할당된 작업을 가져오고, 이를 구성된 AI 에이전트에 디스패치한 뒤, 결과를 다시 보고합니다. runner 데몬은 이 루프를 지속적으로 유지하므로 각 작업을 수동으로 트리거할 필요가 없습니다.

### AI 에이전트 (사용자 제공)

Pi Dash는 에이전트에 구애받지 않습니다. 직접 사용하는 코딩 에이전트를 가져와 연결하세요. 현재 runner는 **Claude Code**와 **Codex**에 대한 일급(first-class) 지원을 제공하며, 디스패치 계층은 오케스트레이션 모델을 변경하지 않고도 추가 에이전트를 연결할 수 있도록 설계되었습니다. runner가 호출할 에이전트를 구성하기만 하면 나머지는 Pi Dash가 처리합니다.

## 🚀 설치

> **Pi Dash Cloud**가 이제 **[pidash.airepublic.com](https://pidash.airepublic.com)** 에서 정식 서비스됩니다. 직접 인프라를 운영하지 않고 시작하려면 가입하세요. 셀프 호스팅을 선호하시나요? 아래 단계를 따르세요.

### 1. Pi Dash Platform (셀프 호스팅)

#### 요구 사항

- Docker Engine 설치 및 실행 중
- Node.js 버전 22 이상 [LTS 버전](https://nodejs.org/en/about/previous-releases)
- Python 버전 3.12 이상
- Postgres 버전 v15 이상
- Valkey v7 이상 (또는 Redis 7 이상, 드롭인 호환)
- **메모리**: 최소 **12 GB RAM** 권장
  > RAM이 8 GB뿐인 시스템에서 프로젝트를 실행하면 (특히 Docker 컨테이너 빌드/시작 또는 의존성 설치 중에) 설정 실패나 메모리 충돌이 발생할 수 있습니다. 가능하면 GitHub Codespaces와 같은 클라우드 환경을 사용하거나 로컬 RAM을 늘리세요.

#### 설정

1. 저장소 클론

```bash
git clone https://github.com/The-AI-Republic/pi-dash.git [folder-name]
cd [folder-name]
chmod +x setup.sh
```

2. setup.sh 실행

```bash
./setup.sh
```

`setup.sh`는 모든 `.env.example`을 해당 `.env` 파일로 복사하고(저장소 루트 외에 `apps/web`, `apps/api`, `apps/space`, `apps/admin`, `apps/live`), 고유한 Django `SECRET_KEY`를 생성하여 `apps/api/.env`에 추가한 다음, `pnpm install`을 실행합니다. 기본 루프백 개발 설정의 경우 어떤 `.env` 파일도 수동으로 편집할 필요가 없습니다. `.env.example` 기본값(localhost URL, `pi-dash` 데이터베이스 자격 증명, 로컬 MinIO 엔드포인트 등)이 별다른 설정 없이 작동합니다. 기본값이 아닌 호스트/포트에 바인딩하거나 외부 서비스를 연결하는 경우에만 편집하세요.

3. 컨테이너 시작

```bash
docker compose -f docker-compose-local.yml up
```

4. 웹 앱 시작:

```bash
pnpm dev
```

5. 브라우저에서 http://localhost:3001/god-mode/ 를 열고 인스턴스 관리자로 자신을 등록합니다
6. 브라우저에서 http://localhost:3000 을 열고 동일한 자격 증명으로 로그인합니다

#### 프로덕션 배포

(로컬 개발이 아닌) 실제 셀프 호스팅 배포의 경우, 직접 관리하고자 하는 범위에 맞는 방식을 선택하세요:

- **[올인원 Docker 이미지](./deployments/aio/community/README.md)** — 모든 Pi Dash 서비스를 하나로 묶어 내부적으로 `supervisord`가 관리하는 단일 컨테이너입니다. 가장 간단한 방식으로, `docker run` 명령 하나면 됩니다. 데모, 홈랩 구성, 평가, 소규모 팀에 가장 적합합니다. 외부 Postgres / Redis / RabbitMQ / S3 호환 스토리지는 여전히 필요합니다.
- **[Docker Compose / Swarm 셀프 호스팅](./deployments/cli/community/README.md)** — 전체 마이크로서비스 스택입니다(서비스 컨테이너 6개 + 데이터베이스 + 큐 + 스토리지). 구성은 더 복잡하지만 서비스별로 독립적인 스케일링과 롤링 업데이트가 가능합니다. 평가 이상의 용도에는 이 방식을 권장합니다.
- **Kubernetes / Helm** — Helm 차트 배포는 계획되어 있으나 아직 제공되지 않습니다. [`deployments/kubernetes/community/README.md`](./deployments/kubernetes/community/README.md)를 참조하세요.

### 2. Pi Dash CLI 및 Runner Daemon

에이전트가 작업을 가져와 실행하도록 하려는 모든 머신에 CLI를 설치하세요. 현재 지원되는 플랫폼은 다음과 같습니다:

- **macOS** — Apple Silicon (arm64)
- **macOS** — Intel (x86_64)
- **Linux** — arm64 및 x86_64
- **Windows** — x86_64

macOS와 Linux에서는 개발 머신 터미널에서 다음 명령을 실행하세요:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh
```

Windows에서는 MSI 설치 프로그램을 다운로드하여 실행하세요:

<https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi>

특정 버전에 고정하려면(또는 프리릴리스를 설치하려면) `latest`를 태그로 바꾸세요. 예: `.../releases/download/pidash-v0.1.4/pidash-installer.sh`. 프리릴리스는 `/latest/`에서 제외되므로, 위의 한 줄 명령은 항상 마지막 안정 릴리스를 제공합니다. 래퍼, 베어 설치 프로그램, Windows 변형을 포함한 전체 버전 고정 방법은 [`runner/README.md`](./runner/README.md#installing-a-specific-version-pinning--prereleases)에 있습니다.

그런 다음 머신을 인증하고 runner를 등록하세요. 표준 흐름은 두 개의 명령으로 이루어집니다:

```bash
# 1. 브라우저 기반 device-code 로그인 (`gh auth login` / `stripe login`과 유사).
#    CLI 토큰을 ~/.config/pidash/config.toml에 저장합니다.
pidash auth login --url https://your-pidash-instance.com

# 2. 이 호스트를 runner로 등록합니다. 1단계의 토큰을 사용하므로
#    enrollment-token을 붙여넣을 필요가 없습니다. 첫 runner에서는 OS
#    서비스(Linux의 systemd 사용자 유닛, macOS의 launchd 에이전트, 또는
#    Windows의 사용자별 예약 작업)를 설치하고 데몬을 시작합니다.
pidash runner add --project <project-id>
```

`pidash auth login`은 아직 runner가 없을 때 인라인으로 runner 추가 여부를 묻기 때문에, 새 개발 노트북을 단일 명령으로 온보딩할 수 있습니다. 나중에 `pidash runner add --project <other-project-id>`로 runner를 추가하세요.

runner 데몬은 백그라운드에서 실행되며, 할당된 작업을 폴링하고, 이를 AI 에이전트에 디스패치한 뒤, 결과를 플랫폼에 다시 보고합니다.

유용한 명령:

| 명령            | 설명                                                            |
| --------------- | --------------------------------------------------------------- |
| `pidash status` | 서비스 및 데몬 상태 출력                                        |
| `pidash tui`    | 데몬을 모니터링하는 대화형 터미널 UI 열기                       |
| `pidash doctor` | 사전 점검 실행 (에이전트 설치, git 구성, 플랫폼 접근 가능 여부) |
| `pidash stop`   | 데몬 중지                                                       |

사용 가능한 모든 명령은 `pidash --help`를 참조하세요.

### 3. AI 에이전트 (사용자 제공)

Pi Dash는 AI 에이전트를 함께 제공하지 않습니다. 직접 가져와야 합니다. 선택한 에이전트 CLI가 Pi Dash CLI를 실행하는 머신에 설치되어 접근 가능한지 확인하세요. runner는 현재 기본적으로 두 가지 종류의 에이전트를 지원합니다:

- **Claude Code** — [`claude`](https://docs.anthropic.com/en/docs/claude-code)를 설치하고 `claude --version`이 작동하는지 확인하세요.
- **Codex** — [`codex`](https://github.com/openai/codex)를 설치하고 `codex --version`이 작동하는지 확인하세요.

`pidash doctor`는 서비스를 가동하기 전에 구성된 에이전트가 `PATH`에 있는지, 그리고 클라우드에 접근 가능한지 확인합니다.

### 4. 코딩 에이전트용 Pi Dash 스킬 (선택 사항)

[`pi-dash-skill`](https://github.com/The-AI-Republic/pi-dash-skill)은 이식 가능한 에이전트 스킬을 패키징하여 Claude Code나 Codex가 코딩 세션에서 `pidash` CLI를 통해 Pi Dash 이슈를 직접 생성, 나열, 이동, 검사할 수 있게 합니다.

```bash
npx @airepublic/pidash-skill-installer           # Claude Code, Codex, 또는 둘 다에 설치
```

설치 프로그램은 대상(기본값: 모두)을 묻고 필요 시 GitHub에서 스킬을 가져옵니다. 클론이 필요 없습니다. 프롬프트를 건너뛰려면 `--all`, `--claude-code`, 또는 `--codex`를 전달하세요.

Codex 사용자는 Codex 세션 내부에서 기본 제공되는 `$skill-installer`를 통해서도 설치할 수 있습니다. 환경 변수 재정의(`CLAUDE_HOME` / `CODEX_HOME`), 클론 기반 설치 방식, 기타 대안에 대해서는 [pi-dash-skill README](https://github.com/The-AI-Republic/pi-dash-skill#readme)를 참조하세요.

## ⚙️ 사용 기술

[![React Router](https://img.shields.io/badge/-React%20Router-CA4245?logo=react-router&style=for-the-badge&logoColor=white)](https://reactrouter.com/)
[![Django](https://img.shields.io/badge/Django-092E20?style=for-the-badge&logo=django&logoColor=green)](https://www.djangoproject.com/)
[![Node JS](https://img.shields.io/badge/node.js-339933?style=for-the-badge&logo=Node.js&logoColor=white)](https://nodejs.org/en)

## 📝 문서

기능, 설정, 사용법에 대해 알아보려면 [Pi Dash 문서](https://airepublic.com/docs)를 살펴보세요.

## ❤️ 커뮤니티

[GitHub Discussions](https://github.com/The-AI-Republic/pi-dash/discussions)에서 대화에 참여하거나, X에서 [@ai_republic](https://x.com/ai_republic)를 팔로우하거나, 업데이트 소식을 보려면 [airepublic.com](https://airepublic.com/)을 방문하세요. 저희는 모든 커뮤니티 채널에서 [행동 강령](./CODE_OF_CONDUCT.md)을 준수합니다.

질문하기, 버그 제보, 토론 참여, 아이디어 공유, 기능 요청, 또는 여러분의 프로젝트 소개를 언제든지 환영합니다. 여러분의 의견을 기다립니다!

## 🛡️ 보안

Pi Dash에서 보안 취약점을 발견한 경우, 공개 이슈를 등록하는 대신 책임감 있게 제보해 주세요. 자세한 내용은 [SECURITY.md](./SECURITY.md)를 참조하세요.

보안 문제를 알리려면 [privacy_security@airepublic.com](mailto:privacy_security@airepublic.com)으로 이메일을 보내주세요.

## 🤝 기여하기

Pi Dash에 기여할 수 있는 방법은 다양합니다:

- [버그](https://github.com/The-AI-Republic/pi-dash/issues/new)를 제보하거나 기능 요청을 제출하세요.
- 문서를 검토하고 이를 개선하기 위한 풀 리퀘스트를 제출하세요. 오타 수정이든 새로운 내용 추가든 모두 환영합니다.
- [인기 있는 기능 요청](https://github.com/The-AI-Republic/pi-dash/issues)에 투표하여 지지를 보여주세요.

풀 리퀘스트 제출 과정에 대한 자세한 내용은 [CONTRIBUTING.md](./CONTRIBUTING.md)를 읽어주세요.

### 여러분이 없었다면 해낼 수 없었습니다.

<a href="https://github.com/The-AI-Republic/pi-dash/graphs/contributors">
  우리의 커뮤니티 기여자들
</a>

## 감사의 말

Pi Dash는 오픈소스 프로젝트 관리 도구인 [Plane](https://github.com/makeplane/plane) 위에 구축되었습니다. Pi Dash를 가능하게 한 기반을 마련해 준 Plane 팀과 그 기여자들에게 감사드립니다.

## 라이선스

이 프로젝트는 [GNU Affero General Public License v3.0](./LICENSE.txt) 하에 라이선스가 부여됩니다.
