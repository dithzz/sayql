#!/bin/sh
# Install or update the SayQL for teams server with Docker.
#
#   curl -fsSL https://sayql.dev/install | sh
#
# It asks for a domain. With one, Caddy gets an HTTPS certificate for it automatically (the
# domain must point at this machine, with ports 80 and 443 open). Without one, the server runs on
# http://localhost:8080 so you can try it on this machine first. Run it again any time to update.
# Settings, without the questions: SAYQL_DOMAIN, SAYQL_DIR (default ~/sayql), SAYQL_ADMIN_EMAIL.
set -eu

VERSION=0.2.4
IMAGE="ghcr.io/azenvoc/sayql-server:$VERSION"
CADDY_IMAGE="caddy:2.10"
DIR="${SAYQL_DIR:-$HOME/sayql}"

say() { printf '%s\n' "$*"; }
fail() { printf '\nerror: %s\n' "$*" >&2; exit 1; }
ask() {
	# Read from the terminal even when this script arrives through a pipe.
	if (: </dev/tty) 2>/dev/null; then
		printf '%s' "$1" >/dev/tty 2>/dev/null || true
		read -r REPLY </dev/tty || REPLY=""
	else
		REPLY=""
	fi
}

command -v docker >/dev/null 2>&1 ||
	fail "Docker isn't installed. Install it (https://docs.docker.com/get-docker/) and run this again."
docker compose version >/dev/null 2>&1 ||
	fail "Docker Compose v2 is missing. Update Docker (https://docs.docker.com/compose/install/) and run this again."
docker info >/dev/null 2>&1 ||
	fail "Can't reach Docker. Start it, or run this as a user that can use Docker (or with sudo)."

mkdir -p "$DIR"
cd "$DIR"

# Keep the answers from last time.
if [ -f .env ]; then
	# shellcheck disable=SC1091
	. ./.env
fi
DOMAIN="${SAYQL_DOMAIN:-}"
ADMIN_EMAIL="${SAYQL_ADMIN_EMAIL:-}"
if [ ! -f .env ]; then
	say ""
	say "SayQL for teams: server setup in $DIR"
	say ""
	if [ -z "$DOMAIN" ]; then
		say "Your team reaches the server at an HTTPS address, e.g. sayql.yourcompany.com."
		say "Point that name at this machine and open ports 80 and 443; the certificate is automatic."
		ask "Domain (press Enter to try it on this machine first): "
		DOMAIN=$(printf '%s' "$REPLY" | tr -d '[:space:]' | sed -e 's#^https\{0,1\}://##' -e 's#/.*$##')
	fi
	if [ -z "$ADMIN_EMAIL" ]; then
		ask "Your email, for the admin account (optional): "
		ADMIN_EMAIL=$(printf '%s' "$REPLY" | tr -d '[:space:]')
	fi
	umask 077
	{
		say "# SayQL server settings. Edit, then run: docker compose up -d"
		say "SAYQL_DOMAIN=$DOMAIN"
		say "SAYQL_ADMIN_EMAIL=$ADMIN_EMAIL"
		say "# Paste a licence key on the admin page, or set it here."
		say "SAYQL_LICENSE="
	} >.env
	umask 022
fi

if [ -n "$DOMAIN" ]; then
	URL="https://$DOMAIN"
	cat >compose.yml <<EOF
# Written by https://sayql.dev/install. Settings live in .env next to this file.
services:
  sayql:
    image: $IMAGE
    restart: unless-stopped
    # A database on this machine is at host.docker.internal.
    extra_hosts:
      - "host.docker.internal:host-gateway"
    environment:
      SAYQL_PUBLIC_URL: https://\${SAYQL_DOMAIN}
      SAYQL_ADMIN_EMAIL: \${SAYQL_ADMIN_EMAIL:-}
      SAYQL_LICENSE: \${SAYQL_LICENSE:-}
    volumes:
      - sayql-data:/data
  # HTTPS with an automatic certificate for SAYQL_DOMAIN.
  caddy:
    image: $CADDY_IMAGE
    restart: unless-stopped
    command: caddy reverse-proxy --from \${SAYQL_DOMAIN} --to sayql:8080
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - caddy-data:/data
volumes:
  sayql-data:
  caddy-data:
EOF
else
	URL="http://localhost:8080"
	cat >compose.yml <<EOF
# Written by https://sayql.dev/install. Settings live in .env next to this file.
# This runs on this machine only. For your team, run the installer again with a domain:
#   rm .env && curl -fsSL https://sayql.dev/install | sh
services:
  sayql:
    image: $IMAGE
    restart: unless-stopped
    # A database on this machine is at host.docker.internal.
    extra_hosts:
      - "host.docker.internal:host-gateway"
    ports:
      - "127.0.0.1:8080:8080"
    environment:
      SAYQL_PUBLIC_URL: http://localhost:8080
      SAYQL_ADMIN_EMAIL: \${SAYQL_ADMIN_EMAIL:-}
      SAYQL_LICENSE: \${SAYQL_LICENSE:-}
    volumes:
      - sayql-data:/data
volumes:
  sayql-data:
EOF
fi

say ""
say "Starting SayQL server ${VERSION}..."
docker compose pull --quiet --ignore-pull-failures
# Recreated so the logs below are from this start only.
docker compose up -d --remove-orphans --force-recreate

# The server prints a one-time admin page link until the first admin signs in.
LINK=""
i=0
while [ $i -lt 30 ]; do
	LINK=$(docker compose logs sayql 2>/dev/null | grep -o 'http[^ ]*/admin#code=[A-Za-z0-9-]*' | tail -n 1 || true)
	[ -n "$LINK" ] && break
	if docker compose logs sayql 2>/dev/null | grep -q "listening"; then
		break
	fi
	i=$((i + 1))
	sleep 1
done

say ""
if [ -n "$LINK" ]; then
	say "Done. Open this link to set up SayQL (it works once):"
	say ""
	say "    $LINK"
	say ""
	say "On the admin page: add a database, then invite your team. Each person gets a link that"
	say "opens the SayQL app (https://sayql.dev) and signs them in."
	[ -n "$DOMAIN" ] && say "The HTTPS certificate can take a minute on the first start."
else
	say "SayQL is running at $URL/admin"
	say "Need a new admin sign-in link? Run:"
	say "    cd $DIR && docker compose exec sayql sayql-server admin invite --email you@yourcompany.com --role admin"
fi
say ""
say "Your settings and data: $DIR (back up the sayql-data volume). Update: run this again."
