<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./pi-symbol-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="./pi-symbol-light.svg" />
    <img alt="Pi Dash" src="./pi-symbol-light.svg" width="120" />
  </picture>
</p>

<p align="center"><b>Pi Dash -- Plataforma de Orquestación de Agentes de IA</b></p>

<p align="center">
    <a href="https://pidash.airepublic.com"><b>En vivo</b></a> •
    <a href="https://airepublic.com/"><b>Sitio web</b></a> •
    <a href="https://airepublic.com/docs"><b>Documentación</b></a> •
    <a href="https://airepublic.com/"><b>Comunidad</b></a> •
    <a href="https://x.com/ai_republic"><b>X</b></a>
</p>

<p align="center">
    <a href="./README.md"><b>English</b></a> •
    <a href="./README.zh-CN.md"><b>简体中文</b></a> •
    <a href="./README.ja.md"><b>日本語</b></a> •
    <a href="./README.pt-BR.md"><b>Português (BR)</b></a> •
    <b>Español</b> •
    <a href="./README.ko.md"><b>한국어</b></a>
</p>

Pi Dash es una plataforma de orquestación de agentes de IA de código abierto creada para **As Coding (asynchronous vibe coding)**: un flujo de trabajo en el que defines lo que hay que construir y los agentes de codificación se encargan de la implementación en segundo plano. En lugar de supervisar constantemente las ejecuciones de los agentes y observar cómo se desplazan las terminales, Pi Dash te permite centrarte en el trabajo que importa: definir el alcance de las tareas, revisar los resultados y entregar productos.

