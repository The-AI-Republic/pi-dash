#!/bin/bash

BRANCH=${BRANCH:-master}
SCRIPT_DIR=$PWD
SERVICE_FOLDER=pi-dash-app
PI_DASH_INSTALL_DIR=$PWD/$SERVICE_FOLDER
export APP_RELEASE=stable
export DOCKERHUB_USER=airepublic
export PULL_POLICY=${PULL_POLICY:-if_not_present}
export GH_REPO=The-AI-Republic/pi-dash
export RELEASE_DOWNLOAD_URL="https://github.com/$GH_REPO/releases/download"
export FALLBACK_DOWNLOAD_URL="https://raw.githubusercontent.com/$GH_REPO/$BRANCH/deployments/cli/community"

CPU_ARCH=$(uname -m)
OS_NAME=$(uname)
UPPER_CPU_ARCH=$(tr '[:lower:]' '[:upper:]' <<< "$CPU_ARCH")

mkdir -p $PI_DASH_INSTALL_DIR/archive
DOCKER_FILE_PATH=$PI_DASH_INSTALL_DIR/docker-compose.yaml
DOCKER_ENV_PATH=$PI_DASH_INSTALL_DIR/pi_dash.env

function print_header() {
clear

cat <<"EOF"
##+.    ##+    .##-                  
 ######+.######-.######.              
 #######.   -###    +#####+.          
 #######.      +       +######.       
 #######.              .#######       
 #######.              .#######       
  #######       +      .#######       
    .+#####+    ###-   .#######       
        .######.-#####+.+######       
            -##.    -##    .+##
EOF
}

