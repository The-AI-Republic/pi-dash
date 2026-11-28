# Self Hosting

In this guide, we will walk you through the process of setting up a self-hosted environment. Self-hosting allows you to have full control over your applications and data. It's a great way to ensure privacy, control, and customization.

We will cover two main options for setting up your self-hosted environment: using a cloud server or using your desktop. For the cloud server, we will use an AWS EC2 instance. For the desktop, we will use Docker to create a local environment.

> **Just want to try Pi Dash quickly?**
> If you're evaluating Pi Dash or running it on a homelab / small VPS, the
> [All-in-One Docker image](../aio/community/README.md) is a one-container
> shortcut — single `docker run` command instead of docker-compose.
> External Postgres / Redis / RabbitMQ / S3-compatible storage are still
> required. Come back to this guide when you're ready for a full
> production deployment with independent service scaling.

Let's get started!

## Setting up Docker Environment

<details>
  <summary>Option 1 - Using Cloud Server</summary>
  <p>Best way to start is to create EC2 machine on AWS. It must have minimum of 2vCPU and 4GB RAM.</p>
  <p>Run the below command to install docker engine.</p>

`curl -fsSL https://get.docker.com | sh -`

</details>

---

<details>
  <summary>Option 2 - Using Desktop</summary>

#### For Mac

  <ol>
    <li> Download Docker Desktop for Mac from the <a href="https://hub.docker.com/editions/community/docker-ce-desktop-mac/" target="_blank">Docker Hub</a>. </li>
    <li> Double-click the downloaded `.dmg` file and drag the Docker app icon to the Applications folder. </li>
    <li>Open Docker Desktop from the Applications folder. You might be asked to provide your system password to install additional software.</li>
  </ol>

#### For Windows:

  <ol>
    <li>Download Docker Desktop for Windows from the <a href="https://hub.docker.com/editions/community/docker-ce-desktop-windows/" target="_blank">Docker Hub</a>.</li>
    <li>Run the installer and follow the instructions. You might be asked to enable Hyper-V and "Containers" Windows features.</li>
    <li>Open Docker Desktop. You might be asked to log out and log back in, or restart your machine, for changes to take effect.</li>
  </ol>

After installation, you can verify the installation by opening a terminal (Command Prompt on Windows, Terminal app on Mac) and running the command `docker --version`. This should display the installed version of Docker.

</details>

---

## Installing Pi Dash

Installing pi dash is a very easy and minimal step process.

### Prerequisite

