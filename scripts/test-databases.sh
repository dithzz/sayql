#!/usr/bin/env bash
# Runs the database tests against every server version in docs/DATABASE_COMPATIBILITY.md, each in a
# throwaway Docker container, then removes the containers. Needs Docker; SQL Server and MySQL 5.7 run
# as amd64 images (emulated on Apple silicon, so slower).
#
#   scripts/test-databases.sh              # the whole matrix
#   scripts/test-databases.sh pg17 mysql84 # just these (names below)
#
# Containers are named sayql-dbtest-* and use ports 57xxx; nothing else is touched.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"

ALL=(pg13 pg15 pg16 pg17 mysql57 mysql80 mysql84 mariadb106 mariadb1011 mariadb114 mssql2019 mssql2022)
TARGETS=("${@:-${ALL[@]}}")
PW="Sq$(openssl rand -hex 8)!aA1"

start() { # name image port env... -- extra args
  local name=$1 image=$2 port=$3 inner=$4
  shift 4
  docker rm -f "sayql-dbtest-$name" >/dev/null 2>&1 || true
  docker run -d --name "sayql-dbtest-$name" -p "$port:$inner" "$@" "$image" >/dev/null
}

wait_for() { # name health-command...
  local name=$1
  shift
  for _ in $(seq 1 90); do
    if docker exec "sayql-dbtest-$name" sh -c "$*" >/dev/null 2>&1; then return 0; fi
    sleep 2
  done
  echo "$name did not become ready" >&2
  return 1
}

run() { # name kind url
  local name=$1 var=$2 url=$3
  echo "== $name"
  if env SAYQL_TEST_PG_URL= SAYQL_TEST_MYSQL_URL= SAYQL_TEST_MSSQL_URL= "$var=$url" \
    cargo test -q --lib -- integration_tests:: mcp::tests::tools_work_against_real_databases; then
    PASSED+=("$name")
  else
    FAILED+=("$name")
  fi
  docker rm -f "sayql-dbtest-$name" >/dev/null
}

PASSED=()
FAILED=()
cargo test -q --lib --no-run
for t in "${TARGETS[@]}"; do
  case $t in
    pg1[0-9])
      start "$t" "postgres:${t#pg}" 57000 5432 -e POSTGRES_PASSWORD="$PW" -e POSTGRES_DB=sayql
      wait_for "$t" pg_isready -U postgres -h 127.0.0.1
      run "$t" SAYQL_TEST_PG_URL "postgres://postgres:$PW@127.0.0.1:57000/sayql" ;;
    mysql57)
      start "$t" mysql:5.7 57001 3306 --platform linux/amd64 -e MYSQL_ROOT_PASSWORD="$PW"
      wait_for "$t" "mysqladmin ping -h 127.0.0.1 -uroot -p\"\$MYSQL_ROOT_PASSWORD\""
      run "$t" SAYQL_TEST_MYSQL_URL "mysql://root:$PW@127.0.0.1:57001" ;;
    mysql8*)
      v=${t#mysql}
      start "$t" "mysql:${v:0:1}.${v:1}" 57001 3306 -e MYSQL_ROOT_PASSWORD="$PW"
      wait_for "$t" "mysqladmin ping -h 127.0.0.1 -uroot -p\"\$MYSQL_ROOT_PASSWORD\""
      run "$t" SAYQL_TEST_MYSQL_URL "mysql://root:$PW@127.0.0.1:57001" ;;
    mariadb*)
      v=${t#mariadb}
      start "$t" "mariadb:${v:0:2}.${v:2}" 57002 3306 -e MARIADB_ROOT_PASSWORD="$PW"
      wait_for "$t" healthcheck.sh --connect --innodb_initialized
      run "$t" SAYQL_TEST_MYSQL_URL "mysql://root:$PW@127.0.0.1:57002" ;;
    mssql*)
      start "$t" "mcr.microsoft.com/mssql/server:${t#mssql}-latest" 57003 1433 --platform linux/amd64 \
        -e ACCEPT_EULA=Y -e MSSQL_SA_PASSWORD="$PW"
      wait_for "$t" "/opt/mssql-tools18/bin/sqlcmd -C -S localhost -U sa -P \"\$MSSQL_SA_PASSWORD\" -Q 'SELECT 1' -b"
      run "$t" SAYQL_TEST_MSSQL_URL "mssql://sa:$PW@127.0.0.1:57003/master" ;;
    *) echo "unknown target: $t (one of: ${ALL[*]})" >&2; exit 2 ;;
  esac
done

echo
echo "passed: ${PASSED[*]:-none}"
echo "failed: ${FAILED[*]:-none}"
[ ${#FAILED[@]} -eq 0 ]