function spinner() {
    local pid=$1
    local delay=.5
    local spinstr='|/-\'

    if ! ps -p "$pid" > /dev/null; then  
        echo "Invalid PID: $pid"  
        return 1  
    fi  
    while ps -p "$pid" > /dev/null; do  
        local temp=${spinstr#?}
        printf " [%c]  " "$spinstr" >&2
        local spinstr=$temp${spinstr%"$temp"}
        sleep $delay
        printf "\b\b\b\b\b\b" >&2
    done
    printf "    \b\b\b\b" >&2
}

function checkLatestRelease(){
    echo "Checking for the latest release..." >&2
    local latest_release=$(curl -sSL https://api.github.com/repos/$GH_REPO/releases/latest |  grep -o '"tag_name": "[^"]*"' | sed 's/"tag_name": "//;s/"//g')
    if [ -z "$latest_release" ]; then
        echo "Failed to check for the latest release. Exiting..." >&2
        exit 1
    fi

    echo $latest_release    
}

function initialize(){
    printf "Please wait while we check the availability of Docker images for the selected release ($APP_RELEASE) with ${UPPER_CPU_ARCH} support." >&2

    if [ "$CUSTOM_BUILD" == "true" ]; then
        echo "" >&2
        echo "" >&2
        echo "${UPPER_CPU_ARCH} images are not available for selected release ($APP_RELEASE)." >&2
        echo "build"
        return 1
    fi

    local IMAGE_NAME=airepublic/pi-dash-proxy
    local IMAGE_TAG=${APP_RELEASE}
    docker manifest inspect "${IMAGE_NAME}:${IMAGE_TAG}" | grep -q "\"architecture\": \"${CPU_ARCH}\"" &
    local pid=$!
    spinner "$pid"
    
    echo "" >&2

    wait "$pid"

    if [ $? -eq 0 ]; then
        echo "Pi Dash supports ${CPU_ARCH}" >&2
        echo "available"
        return 0
    else
        echo "" >&2
        echo "" >&2
        echo "${UPPER_CPU_ARCH} images are not available for selected release ($APP_RELEASE)." >&2
        echo "" >&2
        echo "build"
        return 1
    fi
}
function getEnvValue() {
    local key=$1
    local file=$2

    if [ -z "$key" ] || [ -z "$file" ]; then
        echo "Invalid arguments supplied"
        exit 1
    fi

    if [ -f "$file" ]; then
        grep -q "^$key=" "$file"
        if [ $? -eq 0 ]; then
            local value
            value=$(grep "^$key=" "$file" | cut -d'=' -f2)
            echo "$value"
        else
            echo ""
        fi
    fi
}
function updateEnvFile() {
    local key=$1
    local value=$2
    local file=$3

    if [ -z "$key" ] || [ -z "$value" ] || [ -z "$file" ]; then
        echo "Invalid arguments supplied"
        exit 1
    fi

    if [ -f "$file" ]; then
        # check if key exists in the file
        grep -q "^$key=" "$file"
        if [ $? -ne 0 ]; then
            echo "$key=$value" >> "$file"
            return
        else 
            if [ "$OS_NAME" == "Darwin" ]; then
                value=$(echo "$value" | sed 's/|/\\|/g')
                sed -i '' "s|^$key=.*|$key=$value|g" "$file"
            else
                value=$(echo "$value" | sed 's/\//\\\//g')
                sed -i "s/^$key=.*/$key=$value/g" "$file"
            fi
        fi
    else
        echo "File not found: $file"
        exit 1
    fi
}

function generateSecret() {
    if command -v openssl >/dev/null 2>&1; then
        openssl rand -hex 32
    elif [ -r /dev/urandom ]; then
        LC_ALL=C tr -dc 'a-f0-9' < /dev/urandom | head -c 64
    else
        echo ""
    fi
}

function ensureGeneratedSecret() {
    local key=$1
    local file=$2

    if [ -z "$key" ] || [ -z "$file" ]; then
        echo "ensureGeneratedSecret: invalid arguments" >&2
        exit 1
    fi

    local current
    current=$(getEnvValue "$key" "$file")

    if [ -z "$current" ]; then
        local generated
        generated=$(generateSecret)
        if [ -z "$generated" ]; then
            echo "Failed to auto-generate a value for $key (install openssl or ensure /dev/urandom is readable)." >&2
            echo "Set $key manually in $file and re-run." >&2
            exit 1
        fi
        updateEnvFile "$key" "$generated" "$file"
        echo "Generated a unique $key and stored it in $file" >&2
    fi
}

function updateCustomVariables(){
    echo "Updating custom variables..." >&2
    updateEnvFile "DOCKERHUB_USER" "$DOCKERHUB_USER" "$DOCKER_ENV_PATH"
    updateEnvFile "APP_RELEASE" "$APP_RELEASE" "$DOCKER_ENV_PATH"
    updateEnvFile "PULL_POLICY" "$PULL_POLICY" "$DOCKER_ENV_PATH"
    updateEnvFile "CUSTOM_BUILD" "$CUSTOM_BUILD" "$DOCKER_ENV_PATH"
    echo "Custom variables updated successfully" >&2
}

function syncEnvFile(){
    echo "Syncing environment variables..." >&2
    if [ -f "$PI_DASH_INSTALL_DIR/pi_dash.env.bak" ]; then
        updateCustomVariables
        
        # READ keys of pi_dash.env and update the values from pi_dash.env.bak
        while IFS= read -r line
        do
            # ignore is the line is empty or starts with #
            if [ -z "$line" ] || [[ $line == \#* ]]; then
                continue
            fi
            key=$(echo "$line" | cut -d'=' -f1)
            value=$(getEnvValue "$key" "$PI_DASH_INSTALL_DIR/pi_dash.env.bak")
            if [ -n "$value" ]; then
                updateEnvFile "$key" "$value" "$DOCKER_ENV_PATH"
            fi
        done < "$DOCKER_ENV_PATH"
    fi
    echo "Environment variables synced successfully" >&2
}

function buildYourOwnImage(){
    echo "Building images locally..."

    export DOCKERHUB_USER="mypidash"
    export APP_RELEASE="local"
    export PULL_POLICY="never"
    CUSTOM_BUILD="true"

    # checkout the code to ~/tmp/pi_dash folder and build the images
    local PI_DASH_TEMP_CODE_DIR=~/tmp/pi_dash
    rm -rf $PI_DASH_TEMP_CODE_DIR
    mkdir -p $PI_DASH_TEMP_CODE_DIR
    REPO=https://github.com/$GH_REPO.git
    git clone "$REPO" "$PI_DASH_TEMP_CODE_DIR"  --branch "$BRANCH" --single-branch --depth 1

    cp "$PI_DASH_TEMP_CODE_DIR/deployments/cli/community/build.yml" "$PI_DASH_TEMP_CODE_DIR/build.yml"

    cd "$PI_DASH_TEMP_CODE_DIR" || exit

    /bin/bash -c "$COMPOSE_CMD -f build.yml build --no-cache"  >&2
    if [ $? -ne 0 ]; then
        echo "Build failed. Exiting..."
        exit 1
    fi
    echo "Build completed successfully"
    echo ""
    echo "You can now start the services by running the command: ./setup.sh start"
    echo ""
}

function install() {
    echo "Begin Installing Pi Dash"
    echo ""

    if [ "$APP_RELEASE" == "stable" ]; then
        export APP_RELEASE=$(checkLatestRelease)
    fi

    local build_image=$(initialize)

    if [ "$build_image" == "build" ]; then
        # ask for confirmation to continue building the images
        echo "Do you want to continue with building the Docker images locally?"
        read -p "Continue? [y/N]: " confirm
        if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
            echo "Exiting..."
            exit 0
        fi
    fi

    if [ "$build_image" == "build" ]; then
        download "true"
    else
        download "false"
    fi
}

function download() {
    local LOCAL_BUILD=$1
    cd $SCRIPT_DIR
    TS=$(date +%s)
    if [ -f "$PI_DASH_INSTALL_DIR/docker-compose.yaml" ]
    then
        mv $PI_DASH_INSTALL_DIR/docker-compose.yaml $PI_DASH_INSTALL_DIR/archive/$TS.docker-compose.yaml
    fi

    RESPONSE=$(curl -sSL -H 'Cache-Control: no-cache, no-store' -w "HTTPSTATUS:%{http_code}" "$RELEASE_DOWNLOAD_URL/$APP_RELEASE/docker-compose.yml?$(date +%s)")
    BODY=$(echo "$RESPONSE" | sed -e 's/HTTPSTATUS\:.*//g')
    STATUS=$(echo "$RESPONSE" | tr -d '\n' | sed -e 's/.*HTTPSTATUS://')

    if [ "$STATUS" -eq 200 ]; then
        echo "$BODY" > $PI_DASH_INSTALL_DIR/docker-compose.yaml
    else
        # Fallback to download from the raw github url
        RESPONSE=$(curl -sSL -H 'Cache-Control: no-cache, no-store' -w "HTTPSTATUS:%{http_code}" "$FALLBACK_DOWNLOAD_URL/docker-compose.yml?$(date +%s)")
        BODY=$(echo "$RESPONSE" | sed -e 's/HTTPSTATUS\:.*//g')
        STATUS=$(echo "$RESPONSE" | tr -d '\n' | sed -e 's/.*HTTPSTATUS://')

        if [ "$STATUS" -eq 200 ]; then
            echo "$BODY" > $PI_DASH_INSTALL_DIR/docker-compose.yaml
        else
            echo "Failed to download docker-compose.yml. HTTP Status: $STATUS"
            echo "URL: $RELEASE_DOWNLOAD_URL/$APP_RELEASE/docker-compose.yml"
            mv $PI_DASH_INSTALL_DIR/archive/$TS.docker-compose.yaml $PI_DASH_INSTALL_DIR/docker-compose.yaml
            exit 1
        fi
    fi

    RESPONSE=$(curl -sSL -H 'Cache-Control: no-cache, no-store' -w "HTTPSTATUS:%{http_code}" "$RELEASE_DOWNLOAD_URL/$APP_RELEASE/variables.env?$(date +%s)")
    BODY=$(echo "$RESPONSE" | sed -e 's/HTTPSTATUS\:.*//g')
    STATUS=$(echo "$RESPONSE" | tr -d '\n' | sed -e 's/.*HTTPSTATUS://')

    if [ "$STATUS" -eq 200 ]; then
        echo "$BODY" > $PI_DASH_INSTALL_DIR/variables-upgrade.env
    else
        # Fallback to download from the raw github url
        RESPONSE=$(curl -sSL -H 'Cache-Control: no-cache, no-store' -w "HTTPSTATUS:%{http_code}" "$FALLBACK_DOWNLOAD_URL/variables.env?$(date +%s)")
        BODY=$(echo "$RESPONSE" | sed -e 's/HTTPSTATUS\:.*//g')
        STATUS=$(echo "$RESPONSE" | tr -d '\n' | sed -e 's/.*HTTPSTATUS://')

        if [ "$STATUS" -eq 200 ]; then
            echo "$BODY" > $PI_DASH_INSTALL_DIR/variables-upgrade.env
        else
            echo "Failed to download variables.env. HTTP Status: $STATUS"
            echo "URL: $RELEASE_DOWNLOAD_URL/$APP_RELEASE/variables.env"
            mv $PI_DASH_INSTALL_DIR/archive/$TS.docker-compose.yaml $PI_DASH_INSTALL_DIR/docker-compose.yaml
            exit 1
        fi
    fi

    if [ -f "$DOCKER_ENV_PATH" ];
    then
        cp "$DOCKER_ENV_PATH" "$PI_DASH_INSTALL_DIR/archive/$TS.env"
        cp "$DOCKER_ENV_PATH" "$PI_DASH_INSTALL_DIR/pi_dash.env.bak"
    fi

    mv $PI_DASH_INSTALL_DIR/variables-upgrade.env $DOCKER_ENV_PATH

    syncEnvFile

    # Generate fresh per-install values for any blank secrets. On upgrades
    # syncEnvFile will already have pulled forward the user's existing values,
    # so this only fires on the initial install.
    ensureGeneratedSecret "SECRET_KEY" "$DOCKER_ENV_PATH"
    ensureGeneratedSecret "LIVE_SERVER_SECRET_KEY" "$DOCKER_ENV_PATH"

    if [ "$LOCAL_BUILD" == "true" ]; then
        export DOCKERHUB_USER="mypidash"
        export APP_RELEASE="local"
        export PULL_POLICY="never"
        CUSTOM_BUILD="true"

        buildYourOwnImage

        if [ $? -ne 0 ]; then
            echo ""
            echo "Build failed. Exiting..."
            exit 1
        fi
        updateCustomVariables
    else
        CUSTOM_BUILD="false"
        updateCustomVariables
        /bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH pull --policy always"

        if [ $? -ne 0 ]; then
            echo ""
            echo "Failed to pull the images. Exiting..."
            exit 1
        fi
    fi
    
    echo ""
    echo "Most recent version of Pi Dash is now available for you to use"
    echo ""
    echo "In case of 'Upgrade', please check the 'pi_dash.env 'file for any new variables and update them accordingly"
    echo ""
}
# Connection URLs the services fall back to when pi_dash.env leaves
# DATABASE_URL / AMQP_URL blank (the defaults in docker-compose.yml).
DEFAULT_DATABASE_URL="postgresql://pi-dash:pi-dash@pi-dash-db/pi-dash"
DEFAULT_AMQP_URL="amqp://pi-dash:pi-dash@pi-dash-mq:5672/pi-dash"

# Percent-decoding as Python's urllib.parse.unquote does it, which is what
# Kombu and dj-database-url use: a literal "+" and backslashes stay as they are.
function urlDecode() {
    local s=$1 out="" hex
    while [[ $s == *%* ]]; do
        out+=${s%%[%]*}
        s=${s#*[%]}
        hex=${s:0:2}
        if [[ $hex =~ ^[0-9A-Fa-f]{2}$ ]]; then
            printf -v hex '%b' "\\x$hex"
            out+=$hex
            s=${s:2}
        else
            out+=%
        fi
    done
    printf '%s' "$out$s"
}

# Decodes the body of a JSON string (Compose writes & < > as \u00XX).
function jsonUnescape() {
    local s=$1 out="" c code
    while [[ $s == *\\* ]]; do
        out+=${s%%\\*}
        s=${s#*\\}
        c=${s:0:1}
        s=${s:1}
        case $c in
            n) out+=$'\n' ;;
            t) out+=$'\t' ;;
            r) out+=$'\r' ;;
            b) out+=$'\b' ;;
            f) out+=$'\f' ;;
            u)
                code=${s:0:4}
                s=${s:4}
                if [[ $code =~ ^00[0-7][0-9A-Fa-f]$ ]]; then
                    printf -v c '%b' "\\x${code:2:2}"
                else
                    printf -v c '%b' "\\u$code"
                fi
                out+=$c
                ;;
            *) out+=$c ;;
        esac
    done
    printf '%s' "$out$s"
}