- Docker installed and running
- OS with bash scripting enabled (Ubuntu, Linux AMI, macos). Windows systems need to have [gitbash](https://git-scm.com/download/win)
- User context used must have access to docker services. In most cases, use sudo su to switch as root user
- Use the terminal (or gitbash) window to run all the future steps

### Downloading Latest Release

```
mkdir pi-dash-selfhost

cd pi-dash-selfhost
```

#### For _Docker Compose_ based setup

```
curl -fsSL -o setup.sh https://github.com/The-AI-Republic/pi-dash/releases/latest/download/setup.sh

chmod +x setup.sh
```

#### For _Docker Swarm_ based setup

```
curl -fsSL -o setup.sh https://github.com/The-AI-Republic/pi-dash/releases/latest/download/swarm.sh

chmod +x setup.sh
```

---

### Proceed with setup

Above steps will set you ready to install and start pi dash services.

Lets get started by running the `./setup.sh` command.

This will prompt you with the below options.

#### Docker Compose

```bash
Select an Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 1
```

For the 1st time setup, type "1" as action input.

This will create a folder `pi-dash-app` and will download 2 files inside that

- `docker-compose.yaml`
- `pi_dash.env`

Again the `options [1-8]` will be popped up, and this time hit `8` to exit.

#### Docker Swarm

```bash
Select an Action you want to perform:
   1) Deploy Stack
   2) Remove Stack
   3) View Stack Status
   4) Redeploy Stack
   5) Upgrade
   6) View Logs
   7) Exit

Action [3]: 1
```

For the 1st time setup, type "1" as action input.

This will create a create a folder `pi-dash-app` and will download 2 files inside that

- `docker-compose.yaml`
- `pi_dash.env`

Again the `options [1-7]` will be popped up, and this time hit `7` to exit.

---

### Continue with setup - Environment Settings

Before proceeding, we suggest used to review `.env` file and set the values.
Below are the most import keys you must refer to. _<span style="color: #fcba03">You can use any text editor to edit this file</span>_.

> `SECRET_KEY` and `LIVE_SERVER_SECRET_KEY` - These are generated automatically
> during `Install` with `openssl rand -hex 32` and written to `pi_dash.env`.
> Each install gets a unique value. Back up `pi_dash.env` (or at least these
> two values) — losing them will invalidate existing sessions, CSRF tokens, and
> any signed URLs. To rotate, overwrite the value in `pi_dash.env` and restart
> the stack.

> `LISTEN_HTTP_PORT` - This is default set to `80`. Make sure the port you choose to use is not preoccupied. (e.g `LISTEN_HTTP_PORT=8080`)

> `WEB_URL` - This is default set to `http://localhost`. Change this to the FQDN you plan to use along with LISTEN_HTTP_PORT (eg. `https://pi-dash.example.com:8080` or `http://[IP-ADDRESS]:8080`)

> `CORS_ALLOWED_ORIGINS` - This is default set to `http://localhost`. Change this to the FQDN you plan to use along with LISTEN_HTTP_PORT (eg. `https://pi-dash.example.com:8080` or `http://[IP-ADDRESS]:8080`)

There are many other settings you can play with, but we suggest you configure `EMAIL SETTINGS` as it will enable you to invite your teammates onto the platform.

---

### Continue with setup - Start Server (Docker Compose)

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `2` to start the services

```bash
Select a Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 2
```

Expect something like this.  
![Downloading docker images](images/download.png)

Be patient as it might take sometime based on download speed and system configuration. If all goes well, you must see something like this

![Downloading completed](images/started.png)

This is the confirmation that all images were downloaded and the services are up & running.

You have successfully self hosted `Pi Dash` instance. Access the application by going to IP or domain you have configured it (e.g `https://pi-dash.example.com:8080` or `http://[IP-ADDRESS]:8080`)

---

### Stopping the Server / Remove Stack

In case you want to make changes to `pi_dash.env` variables, we suggest you to stop the services before doing that.

#### Docker Compose

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `3` to stop the services

```bash
Select a Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 3
```

If all goes well, you must see something like this

![Stop Services](images/stopped.png)

#### Docker Swarm

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `2` to stop the services

```bash
Select an Action you want to perform:
   1) Deploy Stack
   2) Remove Stack
   3) View Stack Status
   4) Redeploy Stack
   5) Upgrade
   6) View Logs
   7) Exit

Action [3]: 2
```

If all goes well, you will see the confirmation from docker cli

---

### Restarting the Server / Redeploy Stack

In case you want to make changes to `pi_dash.env` variables, without stopping the server or you noticed some abnormalies in services, you can restart the services with `RESTART` / `REDEPLOY` option.

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `4` to restart the services

#### Docker Compose

```bash
Select a Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 4
```

If all goes well, you must see something like this

![Restart Services](images/restart.png)

#### Docker Swarm

```bash
   1) Deploy Stack
   2) Remove Stack
   3) View Stack Status
   4) Redeploy Stack
   5) Upgrade
   6) View Logs
   7) Exit

Action [3]: 4
```

If all goes well, you will see the confirmation from docker cli

---

### Upgrading Pi Dash Version

It is always advised to keep Pi Dash up to date with the latest release.

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `5` to upgrade the release.

#### Docker Compose

```bash
Select a Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 5
```

By choosing this, it will stop the services and then will download the latest `docker-compose.yaml` and `pi_dash.env`.

You must expect the below message

![Alt text](images/upgrade.png)

Once done, choose `8` to exit from prompt.

> It is very important for you to validate the `pi_dash.env` for the new changes.

Once done with making changes in `pi_dash.env` file, jump on to `Start Server`

#### Docker Swarm

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `5` to upgrade the release.

```bash
   1) Deploy Stack
   2) Remove Stack
   3) View Stack Status
   4) Redeploy Stack
   5) Upgrade
   6) View Logs
   7) Exit

Action [3]: 5
```

By choosing this, it will stop the services and then will download the latest `docker-compose.yaml` and `pi_dash.env`.

Once done, choose `7` to exit from prompt.

> It is very important for you to validate the `pi_dash.env` for the new changes.

Once done with making changes in `pi_dash.env` file, jump on to `Redeploy Stack`

#### Database and RabbitMQ credentials

Older `variables.env` templates set `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB`, `PGDATABASE` and the `RABBITMQ_USER` / `RABBITMQ_PASSWORD` / `RABBITMQ_VHOST` values to `pi dash` (with a space). The current template uses `pi-dash`. That matches the `DATABASE_URL` / `AMQP_URL` defaults in `docker-compose.yml`, which the services use when those two variables are blank.

- **Upgrades keep your values.** `Upgrade` carries every value from your existing `pi_dash.env` into the new one, so an install that already works keeps its credentials. Nothing is renamed inside the database or the broker.
- **PostgreSQL and RabbitMQ read these values only when their volume is first created.** Editing them in `pi_dash.env` later does not change the user, password or database stored in the `pgdata` / `rabbitmq_data` volumes.
- **`Start` checks this.** Before the app starts, `setup.sh` starts the bundled database and broker and confirms that they accept the credentials the app will connect with: the `DATABASE_URL` and `AMQP_URL` that Compose resolves for the `api` service (exported shell variables included), a PostgreSQL login by service name with the password (the image trusts loopback connections, so a local login would skip the password), and a RabbitMQ user with permissions on the vhost (`/` when the URL names none). If a data service fails to start, `Start` stops. If one is still not ready after about two minutes (`CREDENTIAL_CHECK_ATTEMPTS` × `CREDENTIAL_CHECK_INTERVAL` seconds), it warns, skips that check and starts the app, which retries its own connection. If a volume was created with other values (for example `pi dash`), it stops and names the variable to fix, instead of leaving the API failing with authentication errors.

To fix a mismatch, keep your volumes and point the app at the credentials they were created with. Encode a space as `%20`:

```env
DATABASE_URL=postgresql://pi%20dash:pi%20dash@pi-dash-db:5432/pi%20dash
AMQP_URL=amqp://pi%20dash:pi%20dash@pi-dash-mq:5672/pi%20dash
```

Then run `Start` again. Only remove the volumes if you mean to start with an empty database.

---

### Object storage image (MinIO)

The bundled object storage service (`pi-dash-minio`) uses **`pgsty/minio`**, a third-party image, rather than an official MinIO image. The trust decision and how to re-pin it are recorded here.

**Why not an official image.** MinIO no longer publishes `minio/minio` on Docker Hub, and `quay.io/minio/minio` refuses anonymous pulls. Either one makes a fresh `docker compose up` fail with a pull error. No official, anonymously pullable MinIO server image was available when this was checked (2026-11-28).

**Why `pgsty/minio`.** It is the community-maintained build of the MinIO server published by the Pigsty project ([github.com/pgsty/minio](https://github.com/pgsty/minio), Docker Hub `pgsty/minio`). It is a drop-in replacement for the old image: same `docker-entrypoint.sh`, runs as root, same `/data` layout, and it ships `mc` (a link to `mcli`), which the `docker-compose-local.yml` bucket bootstrap uses. Existing `uploads` volumes keep working with no `user:` override. Chainguard's free MinIO image was considered and rejected: it only publishes `latest` and runs as non-root, which breaks existing volumes.

**How it is trusted.** The image is pinned by release tag **and** multi-arch digest, so a pull can only ever fetch the exact image that was reviewed:

|           |                                                                                                                    |
| --------- | ------------------------------------------------------------------------------------------------------------------ |
| Image     | `pgsty/minio:RELEASE.2026-08-04T00-00-00Z@sha256:b6bfe7239bfc83fb90d31612d9704d86039dd714f7904b3f1ad68f211e602372` |
| Platforms | `linux/amd64`, `linux/arm64` (OCI image index)                                                                     |
| Verified  | 2026-11-28: Docker Hub resolves the tag to exactly this digest                                                     |
| Used by   | `deployments/cli/community/docker-compose.yml`, `docker-compose.yml`, `docker-compose-local.yml`                   |

The `self-host-checks` CI workflow re-checks on every change that the pinned reference still resolves for both platforms, and that this table names the same digest as the compose files.

**Re-pinning.** Never switch to `latest`. To move to a newer release, pick a tag from Docker Hub and read the upstream release notes. Resolve its digest with `docker buildx imagetools inspect pgsty/minio:<tag>` and confirm that it lists `linux/amd64` and `linux/arm64`. Then update the compose files and this table in the same change. If an official MinIO image becomes anonymously pullable again, prefer it.

---

### View Logs

There would a time when you might want to check what is happening inside the API, Worker or any other container.

Lets again run the `./setup.sh` command. You will again be prompted with the below options.

This time select `6` to view logs.

#### Docker Compose

```bash
Select a Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 6
```

#### Docker Swarm

```bash
   1) Deploy Stack
   2) Remove Stack
   3) View Stack Status
   4) Redeploy Stack
   5) Upgrade
   6) View Logs
   7) Exit

Action [3]: 6
```

#### Service Menu Options for Logs

This will further open sub-menu with list of services

```bash
Select a Service you want to view the logs for:
   1) Web
   2) Space
   3) API
   4) Worker
   5) Beat-Worker
   6) Migrator
   7) Proxy
   8) Redis
   9) Postgres
   10) Minio
   11) RabbitMQ
   0) Back to Main Menu

Service: 3
```

Select any of the service to view the logs e.g. `3`. Expect something similar to this

```bash
api-1  | Waiting for database...
api-1  | Database available!
api-1  | Waiting for database migrations to complete...
api-1  | Waiting for database migrations to complete...
api-1  | Waiting for database migrations to complete...
api-1  | Waiting for database migrations to complete...
api-1  | Waiting for database migrations to complete...
api-1  | Waiting for database migrations to complete...
api-1  | Waiting for database migrations to complete...
api-1  | No migrations Pending. Starting processes ...
api-1  | Instance registered
api-1  | ENABLE_SIGNUP loaded with value from environment variable.
api-1  | ENABLE_EMAIL_PASSWORD loaded with value from environment variable.
api-1  | ENABLE_MAGIC_LINK_LOGIN loaded with value from environment variable.
api-1  | GOOGLE_CLIENT_ID loaded with value from environment variable.
api-1  | GITHUB_CLIENT_ID loaded with value from environment variable.
api-1  | GITHUB_CLIENT_SECRET loaded with value from environment variable.
api-1  | EMAIL_HOST loaded with value from environment variable.
api-1  | EMAIL_HOST_USER loaded with value from environment variable.
api-1  | EMAIL_HOST_PASSWORD loaded with value from environment variable.
api-1  | EMAIL_PORT loaded with value from environment variable.
api-1  | EMAIL_FROM loaded with value from environment variable.
api-1  | EMAIL_USE_TLS loaded with value from environment variable.
api-1  | EMAIL_USE_SSL loaded with value from environment variable.
api-1  | OPENAI_API_KEY loaded with value from environment variable.
api-1  | GPT_ENGINE loaded with value from environment variable.
api-1  | UNSPLASH_ACCESS_KEY loaded with value from environment variable.
api-1  | Checking bucket...
api-1  | Bucket 'uploads' does not exist. Creating bucket...
api-1  | Bucket 'uploads' created successfully.
api-1  | Public read access policy set for bucket 'uploads'.
api-1  | Cache Cleared
api-1  | [2024-05-02 03:56:01 +0000] [1] [INFO] Starting gunicorn 21.2.0
api-1  | [2024-05-02 03:56:01 +0000] [1] [INFO] Listening at: http://0.0.0.0:8000 (1)
api-1  | [2024-05-02 03:56:01 +0000] [1] [INFO] Using worker: uvicorn.workers.UvicornWorker
api-1  | [2024-05-02 03:56:01 +0000] [25] [INFO] Booting worker with pid: 25
api-1  | [2024-05-02 03:56:03 +0000] [25] [INFO] Started server process [25]
api-1  | [2024-05-02 03:56:03 +0000] [25] [INFO] Waiting for application startup.
api-1  | [2024-05-02 03:56:03 +0000] [25] [INFO] ASGI 'lifespan' protocol appears unsupported.
api-1  | [2024-05-02 03:56:03 +0000] [25] [INFO] Application startup complete.

```

To exit this, use `CTRL+C` and then you will land on to the main-menu with the list of actions.

Similarly, you can view the logs of other services.

---

### Backup Data (Docker Compose)

There would a time when you might want to backup your data from docker volumes to external storage like S3 or drives.

Lets again run the `./setup.sh` command. You will again be prompted with the below options. This time select `7` to Backup the data.

```bash
Select a Action you want to perform:
   1) Install (x86_64)
   2) Start
   3) Stop
   4) Restart
   5) Upgrade
   6) View Logs
   7) Backup Data
   8) Exit

Action [2]: 7
```

In response, you can find the backup folder

```bash
Backing Up pi-dash-app_pgdata
Backing Up pi-dash-app_redisdata
Backing Up pi-dash-app_uploads

Backup completed successfully. Backup files are stored in /....../pi-dash-app/backup/20240502-1120
```

---

### Restore Data (Docker Compose)

When you want to restore the previously backed-up data, follow the instructions below.

1. Make sure that Pi Dash-CE is installed, started, and then stopped. This ensures that the Docker volumes are created.

1. Download the restore script using the command below. We suggest downloading it in the same folder as `setup.sh`.

   ```bash
   curl -fsSL -o restore.sh https://github.com/The-AI-Republic/pi-dash/releases/latest/download/restore.sh
   chmod +x restore.sh
   ```

1. Execute the command below to restore your data.

   ```bash
   ./restore.sh <path to backup folder containing *.tar.gz files>
   ```

   As an example, for a backup folder `/opt/pi-dash-selfhost/pi-dash-app/backup/20240722-0914`, expect the response below:

   ```bash
   --------------------------------------------
    ____  _                          /////////
   |  _ \| | __ _ _ __   ___         /////////
   | |_) | |/ _` | '_ \ / _ \   /////    /////
   |  __/| | (_| | | | |  __/   /////    /////
   |_|   |_|\__,_|_| |_|\___|        ////
                                    ////
   --------------------------------------------
   Project management tool from the future
   --------------------------------------------
   Found /opt/pi-dash-selfhost/pi-dash-app/backup/20240722-0914/pgdata.tar.gz
   .....Restoring pi-dash-app_pgdata
   .....Successfully restored volume pi-dash-app_pgdata from pgdata.tar.gz

   Found /opt/pi-dash-selfhost/pi-dash-app/backup/20240722-0914/redisdata.tar.gz
   .....Restoring pi-dash-app_redisdata
   .....Successfully restored volume pi-dash-app_redisdata from redisdata.tar.gz

   Found /opt/pi-dash-selfhost/pi-dash-app/backup/20240722-0914/uploads.tar.gz
   .....Restoring pi-dash-app_uploads
   .....Successfully restored volume pi-dash-app_uploads from uploads.tar.gz


   Restore completed successfully.
   ```

1. Start the Pi Dash instance using `./setup.sh start`.

---

### Restore for Commercial Air-Gapped (Docker Compose)

When you want to restore the previously backed-up data on Pi Dash Commercial Air-Gapped version, follow the instructions below.

1. Download the restore script using the command below

   ```bash
   curl -fsSL -o restore-airgapped.sh https://github.com/The-AI-Republic/pi-dash/releases/latest/download/restore-airgapped.sh
   chmod +x restore-airgapped.sh
   ```

1. Copy the backup folder and the `restore-airgapped.sh` to `Commercial Airgapped Edition` server

1. Make sure that Pi Dash Commercial (Airgapped) is extracted and ready to get started. In case it is running, you would need to stop that.

1. Execute the command below to restore your data.

   ```bash
   ./restore-airgapped.sh <path to backup folder containing *.tar.gz files>
   ```

1. After restoration, you are ready to start Pi Dash Commercial (Airgapped) will all your previously saved data.

---

<details>
   <summary><h2>Upgrading from v0.13.2 to v0.14.x</h2></summary>

This is one time activity for users who are upgrading from v0.13.2 to v0.14.0

As there has been significant changes to Self Hosting process, this step mainly covers the data migration from current (v0.13.2) docker volumes from newly created volumes

> Before we begin with migration, make sure your v0.14.0 was started and then stopped. This is required to know the newly created docker volume names.

Begin with downloading the migration script using below command

```

curl -fsSL -o migrate.sh https://raw.githubusercontent.com/The-AI-Republic/pi-dash/master/deploy/selfhost/migration-0.13-0.14.sh

chmod +x migrate.sh

```

Now run the `./migrate.sh` command and expect the instructions as below

```
******************************************************************

This script is solely for the migration purpose only.
This is a 1 time migration of volume data from v0.13.2 => v0.14.x

Assumption:
1. Postgres data volume name ends with _pgdata
2. Minio data volume name ends with _uploads
3. Redis data volume name ends with _redisdata

Any changes to this script can break the migration.

Before you proceed, make sure you run the below command
to know the docker volumes

docker volume ls -q | grep -i "_pgdata"
docker volume ls -q | grep -i "_uploads"
docker volume ls -q | grep -i "_redisdata"

*******************************************************

Given below list of REDIS volumes, identify the prefix of source and destination volumes leaving "_redisdata"
---------------------
pi-dash-app_redisdata
v0132_redisdata

Provide the Source Volume Prefix :
```

**Open another terminal window**, and run the mentioned 3 command. This may be different for users who have changed the volume names in their previous setup (v0.13.2)

For every command you must see 2 records something like shown in above example of `redisdata`

To move forward, you would need PREFIX of old setup and new setup. As per above example, `v0132` is the prefix of v0.13.2 and `pi-dash-app` is the prefix of v0.14.0 setup

**Back to original terminal window**, _Provide the Source Volume Prefix_ and hit ENTER.

Now you will be prompted to _Provide Destination Volume Prefix_. Provide the value and hit ENTER

```
Provide the Source Volume Prefix : v0132
Provide the Destination Volume Prefix : pi-dash-app
```

In case the suffixes are wrong or the mentioned volumes are not found, you will receive the error shown below. The image below displays an error for source volumes.

![Migrate Error](images/migrate-error.png)

In case of successful migration, it will be a silent exit without error.

Now its time to restart v0.14.0 setup.

</details>
