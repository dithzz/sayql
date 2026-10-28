#!/usr/bin/env bash
# A throwaway SayQL team server for the explainer's Teams scenes, on 127.0.0.1:8092, with a small
# SQLite shop to query. Its data lives in scripts/video/out/team-server and is reset each start.
#
#   SAYQL_SERVER_BIN=/path/to/sayql-server scripts/video/team-server.sh     # keep running
#
# It prints a one-time admin page link. Pass it to one capture, with the database:
#
#   SAYQL_ADMIN_LINK='http://127.0.0.1:8092/admin#code=…' \
#   SAYQL_TEAM_DB="$PWD/scripts/video/out/team-server/shop.db" node scripts/video/capture.mjs dark
#
# The capture adds the connection, a person and an agent itself, so restart the server (a new
# link, empty data) before the next capture.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)/out/team-server"
rm -rf "$here"
mkdir -p "$here/data"
python3 - "$here/shop.db" <<'PY'
import random, sqlite3, sys
random.seed(7)
db = sqlite3.connect(sys.argv[1])
db.executescript("""
CREATE TABLE customers (id INTEGER PRIMARY KEY, tenant_id INTEGER, name TEXT, email TEXT, country TEXT, plan TEXT, signed_up_at TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, tenant_id INTEGER, customer_id INTEGER, status TEXT, total REAL, ordered_at TEXT);
CREATE TABLE tickets (id INTEGER PRIMARY KEY, tenant_id INTEGER, customer_id INTEGER, subject TEXT, status TEXT, opened_at TEXT);
CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER, card_last4 TEXT, amount REAL);
""")
first = ["Ava", "Liam", "Noah", "Emma", "Olivia", "Arjun", "Priya", "Mateo", "Kenji", "Lucas", "Sofia", "Mia"]
tenants = [42, 42, 7, 13]
day = lambda m: f"2026-{m:02d}-{random.randint(1, 28):02d}"
for i in range(1, 301):
    n = random.choice(first)
    db.execute("INSERT INTO customers VALUES (?,?,?,?,?,?,?)", (i, random.choice(tenants), f"{n} {random.choice('ABCDEFGHJK')}.", f"{n.lower()}{i}@example.com",
               random.choice(["Canada", "India", "Germany", "Japan", "Brazil"]), random.choice(["free", "pro", "team"]), day(random.randint(1, 10))))
for i in range(1, 2001):
    db.execute("INSERT INTO orders VALUES (?,?,?,?,?,?)", (i, random.choice(tenants), random.randint(1, 300), random.choice(["paid", "paid", "shipped", "refunded"]),
               round(random.uniform(9, 400), 2), day(random.randint(1, 10))))
    db.execute("INSERT INTO payments VALUES (?,?,?,?)", (i, i, f"{random.randint(0, 9999):04d}", 0))
for i in range(1, 401):
    db.execute("INSERT INTO tickets VALUES (?,?,?,?,?,?)", (i, random.choice(tenants), random.randint(1, 300), random.choice(["Refund", "Login", "Billing", "Shipping"]),
               random.choice(["open", "closed"]), day(10)))
db.commit()
PY
export SAYQL_DATA_DIR="$here/data"
export SAYQL_BIND=127.0.0.1:8092
export SAYQL_PUBLIC_URL=http://127.0.0.1:8092
exec "${SAYQL_SERVER_BIN:-sayql-server}" serve
