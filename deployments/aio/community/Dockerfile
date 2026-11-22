ARG PI_DASH_VERSION=v0.27.1
# Interim (NEWPIDASH-380): the exiting migrator runs Django `manage.py
# migrate` from the last published Django image. The backend image is no
# longer published per-release (NEWPIDASH-382), so this tag does not move.
# TODO(NEWPIDASH-380): drop it once `pidash-api migrate up` lands.
ARG BACKEND_VERSION=stable
FROM --platform=$BUILDPLATFORM tonistiigi/binfmt AS binfmt

# Caddy must run on this image's glibc runner, but the published proxy image
# is alpine/musl, so rebuild the same binary here on the glibc builder with
# the same plugins as apps/proxy/Dockerfile.ce (keep the two in step).
FROM caddy:2.10.0-builder AS caddy-builder

RUN xcaddy build \
        --with github.com/caddy-dns/cloudflare@v0.2.1 \
        --with github.com/caddy-dns/digitalocean@04bde2867106aa1b44c2f9da41a285fa02e629c5 \
        --with github.com/mholt/caddy-l4@4d3c80e89c5f80438a3e048a410d5543ff5fb9f4

# CPython 3.12 for the interim exiting migrator only (NEWPIDASH-380): the
# runner below is Debian glibc, so the alpine/musl interpreter from the
# backend image cannot run here. No Python process ever serves traffic.
FROM python:3.12.10-slim-bookworm AS python

FROM airepublic/pi-dash-api:${PI_DASH_VERSION} AS api-img
FROM airepublic/pi-dash-web-new:${PI_DASH_VERSION} AS web-new-img
FROM airepublic/pi-dash-live:${PI_DASH_VERSION} AS live-img
FROM airepublic/pi-dash-backend:${BACKEND_VERSION} AS backend-img

# The Django packages the interim migrator imports, installed on Debian.
# The pins come from the frozen backend image's own requirements, so the
# migrator runs exactly what the last Django publish ran.
FROM python AS pydeps
COPY --from=backend-img /code/requirements.txt /tmp/requirements.txt
COPY --from=backend-img /code/requirements /tmp/requirements
RUN apt-get update && apt-get install -y --no-install-recommends \
        gcc pkg-config libpq-dev libxml2-dev libxslt1-dev \
    && pip install --no-cache-dir -r /tmp/requirements.txt \
    && apt-get purge -y --auto-remove gcc pkg-config libpq-dev libxml2-dev libxslt1-dev \
    && rm -rf /var/lib/apt/lists /tmp/requirements.txt /tmp/requirements

# **************************************************
#  STAGE 1: Runner (active system only: no web, space,
#  admin, beat-worker or RabbitMQ; no Django serving)
# **************************************************
FROM node:22-bookworm-slim AS runner

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    "bash" \
    "ca-certificates" \
    "curl" \
    "hostname" \
    "iproute2" \
    "libssl3" \
    "procps" \
    "supervisor" \
    && rm -rf /var/lib/apt/lists/*

# CPython 3.12 + Django packages for the interim exiting migrator only.
COPY --from=pydeps /usr/local /usr/local
RUN python --version

# pidash-api binary and entrypoints (serve, worker, and the migrator slot
# that NEWPIDASH-380 switches to `pidash-api migrate up`).
COPY --from=api-img /usr/local/bin/pidash-api /usr/local/bin/pidash-api
COPY --from=api-img /code ./api
RUN test -x /usr/local/bin/pidash-api

# web_new static files and the live app (node runs on this glibc base).
COPY --from=web-new-img /usr/share/nginx/html /app/web_new
COPY --from=live-img /app /app/live

# Glibc caddy rebuilt above (plugins match apps/proxy/Dockerfile.ce).
COPY --from=caddy-builder /usr/bin/caddy /usr/bin/caddy
RUN caddy version

COPY dist/Caddyfile /app/proxy/Caddyfile

# Frozen Django tree for the interim exiting migrator only: manage.py, the
# project package, templates and entrypoints. The code is libc-independent;
# backend-img's musl site-packages stay behind (pydeps reinstalled them).
COPY --from=backend-img /code/manage.py /code/package.json ./backend/
COPY --from=backend-img /code/pi_dash ./backend/pi_dash
COPY --from=backend-img /code/templates ./backend/templates
COPY --from=backend-img /code/bin ./backend/bin

COPY start.sh /app/start.sh
COPY dist/pi_dash.env /app/pi_dash.env
COPY supervisor.conf /etc/supervisor/conf.d/supervisor.conf

RUN mkdir -p /app/logs/access && \
    mkdir -p /app/logs/error && \
    mkdir -p /app/data && \
    chmod +x /app/start.sh

VOLUME ['/app/data', '/app/logs']

EXPOSE 80 443

CMD ["/app/start.sh"]