# Prints the value of environment variable $2 for service $1 as Compose
# resolves it (exported variables, then the env file, then the compose
# defaults), which is what the app receives. Fails when Compose cannot print
# its configuration as JSON or the service does not set the variable.
function composeServiceEnv() {
    local service=$1 key=$2 line
    local json
    json=$($COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH config --format json "$service" 2>/dev/null) || return 1
    while IFS= read -r line; do
        if [[ $line =~ ^[[:space:]]*\"$key\":[[:space:]]*\"(.*)\",?[[:space:]]*$ ]]; then
            jsonUnescape "${BASH_REMATCH[1]}"
            return 0
        fi
    done <<< "$json"
    return 1
}

# The URL the app connects with: Compose's resolved value, or, with a Compose
# that cannot print JSON, the env file's value and the compose default.
function appServiceUrl() {
    local key=$1 default=$2 value
    if value=$(composeServiceEnv api "$key"); then
        printf '%s' "$value"
        return
    fi
    value=$(getEnvValue "$key" "$DOCKER_ENV_PATH")
    printf '%s' "${value:-$default}"
}

# Splits scheme://user:pass@host:port/path?query the way Python's urlsplit
# does and prints user, password, host, port and the decoded path, separated
# by the unit separator (\x1f) so empty fields survive `read`.
function splitServiceUrl() {
    local rest=${1#*://}
    local netloc=${rest%%[/?#]*}
    local path=${rest:${#netloc}}
    path=${path#/}
    path=${path%%[?#]*}
    local auth="" hostport=$netloc
    if [[ $netloc == *@* ]]; then
        auth=${netloc%@*}
        hostport=${netloc##*@}
    fi
    local user=${auth%%:*} password=""
    if [[ $auth == *:* ]]; then password=${auth#*:}; fi
    local host=${hostport%%:*} port=""
    if [[ $hostport == *:* ]]; then port=${hostport#*:}; fi
    printf '%s\x1f%s\x1f%s\x1f%s\x1f%s\n' "$(urlDecode "$user")" "$(urlDecode "$password")" "$host" "$port" "$(urlDecode "$path")"
}

function credentialMismatch() {
    local service=$1 detail=$2 variable=$3
    echo "" >&2
    echo "   $service: $detail ❌" >&2
    echo "" >&2
    echo "   $service only reads its user, password and database when its volume" >&2
    echo "   is first created, so this volume was created with other values." >&2
    echo "   Installs created from older templates used 'pi dash' for all of them." >&2
    echo "   Set $variable in $DOCKER_ENV_PATH to the credentials the volume was" >&2
    echo "   created with, then start again. See \"Database and RabbitMQ" >&2
    echo "   credentials\" in the self-hosting README." >&2
    echo "   Stop the data services with: ./setup.sh stop" >&2
}

function serviceNotReady() {
    local service=$1 detail=$2
    echo "" >&2
    echo "   $service did not become ready: $detail ❌" >&2
    echo "" >&2
    echo "   Check its logs with: $COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH logs $3" >&2
    echo "   Stop the data services with: ./setup.sh stop" >&2
}

# Postgres and RabbitMQ only take their credentials from the environment
# when their volume is first created. Before the app starts, check that the
# bundled services accept the credentials the app will connect with, so an
# old volume fails here with a clear message instead of an auth error in
# the API logs. External services (another host in the URL) are skipped.
function checkBundledCredentials() {
    local attempts=${CREDENTIAL_CHECK_ATTEMPTS:-60}
    local interval=${CREDENTIAL_CHECK_INTERVAL:-2}
    local compose="$COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH"
    local services
    services=$($compose config --services 2>/dev/null)
    local user password host port name out i ready

    IFS=$'\x1f' read -r user password host port name <<< "$(splitServiceUrl "$(appServiceUrl DATABASE_URL "$DEFAULT_DATABASE_URL")")"
    if [ "$host" == "pi-dash-db" ] && grep -qx "pi-dash-db" <<< "$services"; then
        port=${port:-5432}
        # 127.0.0.1 only answers once first-boot initialisation is done.
        ready=""
        for ((i = 0; i < attempts; i++)); do
            out=$($compose exec -T pi-dash-db pg_isready -h 127.0.0.1 -p "$port" 2>&1) && { ready=1; break; }
            sleep "$interval"
        done
        if [ -z "$ready" ]; then
            serviceNotReady "PostgreSQL" "no answer on port $port (${out:-no output})" "pi-dash-db"
            return 1
        fi
        # Log in over TCP with the password, as the app does: the image
        # trusts its local socket, which would skip the password.
        ready=""
        for ((i = 0; i < attempts; i++)); do
            out=$(PGPASSWORD="$password" $compose exec -T -e PGPASSWORD pi-dash-db psql -h 127.0.0.1 -p "$port" -U "$user" -d "$name" -tAc "select 1" 2>&1) && { ready=1; break; }
            [[ $out == *"does not exist"* || $out == *"password authentication failed"* ]] && break
            sleep "$interval"
        done
        if [[ $out == *"password authentication failed"* ]]; then
            credentialMismatch "PostgreSQL" "the database volume does not accept the password for role \"$user\"" "DATABASE_URL"
            return 1
        fi
        if [[ $out == *"does not exist"* ]]; then
            credentialMismatch "PostgreSQL" "the database volume has no role \"$user\" with database \"$name\" (${out##*FATAL:  })" "DATABASE_URL"
            return 1
        fi
        if [ -z "$ready" ]; then
            serviceNotReady "PostgreSQL" "${out##*error: }" "pi-dash-db"
            return 1
        fi
    fi

    IFS=$'\x1f' read -r user password host port name <<< "$(splitServiceUrl "$(appServiceUrl AMQP_URL "$DEFAULT_AMQP_URL")")"
    if [ "$host" == "pi-dash-mq" ] && grep -qx "pi-dash-mq" <<< "$services"; then
        # Kombu's defaults for parts the URL leaves out.
        if [ -z "$user" ]; then
            user="guest"
            password=${password:-guest}
        fi
        name=${name:-/}
        ready=""
        for ((i = 0; i < attempts; i++)); do
            $compose exec -T pi-dash-mq rabbitmq-diagnostics -q check_running >/dev/null 2>&1 && { ready=1; break; }
            sleep "$interval"
        done
        if [ -z "$ready" ]; then
            serviceNotReady "RabbitMQ" "the broker is not running" "pi-dash-mq"
            return 1
        fi
        if ! $compose exec -T pi-dash-mq rabbitmqctl -q authenticate_user "$user" "$password" >/dev/null 2>&1; then
            credentialMismatch "RabbitMQ" "the broker volume does not accept user \"$user\"" "AMQP_URL"
            return 1
        fi
        if ! $compose exec -T pi-dash-mq rabbitmqctl -s list_vhosts name 2>/dev/null | grep -qxF -- "$name"; then
            credentialMismatch "RabbitMQ" "the broker volume has no vhost \"$name\"" "AMQP_URL"
            return 1
        fi
        # A valid user on an existing vhost can still lack access to it.
        if ! $compose exec -T pi-dash-mq rabbitmqctl -s list_permissions -p "$name" 2>/dev/null | cut -f1 | grep -qxF -- "$user"; then
            credentialMismatch "RabbitMQ" "user \"$user\" has no permissions on vhost \"$name\"" "AMQP_URL"
            return 1
        fi
    fi
    return 0
}

function startServices() {
    # Data services first: their volumes must accept the app's credentials.
    # Only the bundled ones this compose file still defines.
    local data_services
    data_services=$($COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH config --services 2>/dev/null | grep -xE "pi-dash-db|pi-dash-mq" | tr '\n' ' ')
    if [ -n "$data_services" ]; then
        if ! /bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH up -d --pull if_not_present --quiet-pull $data_services"; then
            echo "" >&2
            echo "   Failed to start the data services ($data_services) ❌" >&2
            exit 1
        fi
        checkBundledCredentials || exit 1
    fi

    /bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH up -d --pull if_not_present --quiet-pull"

    local migrator_container_id=$(docker container ls -aq -f "name=$SERVICE_FOLDER-migrator")
    if [ -n "$migrator_container_id" ]; then
        local idx=0
        while docker inspect --format='{{.State.Status}}' $migrator_container_id | grep -q "running"; do
            local message=">> Waiting for Data Migration to finish"
            local dots=$(printf '%*s' $idx | tr ' ' '.')
            echo -ne "\r$message$dots"
            ((idx++))
            sleep 1
        done
    fi
    printf "\r\033[K"
    echo ""
    echo "   Data Migration completed successfully ✅"

    # if migrator exit status is not 0, show error message and exit
    if [ -n "$migrator_container_id" ]; then
        local migrator_exit_code=$(docker inspect --format='{{.State.ExitCode}}' $migrator_container_id)
        if [ $migrator_exit_code -ne 0 ]; then
            echo "Pi Dash Server failed to start ❌"
            # stopServices
            echo
            echo "Please check the logs for the 'migrator' service and resolve the issue(s)."
            echo "Stop the services by running the command: ./setup.sh stop"
            exit 1
        fi
    fi

    local api_container_id=$(docker container ls -q -f "name=$SERVICE_FOLDER-api")

    # Verify container exists
    if [ -z "$api_container_id" ]; then
        echo "   Error: API container not found. Please check if services are running."
        exit 1
    fi

    local idx2=0
    local api_ready=true        # assume success, flip on timeout
    local max_wait_time=300  # 5 minutes timeout
    local start_time=$(date +%s)

    echo "   Waiting for API Service to be ready..."
    while ! docker exec "$api_container_id" python3 -c "import urllib.request; urllib.request.urlopen('http://localhost:8000/', timeout=3)" > /dev/null 2>&1; do
        local current_time=$(date +%s)
        local elapsed_time=$((current_time - start_time))

        if [ $elapsed_time -gt $max_wait_time ]; then
            echo ""
            echo "   API Service health check timed out after 5 minutes"
            echo "   Checking if API container is still running..."
            if docker ps | grep -q "$SERVICE_FOLDER-api"; then
                echo "   API container is running but did not pass the health-check. Continuing without marking it ready."
                api_ready=false
                break
            else
                echo "   API container is not running. Please check logs."
                exit 1
            fi
        fi

        local message=">> Waiting for API Service to Start (${elapsed_time}s)"
        local dots=$(printf '%*s' $idx2 | tr ' ' '.')
        echo -ne "\r$message$dots"
        ((idx2++))
        sleep 1
    done
    printf "\r\033[K"
    if [ "$api_ready" = true ]; then
        echo "   API Service started successfully ✅"
    else
        echo "   ⚠️  API Service did not respond to health-check – please verify manually."
    fi
    source "${DOCKER_ENV_PATH}"
    echo "   Pi Dash Server started successfully ✅"
    echo ""
    echo "   You can access the application at $WEB_URL"
    echo ""

}
function stopServices() {
    /bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH --env-file=$DOCKER_ENV_PATH down"
}
function restartServices() {
    stopServices
    startServices
}
function upgrade() {
    local latest_release=$(checkLatestRelease)

    echo ""
    echo "Current release: $APP_RELEASE"

    if [ "$latest_release" == "$APP_RELEASE" ]; then
        echo ""
        echo "You are already using the latest release"
        exit 0
    fi

    echo "Latest release: $latest_release"
    echo ""

    # Check for confirmation to upgrade
    echo "Do you want to upgrade to the latest release ($latest_release)?"
    read -p "Continue? [y/N]: " confirm

    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        echo "Exiting..."
        exit 0
    fi

    export APP_RELEASE=$latest_release

    echo "Upgrading Pi Dash to the latest release..."
    echo ""

    echo "***** STOPPING SERVICES ****"
    stopServices

    echo
    echo "***** DOWNLOADING STABLE VERSION ****"
    install

    echo "***** PLEASE VALIDATE AND START SERVICES ****"
}
function viewSpecificLogs(){
    local SERVICE_NAME=$1

    if /bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH ps | grep -q '$SERVICE_NAME'"; then
        echo "Service '$SERVICE_NAME' is running."
    else
        echo "Service '$SERVICE_NAME' is not running."
    fi

    /bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH logs -f $SERVICE_NAME"
}
function viewLogs(){
    
    ARG_SERVICE_NAME=$2

    if [ -z "$ARG_SERVICE_NAME" ];
    then
        echo
        echo "Select a Service you want to view the logs for:"
        echo "   1) Web"
        echo "   2) Space"
        echo "   3) API"
        echo "   4) Worker"
        echo "   5) Beat-Worker"
        echo "   6) Migrator"
        echo "   7) Proxy"
        echo "   8) Redis"
        echo "   9) Postgres"
        echo "   10) Minio"
        echo "   11) RabbitMQ"
        echo "   0) Back to Main Menu"
        echo 
        read -p "Service: " DOCKER_SERVICE_NAME

        until (( DOCKER_SERVICE_NAME >= 0 && DOCKER_SERVICE_NAME <= 11 )); do
            echo "Invalid selection. Please enter a number between 0 and 11."
            read -p "Service: " DOCKER_SERVICE_NAME
        done

        if [ -z "$DOCKER_SERVICE_NAME" ];
        then
            echo "INVALID SERVICE NAME SUPPLIED"
        else
            case $DOCKER_SERVICE_NAME in
                1) viewSpecificLogs "web";;
                2) viewSpecificLogs "space";;
                3) viewSpecificLogs "api";;
                4) viewSpecificLogs "worker";;
                5) viewSpecificLogs "beat-worker";;
                6) viewSpecificLogs "migrator";;
                7) viewSpecificLogs "proxy";;
                8) viewSpecificLogs "pi-dash-redis";;
                9) viewSpecificLogs "pi-dash-db";;
                10) viewSpecificLogs "pi-dash-minio";;
                11) viewSpecificLogs "pi-dash-mq";;
                0) askForAction;;
                *) echo "INVALID SERVICE NAME SUPPLIED";;
            esac
        fi
    elif [ -n "$ARG_SERVICE_NAME" ];
    then
        ARG_SERVICE_NAME=$(echo "$ARG_SERVICE_NAME" | tr '[:upper:]' '[:lower:]')
        case $ARG_SERVICE_NAME in
            web) viewSpecificLogs "web";;
            space) viewSpecificLogs "space";;
            api) viewSpecificLogs "api";;
            worker) viewSpecificLogs "worker";;
            beat-worker) viewSpecificLogs "beat-worker";;
            migrator) viewSpecificLogs "migrator";;
            proxy) viewSpecificLogs "proxy";;
            redis) viewSpecificLogs "pi-dash-redis";;
            postgres) viewSpecificLogs "pi-dash-db";;
            minio) viewSpecificLogs "pi-dash-minio";;
            rabbitmq) viewSpecificLogs "pi-dash-mq";;
            *) echo "INVALID SERVICE NAME SUPPLIED";;
        esac
    else
        echo "INVALID SERVICE NAME SUPPLIED"
    fi
}
function backup_container_dir() {
    local BACKUP_FOLDER=$1
    local CONTAINER_NAME=$2
    local CONTAINER_DATA_DIR=$3
    local SERVICE_FOLDER=$4

    echo "Backing up $CONTAINER_NAME data..."
    local CONTAINER_ID=$(/bin/bash -c "$COMPOSE_CMD -f $DOCKER_FILE_PATH ps -q $CONTAINER_NAME")
    if [ -z "$CONTAINER_ID" ]; then
        echo "Error: $CONTAINER_NAME container not found. Make sure the services are running."
        return 1
    fi

    # Create a temporary directory for the backup
    mkdir -p "$BACKUP_FOLDER/$SERVICE_FOLDER"

    # Copy the data directory from the running container
    echo "Copying $CONTAINER_NAME data directory..."
    docker cp -q "$CONTAINER_ID:$CONTAINER_DATA_DIR/." "$BACKUP_FOLDER/$SERVICE_FOLDER/"
    local cp_status=$?

    if [ $cp_status -ne 0 ]; then
        echo "Error: Failed to copy $SERVICE_FOLDER data"
        rm -rf $BACKUP_FOLDER/$SERVICE_FOLDER
        return 1
    fi

    # Create tar.gz of the data
    cd "$BACKUP_FOLDER"
    tar -czf "${SERVICE_FOLDER}.tar.gz" "$SERVICE_FOLDER/"
    local tar_status=$?
    if [ $tar_status -eq 0 ]; then
        rm -rf "$SERVICE_FOLDER/"
    fi
    cd - > /dev/null

    if [ $tar_status -ne 0 ]; then
        echo "Error: Failed to create tar archive"
        return 1
    fi

    echo "Successfully backed up $SERVICE_FOLDER data"
}