Pruébalo ahora en **[pidash.airepublic.com](https://pidash.airepublic.com)**: no requiere instalación.

> Pi Dash evoluciona cada día. Tus sugerencias, ideas y los errores que reportas nos ayudan enormemente. No dudes en abrir una [discusión en GitHub](https://github.com/The-AI-Republic/pi-dash/discussions) o [crear un issue](https://github.com/The-AI-Republic/pi-dash/issues). Leemos todo y respondemos a la mayoría.

## 🌟 Arquitectura

Pi Dash se compone de tres componentes principales:

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./images/pidash_diagram_white.png" />
    <source media="(prefers-color-scheme: light)" srcset="./images/pidash_diagram_black.png" />
    <img alt="Pi Dash architecture diagram" src="./images/pidash_diagram_black.png" width="358" />
  </picture>
</p>

### Plataforma Pi Dash

El centro de orquestación basado en web donde gestionas proyectos, defines tareas y supervisas el progreso de los agentes. Crea elementos de trabajo, organízalos en ciclos y módulos, revisa la salida de los agentes y haz seguimiento de las analíticas, todo desde un único panel. Aquí es donde inviertes tu tiempo en lugar de observar terminales.

### Pi Dash CLI y Runner Daemon

Una herramienta de línea de comandos local y un daemon en segundo plano que se ejecutan en tu máquina de desarrollo. La CLI se conecta a la plataforma Pi Dash, toma las tareas asignadas, las despacha a tu agente de IA configurado y reporta los resultados. El runner daemon mantiene este ciclo en marcha de forma continua para que no tengas que activar cada tarea manualmente.

### Agente de IA (proporcionado por el usuario)

Pi Dash es agnóstico respecto al agente: trae tu propio agente de codificación. Hoy, el runner incluye soporte de primera clase para **Claude Code** y **Codex**; la capa de despacho está diseñada de modo que se puedan integrar agentes adicionales sin cambiar el modelo de orquestación. Tú configuras qué agente invoca el runner; Pi Dash se encarga del resto.

## 🚀 Instalación

> **Pi Dash Cloud** ya está disponible en **[pidash.airepublic.com](https://pidash.airepublic.com)**: regístrate para empezar sin necesidad de ejecutar tu propia infraestructura. ¿Prefieres alojarlo tú mismo? Sigue los pasos a continuación.

### 1. Plataforma Pi Dash (autoalojada)

#### Requisitos

- Docker Engine instalado y en ejecución
- Node.js versión 22+ [versión LTS](https://nodejs.org/en/about/previous-releases)
- Python versión 3.12+
- Postgres versión v15+
- Valkey v7+ (o Redis 7+, compatible como reemplazo directo)
- **Memoria**: se recomienda un mínimo de **12 GB de RAM**
  > Ejecutar el proyecto en un sistema con solo 8 GB de RAM puede provocar fallos de configuración o caídas por falta de memoria (especialmente durante la compilación/inicio del contenedor Docker o la instalación de dependencias). Usa entornos en la nube como GitHub Codespaces o amplía la RAM local si es posible.

#### Configuración

1. Clona el repositorio

```bash
git clone https://github.com/The-AI-Republic/pi-dash.git [folder-name]
cd [folder-name]
chmod +x setup.sh
```

2. Ejecuta setup.sh

```bash
./setup.sh
```

`setup.sh` copia cada `.env.example` a su correspondiente `.env` (la raíz del repositorio más `apps/web`, `apps/api`, `apps/space`, `apps/admin`, `apps/live`), genera una `SECRET_KEY` única de Django y la añade a `apps/api/.env`, y luego ejecuta `pnpm install`. Para la configuración predeterminada de desarrollo en loopback no necesitas editar ningún archivo `.env` manualmente: los valores predeterminados de `.env.example` (URLs de localhost, credenciales de la base de datos `pi-dash`, un endpoint local de MinIO, etc.) funcionan sin más. Edítalos solo si vas a enlazar a un host/puerto distinto del predeterminado o a integrar servicios externos.

3. Inicia los contenedores

```bash
docker compose -f docker-compose-local.yml up
```

4. Inicia las aplicaciones web:

```bash
pnpm dev
```

5. Abre tu navegador en http://localhost:3001/god-mode/ y regístrate como administrador de la instancia
6. Abre tu navegador en http://localhost:3000 e inicia sesión con las mismas credenciales

#### Despliegue en producción

Para despliegues autoalojados reales (no de desarrollo local), elige la opción que se ajuste a cuánto quieras gestionar tú mismo:

- **[Imagen Docker todo en uno](./deployments/aio/community/README.md)**: un único contenedor que agrupa todos los servicios de Pi Dash, gestionados internamente por `supervisord`. La opción más sencilla: un solo comando `docker run`. Ideal para demos, configuraciones de homelab, evaluación y equipos pequeños. Sigue siendo necesario contar con Postgres / Redis / RabbitMQ / almacenamiento compatible con S3 externos.
- **[Autoalojamiento con Docker Compose / Swarm](./deployments/cli/community/README.md)**: la pila completa de microservicios (6 contenedores de servicio + base de datos + cola + almacenamiento). Requiere más configuración, pero te ofrece escalado independiente y actualizaciones continuas por servicio. Recomendado para cualquier uso más allá de la evaluación.
- **Kubernetes / Helm**: la publicación del chart de Helm está planificada pero aún no disponible; consulta [`deployments/kubernetes/community/README.md`](./deployments/kubernetes/community/README.md).

### 2. Pi Dash CLI y Runner Daemon

Instala la CLI en cualquier máquina en la que quieras que los agentes tomen y ejecuten tareas. Plataformas actualmente compatibles:

- **macOS**: Apple Silicon (arm64)
- **macOS**: Intel (x86_64)
- **Linux**: arm64 y x86_64
- **Windows**: x86_64

En macOS y Linux, ejecuta el siguiente comando en la terminal de tu máquina de desarrollo:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh
```

En Windows, descarga y ejecuta el instalador MSI:

<https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi>

Para fijar una versión específica (o instalar una preliberación), reemplaza `latest` por la etiqueta, por ejemplo `.../releases/download/pidash-v0.1.4/pidash-installer.sh`. Las preliberaciones se excluyen de `/latest/`, por lo que los comandos de una sola línea anteriores siempre sirven la última versión estable. Las recetas completas de fijación de versiones (wrapper, instalador básico, variantes de Windows) están en [`runner/README.md`](./runner/README.md#installing-a-specific-version-pinning--prereleases).

Luego autentica la máquina y registra un runner. El flujo estándar consta de dos comandos:

```bash
# 1. Inicio de sesión basado en navegador con device-code (como `gh auth login` / `stripe login`).
#    Almacena un token de la CLI en ~/.config/pidash/config.toml.
pidash auth login --url https://your-pidash-instance.com

# 2. Registra este host como runner. Usa el token del paso 1: no
#    hace falta pegar ningún enrollment-token. En el primer runner, instala el
#    servicio del SO (unidad de usuario systemd en Linux, agente launchd en macOS, o una
#    tarea programada por usuario en Windows) e inicia el daemon.
pidash runner add --project <project-id>
```

`pidash auth login` solicita añadir un runner en línea cuando todavía no existe ninguno, de modo que una laptop de desarrollo nueva puede incorporarse con un solo comando. Añade más runners más adelante con `pidash runner add --project <other-project-id>`.

El runner daemon se ejecuta en segundo plano, sondea las tareas asignadas, las despacha a tu agente de IA y reporta los resultados a la plataforma.

Comandos útiles:

| Comando         | Descripción                                                                              |
| --------------- | ---------------------------------------------------------------------------------------- |
| `pidash status` | Muestra el estado del servicio y del daemon                                              |
| `pidash tui`    | Abre una interfaz de terminal interactiva para monitorear el daemon                      |
| `pidash doctor` | Ejecuta comprobaciones previas (agente instalado, git configurado, plataforma accesible) |
| `pidash stop`   | Detiene el daemon                                                                        |

Consulta `pidash --help` para ver todos los comandos disponibles.

### 3. Agente de IA (proporcionado por el usuario)

Pi Dash no incluye un agente de IA: tú aportas el tuyo. Asegúrate de que la CLI del agente elegido esté instalada y accesible en la máquina que ejecuta la CLI de Pi Dash. El runner admite actualmente dos tipos de agente de forma nativa:

- **Claude Code**: instala [`claude`](https://docs.anthropic.com/en/docs/claude-code) y asegúrate de que `claude --version` funcione.
- **Codex**: instala [`codex`](https://github.com/openai/codex) y asegúrate de que `codex --version` funcione.

`pidash doctor` verifica que el agente configurado esté en el `PATH` y que la nube sea accesible antes de ponerte en marcha.

### 4. Skill de Pi Dash para el agente de codificación (opcional)

[`pi-dash-skill`](https://github.com/The-AI-Republic/pi-dash-skill) empaqueta una skill de agente portátil para que Claude Code o Codex puedan crear, listar, mover e inspeccionar issues de Pi Dash directamente desde una sesión de codificación a través de la CLI `pidash`.

```bash
npx @airepublic/pidash-skill-installer           # se instala en Claude Code, Codex o ambos
```

El instalador solicita un destino (predeterminado: todos) y obtiene la skill desde GitHub bajo demanda: no requiere clonar nada. Pasa `--all`, `--claude-code` o `--codex` para omitir la solicitud.

Los usuarios de Codex también pueden instalarla mediante el `$skill-installer` integrado desde dentro de una sesión de Codex. Consulta el [README de pi-dash-skill](https://github.com/The-AI-Republic/pi-dash-skill#readme) para conocer las variables de entorno alternativas (`CLAUDE_HOME` / `CODEX_HOME`), la ruta de instalación basada en clonación y otras alternativas.

## ⚙️ Construido con

[![React Router](https://img.shields.io/badge/-React%20Router-CA4245?logo=react-router&style=for-the-badge&logoColor=white)](https://reactrouter.com/)
[![Django](https://img.shields.io/badge/Django-092E20?style=for-the-badge&logo=django&logoColor=green)](https://www.djangoproject.com/)
[![Node JS](https://img.shields.io/badge/node.js-339933?style=for-the-badge&logo=Node.js&logoColor=white)](https://nodejs.org/en)

## 📝 Documentación

Explora la [documentación de Pi Dash](https://airepublic.com/docs) para conocer sus funciones, configuración y uso.

## ❤️ Comunidad

Únete a la conversación en [GitHub Discussions](https://github.com/The-AI-Republic/pi-dash/discussions), sigue a [@ai_republic](https://x.com/ai_republic) en X, o visita [airepublic.com](https://airepublic.com/) para mantenerte al día. Seguimos un [Código de conducta](./CODE_OF_CONDUCT.md) en todos nuestros canales comunitarios.

No dudes en hacer preguntas, reportar errores, participar en discusiones, compartir ideas, solicitar funciones o mostrar tus proyectos. ¡Nos encantaría saber de ti!

## 🛡️ Seguridad

Si descubres una vulnerabilidad de seguridad en Pi Dash, repórtala de forma responsable en lugar de abrir un issue público. Consulta [SECURITY.md](./SECURITY.md) para más información.

Para divulgar cualquier problema de seguridad, escríbenos a [privacy_security@airepublic.com](mailto:privacy_security@airepublic.com).

## 🤝 Contribuir

Hay muchas maneras de contribuir a Pi Dash:

- Reporta [errores](https://github.com/The-AI-Republic/pi-dash/issues/new) o envía solicitudes de funciones.
- Revisa la documentación y envía pull requests para mejorarla, ya sea corrigiendo erratas o añadiendo nuevo contenido.
- Muestra tu apoyo votando a favor de las [solicitudes de funciones populares](https://github.com/The-AI-Republic/pi-dash/issues).

Por favor, lee [CONTRIBUTING.md](./CONTRIBUTING.md) para conocer los detalles del proceso de envío de pull requests.

### No lo habríamos logrado sin ti.

<a href="https://github.com/The-AI-Republic/pi-dash/graphs/contributors">
  Nuestros colaboradores de la comunidad
</a>

## Agradecimientos

Pi Dash está construido sobre [Plane](https://github.com/makeplane/plane), una herramienta de gestión de proyectos de código abierto. Agradecemos al equipo de Plane y a sus colaboradores por sentar las bases que hicieron posible Pi Dash.

## Licencia

Este proyecto está licenciado bajo la [GNU Affero General Public License v3.0](./LICENSE.txt).
