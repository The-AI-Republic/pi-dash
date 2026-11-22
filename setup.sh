#!/bin/bash

# Pi Dash Project Setup Script (NEWPIDASH-382: active system only)
# This script prepares the environment by setting up all necessary .env
# files, generating stable secrets, and running the database migrator.
# https://github.com/The-AI-Republic/pi-dash

# Set colors for output messages
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# Print header
echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BOLD}${BLUE}                   Pi Dash - Project Management Tool                    ${NC}"
echo -e "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BOLD}Setting up your environment...${NC}\n"

# Function to copy an env example only when the destination is missing.
# Existing files are never overwritten, so re-running setup keeps secrets.
copy_env_file() {
    local source=$1
    local destination=$2

    if [ -f "$destination" ]; then
        echo -e "${GREEN}✓${NC} Kept existing $destination"
        return 0
    fi

    if [ ! -f "$source" ]; then
        echo -e "${RED}Error: Source file $source does not exist.${NC}"
        return 1
    fi

    cp "$source" "$destination"

    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓${NC} Copied $destination"
    else
        echo -e "${RED}✗${NC} Failed to copy $destination"
        return 1
    fi
}

# Set a KEY in FILE to VALUE when the key is missing or still holds the
# example placeholder. Anything else is kept untouched.
ensure_secret() {
    local file=$1
    local key=$2
    local placeholder=$3
    local value=$4

    if grep -q "^${key}=${placeholder}$" "$file" 2>/dev/null; then
        sed -i "s/^${key}=${placeholder}$/${key}=${value}/" "$file" && return 0
        return 1
    fi
    if grep -q "^${key}=" "$file" 2>/dev/null; then
        return 0
    fi
    echo "${key}=${value}" >> "$file"
}

# Activate pnpm (version set in package.json)
export LC_ALL=C
export LC_CTYPE=C
echo -e "${YELLOW}Setting up environment files...${NC}"

# One env file for the Rust stack and live, plus the infra root file and
# the interim Django migrator file. The old web/space/admin env files are
# gone with the frozen frontends: setup no longer creates them.
success=true

copy_env_file "./.env.example" "./.env" || success=false
copy_env_file "./rust-api/.env.example" "./rust-api/.env" || success=false
copy_env_file "./apps/live/.env.example" "./apps/live/.env" || success=false
# Interim (NEWPIDASH-380): the compose migrator still runs Django
# `manage.py migrate`, so it needs the Django env file. TODO(NEWPIDASH-380):
# drop this once the native `pidash-api migrate up` migrator lands.
copy_env_file "./apps/api/.env.example" "./apps/api/.env" || success=false

# Generate stable secrets (kept across re-runs by ensure_secret).
echo -e "\n${YELLOW}Generating secrets...${NC}"
SECRET_KEY=$(tr -dc 'a-z0-9' < /dev/urandom | head -c50)
LIVE_SERVER_SECRET_KEY=$(tr -dc 'a-z0-9' < /dev/urandom | head -c50)
if [ -z "$SECRET_KEY" ] || [ -z "$LIVE_SERVER_SECRET_KEY" ]; then
    echo -e "${RED}Error: Failed to generate secrets.${NC}"
    echo -e "${RED}Ensure 'tr' and 'head' commands are available on your system.${NC}"
    success=false
fi
if python3 -c "import cryptography.fernet" 2>/dev/null; then
    ASSISTANT_ENCRYPTION_KEY=$(python3 -c "from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())")
else
    # openssl emits standard base64; Fernet needs the urlsafe alphabet.
    ASSISTANT_ENCRYPTION_KEY=$(openssl rand -base64 32 | tr '+/' '-_')
fi
if [ -z "$ASSISTANT_ENCRYPTION_KEY" ]; then
    echo -e "${RED}Error: Failed to generate ASSISTANT_ENCRYPTION_KEY.${NC}"
    success=false
fi

if [ "$success" = true ]; then
    # SECRET_KEY is shared: Rust sessions/keyring and the interim Django
    # migrator must agree (upgraded databases keep Django-encrypted fields).
    ensure_secret "./rust-api/.env" "SECRET_KEY" "change-me-generate-with-setup-sh" "$SECRET_KEY" || success=false
    if [ -f "./apps/api/.env" ]; then
        if grep -q "^SECRET_KEY=" "./apps/api/.env"; then
            echo -e "${GREEN}✓${NC} Kept existing SECRET_KEY in apps/api/.env"
        else
            echo "SECRET_KEY=\"$SECRET_KEY\"" >> ./apps/api/.env \
                && echo -e "${GREEN}✓${NC} Added SECRET_KEY to apps/api/.env" \
                || success=false
        fi
    fi
    ensure_secret "./rust-api/.env" "ASSISTANT_ENCRYPTION_KEY" "change-me-generate-with-setup-sh" "$ASSISTANT_ENCRYPTION_KEY" || success=false
    # LIVE_SERVER_SECRET_KEY is shared by compose live and host `pnpm dev`.
    ensure_secret "./rust-api/.env" "LIVE_SERVER_SECRET_KEY" "change-me-generate-with-setup-sh" "$LIVE_SERVER_SECRET_KEY" || success=false
    if [ -f "./apps/live/.env" ]; then
        ensure_secret "./apps/live/.env" "LIVE_SERVER_SECRET_KEY" "\"secret-key\"" "$LIVE_SERVER_SECRET_KEY" \
            && ensure_secret "./apps/live/.env" "LIVE_SERVER_SECRET_KEY" "secret-key" "$LIVE_SERVER_SECRET_KEY" \
            && echo -e "${GREEN}✓${NC} LIVE_SERVER_SECRET_KEY set in apps/live/.env" \
            || success=false
    fi
fi

# Run the migrator first: if it fails (or has not run), nothing else starts.
# `docker compose run` honors depends_on, so the database comes up too.
if [ "$success" = true ]; then
    echo -e "\n${YELLOW}Running database migrations...${NC}"
    if docker compose run --rm --build migrator; then
        echo -e "${GREEN}✓${NC} Migrations completed"
    else
        echo -e "${RED}✗${NC} Migration failed — fix the error above and re-run ./setup.sh. Nothing else was started."
        success=false
    fi
fi

# Install Node dependencies
corepack enable pnpm || success=false
pnpm install || success=false

# Summary
echo -e "\n${YELLOW}Setup status:${NC}"
if [ "$success" = true ]; then
    echo -e "${GREEN}✓${NC} Environment setup completed successfully!\n"
    echo -e "${BOLD}Next steps:${NC}"
    echo -e "1. Review the .env files (./.env, ./rust-api/.env) if needed"
    echo -e "2. Start the services with: ${BOLD}docker compose up -d${NC}"
    echo -e "3. For local dev: ${BOLD}docker compose -f docker-compose-local.yml up -d${NC} then ${BOLD}pnpm --filter web_new dev${NC} (host, port 3010)"
    echo -e "4. To upgrade later: ${BOLD}docker compose run --rm migrator \&\& docker compose up -d --build${NC} (stops on migration failure)"
    echo -e "\n${GREEN}Happy coding! 🚀${NC}"
else
    echo -e "${RED}✗${NC} Some issues occurred during setup. Please check the errors above.\n"
    echo -e "For help, visit: ${BLUE}https://github.com/The-AI-Republic/pi-dash${NC}"
    exit 1
fi