function backupData() {
    local datetime=$(date +"%Y%m%d-%H%M")
    local BACKUP_FOLDER=$PI_DASH_INSTALL_DIR/backup/$datetime
    mkdir -p "$BACKUP_FOLDER"

    # Check if docker-compose.yml exists
    if [ ! -f "$DOCKER_FILE_PATH" ]; then
        echo "Error: docker-compose.yml not found at $DOCKER_FILE_PATH"
        exit 1
    fi

    backup_container_dir "$BACKUP_FOLDER" "pi-dash-db" "/var/lib/postgresql/data" "pgdata" || exit 1
    backup_container_dir "$BACKUP_FOLDER" "pi-dash-minio" "/export" "uploads" || exit 1
    backup_container_dir "$BACKUP_FOLDER" "pi-dash-mq" "/var/lib/rabbitmq" "rabbitmq_data" || exit 1
    backup_container_dir "$BACKUP_FOLDER" "pi-dash-redis" "/data" "redisdata" || exit 1

    echo ""
    echo "Backup completed successfully. Backup files are stored in $BACKUP_FOLDER"
    echo ""
}
function askForAction() {
    local DEFAULT_ACTION=$1

    if [ -z "$DEFAULT_ACTION" ];
    then
        echo
        echo "Select a Action you want to perform:"
        echo "   1) Install"
        echo "   2) Start"
        echo "   3) Stop"
        echo "   4) Restart"
        echo "   5) Upgrade"
        echo "   6) View Logs"
        echo "   7) Backup Data"
        echo "   8) Exit"
        echo 
        read -p "Action [2]: " ACTION
        until [[ -z "$ACTION" || "$ACTION" =~ ^[1-8]$ ]]; do
            echo "$ACTION: invalid selection."
            read -p "Action [2]: " ACTION
        done

        if [ -z "$ACTION" ];
        then
            ACTION=2
        fi
        echo
    fi

    if [ "$ACTION" == "1" ] || [ "$DEFAULT_ACTION" == "install" ];
    then
        install
        # askForAction
    elif [ "$ACTION" == "2" ] || [ "$DEFAULT_ACTION" == "start" ];
    then
        startServices
        # askForAction
    elif [ "$ACTION" == "3" ] || [ "$DEFAULT_ACTION" == "stop" ];
    then
        stopServices
        # askForAction
    elif [ "$ACTION" == "4" ] || [ "$DEFAULT_ACTION" == "restart" ];
    then
        restartServices
        # askForAction
    elif [ "$ACTION" == "5" ]  || [ "$DEFAULT_ACTION" == "upgrade" ];
    then
        upgrade
        # askForAction
    elif [ "$ACTION" == "6" ]  || [ "$DEFAULT_ACTION" == "logs" ];
    then
        viewLogs "$@"
        askForAction
    elif [ "$ACTION" == "7" ]  || [ "$DEFAULT_ACTION" == "backup" ];
    then
        backupData
    elif [ "$ACTION" == "8" ]
    then
        exit 0
    else
        echo "INVALID ACTION SUPPLIED"
    fi
}

# if docker-compose is installed
if command -v docker-compose &> /dev/null
then
    COMPOSE_CMD="docker-compose"
else
    COMPOSE_CMD="docker compose"
fi

if [ "$CPU_ARCH" == "x86_64" ] || [ "$CPU_ARCH" == "amd64" ]; then
    CPU_ARCH="amd64"
elif [ "$CPU_ARCH" == "aarch64" ] || [ "$CPU_ARCH" == "arm64" ]; then
    CPU_ARCH="arm64"
fi

if [ -f "$DOCKER_ENV_PATH" ]; then
    DOCKERHUB_USER=$(getEnvValue "DOCKERHUB_USER" "$DOCKER_ENV_PATH")
    APP_RELEASE=$(getEnvValue "APP_RELEASE" "$DOCKER_ENV_PATH")
    PULL_POLICY=$(getEnvValue "PULL_POLICY" "$DOCKER_ENV_PATH")
    CUSTOM_BUILD=$(getEnvValue "CUSTOM_BUILD" "$DOCKER_ENV_PATH")

    if [ -z "$DOCKERHUB_USER" ]; then
        DOCKERHUB_USER=airepublic
        updateEnvFile "DOCKERHUB_USER" "$DOCKERHUB_USER" "$DOCKER_ENV_PATH"
    fi

    if [ -z "$APP_RELEASE" ]; then
        APP_RELEASE=stable
        updateEnvFile "APP_RELEASE" "$APP_RELEASE" "$DOCKER_ENV_PATH"
    fi

    if [ -z "$PULL_POLICY" ]; then
        PULL_POLICY=if_not_present
        updateEnvFile "PULL_POLICY" "$PULL_POLICY" "$DOCKER_ENV_PATH"
    fi

    if [ -z "$CUSTOM_BUILD" ]; then
        CUSTOM_BUILD=false
        updateEnvFile "CUSTOM_BUILD" "$CUSTOM_BUILD" "$DOCKER_ENV_PATH"
    fi
fi

# Tests source this file for its functions without running the menu.
if [ "${SETUP_SH_SOURCE_ONLY:-}" != "1" ]; then
    print_header
    askForAction "$@"
fi
