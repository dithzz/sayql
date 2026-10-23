//! A synthetic multi-tenant health database for testing agent mode: tenants, users,
//! memberships with roles, invites, an audit log, patients, appointments, observations and
//! an appointment status lookup table. Every row is made up; names, emails (example.com,
//! .org, .net), 555 phone numbers and TEST-NET IP addresses are fictional.
//!
//! What is planted, for questions with a known answer:
//! - One owner per tenant (`memberships.role = 'owner'`). Northfield Family Practice was
//!   created by Grace Holloway but its owner is Priya Natarajan (an `ownership.transfer`
//!   row in the audit log), so "who owns it?" has two plausible readings.
//! - Personal data in users, patients and invites (`planted_pii`), for the masking gate.
//! - Orphans: 3 appointments whose patient doesn't exist (no foreign key on that column),
//!   and 1 invite from a user who doesn't exist.
//! - A drop: in the last week of the data (from Monday 2026-08-24) Pinecrest Physio has no
//!   appointments; every other week it has 8, and each other tenant has 4.
//! - `appointment_statuses`: status 3 is "Cancelled".
//!
//! The data is fixed (no random values, no dates relative to today), so answers are the
//! same on every run and every database. `script` writes it for each dialect.

use super::integration_tests::{env_target, exec, open};
use super::{ConnConfig, DbPool, Kind, connect};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;

/// The database the fixture is loaded into on PostgreSQL, MySQL and SQL Server.
pub const DATABASE: &str = "sayql_tenant";

pub const TENANTS: [&str; 4] = ["Harbor Lane Clinic", "Northfield Family Practice", "Pinecrest Physio", "Riverbend Dental"];

const USERS: [&str; 24] = [
    "Dana Reyes", "Marcus Bell", "Aiko Tanaka", "Samuel Okoro", "Lena Fischer", "Tomás Ruiz", "Grace Holloway",
    "Ravi Menon", "Priya Natarajan", "Owen Gallagher", "Hana Kim", "Felix Brandt", "Noor Haddad", "Ethan Price",
    "Chloe Martin", "Kwame Mensah", "Isabel Costa", "Victor Lindqvist", "Amara Obi", "Jonah Weiss", "Mei Lin",
    "Carlos Duarte", "Sofia Romano", "Ben Carter",
];

const FIRST: [&str; 15] = [
    "Alba", "Bruno", "Cora", "Desmond", "Elif", "Farah", "Gideon", "Hollis", "Ines", "Jasper", "Keiko", "Leandro",
    "Marisol", "Nikhil", "Odette",
];
const LAST: [&str; 12] =
    ["Abernathy", "Bjornsen", "Castellano", "Delacroix", "Esposito", "Fairbanks", "Granger", "Hargreaves", "Iwasaki", "Jablonski", "Kowalczyk", "Larkspur"];
const STREETS: [&str; 6] = ["Harbour Lane", "Mill Road", "Juniper Court", "Old Quarry Way", "Larch Avenue", "Kestrel Close"];
const TOWNS: [&str; 4] = ["Westhaven", "Brookmere", "Ashcombe", "Tidewell"];

/// The first day of the data: Monday 2026-06-01. There are 13 weeks.
fn week_start(w: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 6, 1).unwrap() + Duration::weeks(w)
}
const WEEKS: i64 = 13;

/// The tenant of a user (users 1-6, 7-12, 13-17, 18-24).
fn user_tenant(u: usize) -> usize {
    match u {
        1..=6 => 1,
        7..=12 => 2,
        13..=17 => 3,
        _ => 4,
    }
}

/// The owner of each tenant, by user id.
const OWNERS: [usize; 4] = [1, 9, 13, 18];
/// Who created each tenant.
const CREATORS: [usize; 4] = [1, 7, 13, 18];

fn ascii(s: &str) -> String {
    s.chars().map(|c| if c == 'á' { 'a' } else { c }).collect()
}

fn user_email(u: usize) -> String {
    ascii(&USERS[u - 1].to_lowercase().replace(' ', ".")) + "@example.com"
}

fn user_phone(u: usize) -> Option<String> {
    (!u.is_multiple_of(5)).then(|| format!("+1 (555) 01{u:02}-{:04}", (u * 373) % 10_000))
}

fn patient_name(p: usize) -> (&'static str, &'static str) {
    (FIRST[(p - 1) % 15], LAST[(p * 5) % 12])
}

fn patient_email(p: usize) -> Option<String> {
    let (f, l) = patient_name(p);
    (!p.is_multiple_of(7)).then(|| format!("{}.{}{p}@example.org", f.to_lowercase(), l.to_lowercase()))
}

fn patient_phone(p: usize) -> Option<String> {
    (!p.is_multiple_of(9)).then(|| format!("+1 555-02{p:02}-{:04}", (p * 919) % 10_000))
}

fn patient_dob(p: usize) -> String {
    format!("{}-{:02}-{:02}", 1940 + (p * 7) % 60, p % 12 + 1, (p * 3) % 28 + 1)
}

fn patient_mrn(p: usize) -> String {
    format!("MRN-{}{p:04}", (p - 1) / 15 + 1)
}

fn patient_address(p: usize) -> String {
    format!("{} {}, {}", 3 + (p * 17) % 190, STREETS[p % 6], TOWNS[p % 4])
}

fn patient_notes(p: usize) -> Option<String> {
    match p % 4 {
        0 => Some(format!("Prefers afternoon visits. Daughter's number is +1 555-0199-{p:04}.")),
        1 => patient_email(p).map(|e| format!("Send letters to {e} rather than post.")),
        2 => Some("Allergic to penicillin.".to_string()),
        _ => None,
    }
}

fn invite_email(i: usize) -> String {
    format!("invitee.{i}@example.net")
}

fn user_ip(u: usize) -> String {
    format!("198.51.100.{}", 10 + u)
}

/// Every planted personal value, for checking that masking hides them all. Only values
/// long enough not to turn up by chance inside other text.
pub fn planted_pii() -> Vec<String> {
    let mut out = Vec::new();
    for u in 1..=USERS.len() {
        out.extend([USERS[u - 1].to_string(), user_email(u), user_ip(u)]);
        out.extend(user_phone(u));
    }
    for p in 1..=60 {
        let (f, l) = patient_name(p);
        out.extend([l.to_string(), patient_dob(p), patient_mrn(p), patient_address(p)]);
        out.extend(patient_email(p));
        out.extend(patient_phone(p));
        out.extend(patient_notes(p));
        if f.len() >= 5 {
            out.push(f.to_string());
        }
    }
    out.extend((1..=8).map(invite_email));
    out
}

enum V {
    I(i64),
    S(String),
    Null,
}

fn s(v: impl Into<String>) -> V {
    V::S(v.into())
}

fn opt(v: Option<String>) -> V {
    v.map_or(V::Null, V::S)
}

fn ts(t: NaiveDateTime) -> V {
    V::S(t.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn at(d: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
    d.and_hms_opt(h, m, 0).unwrap()
}

/// Column types: `int`, `text(n)`, `datetime`, `date`, `decimal`; `pk` marks the key.
struct Table {
    name: &'static str,
    columns: &'static [(&'static str, &'static str)],
    /// (column, referenced table)
    fks: &'static [(&'static str, &'static str)],
    rows: Vec<Vec<V>>,
}

fn tables() -> Vec<Table> {
    let statuses = Table {
        name: "appointment_statuses",
        columns: &[("id", "int pk"), ("code", "text(20)"), ("label", "text(40)")],
        fks: &[],
        rows: [(1, "booked", "Booked"), (2, "completed", "Completed"), (3, "cancelled", "Cancelled"), (4, "no_show", "No-show")]
            .into_iter()
            .map(|(i, c, l)| vec![V::I(i), s(c), s(l)])
            .collect(),
    };
    let tenants = Table {
        name: "tenants",
        columns: &[("id", "int pk"), ("name", "text(80)"), ("slug", "text(40)"), ("tier", "text(20)"), ("created_by_user_id", "int"), ("created_at", "datetime")],
        fks: &[],
        rows: (1..=4)
            .map(|t| {
                vec![
                    V::I(t as i64),
                    s(TENANTS[t - 1]),
                    s(TENANTS[t - 1].split(' ').next().unwrap().to_lowercase()),
                    s(["pro", "team", "free", "pro"][t - 1]),
                    V::I(CREATORS[t - 1] as i64),
                    ts(at(NaiveDate::from_ymd_opt(2025, 1 + t as u32 * 2, 10).unwrap(), 9, 30)),
                ]
            })
            .collect(),
    };
    let users = Table {
        name: "users",
        columns: &[("id", "int pk"), ("email", "text(120)"), ("full_name", "text(80)"), ("phone", "text(30)"), ("created_at", "datetime"), ("last_login_at", "datetime")],
        fks: &[],
        rows: (1..=USERS.len())
            .map(|u| {
                vec![
                    V::I(u as i64),
                    s(user_email(u)),
                    s(USERS[u - 1]),
                    opt(user_phone(u)),
                    ts(at(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + Duration::days(u as i64 * 9), 8, 0)),
                    ts(at(week_start(WEEKS - 1) + Duration::days((u % 6) as i64), 7 + (u % 10) as u32, 15)),
                ]
            })
            .collect(),
    };
    let mut membership_rows = Vec::new();
    for u in 1..=USERS.len() {
        let t = user_tenant(u);
        let role = if OWNERS[t - 1] == u {
            "owner"
        } else if CREATORS[t - 1] == u || u % 4 == 0 {
            "admin"
        } else if u % 5 == 0 {
            "viewer"
        } else {
            "member"
        };
        membership_rows.push((t, u, role));
    }
    // Dana Reyes also helps out at Pinecrest Physio.
    membership_rows.push((3, 1, "admin"));
    let memberships = Table {
        name: "memberships",
        columns: &[("id", "int pk"), ("tenant_id", "int"), ("user_id", "int"), ("role", "text(20)"), ("created_at", "datetime")],
        fks: &[("tenant_id", "tenants"), ("user_id", "users")],
        rows: membership_rows
            .iter()
            .enumerate()
            .map(|(i, (t, u, role))| {
                let day = NaiveDate::from_ymd_opt(2025, 2, 1).unwrap() + Duration::days(i as i64 * 5);
                vec![V::I(i as i64 + 1), V::I(*t as i64), V::I(*u as i64), s(*role), ts(at(day, 10, 0))]
            })
            .collect(),
    };
    let invites = Table {
        name: "invites",
        columns: &[
            ("id", "int pk"),
            ("tenant_id", "int"),
            ("email", "text(120)"),
            ("role", "text(20)"),
            ("invited_by_user_id", "int"),
            ("status", "text(20)"),
            ("created_at", "datetime"),
        ],
        fks: &[("tenant_id", "tenants")],
        rows: (1..=8)
            .map(|i| {
                let t = (i - 1) % 4 + 1;
                // Invite 8 came from a user who no longer exists.
                let by = if i == 8 { 99 } else { OWNERS[t - 1] };
                let status = ["pending", "accepted", "expired"][i % 3];
                vec![
                    V::I(i as i64),
                    V::I(t as i64),
                    s(invite_email(i)),
                    s(if i % 2 == 0 { "member" } else { "viewer" }),
                    V::I(by as i64),
                    s(status),
                    ts(at(week_start(i as i64), 11, 0)),
                ]
            })
            .collect(),
    };
    let patients = Table {
        name: "patients",
        columns: &[
            ("id", "int pk"),
            ("tenant_id", "int"),
            ("mrn", "text(20)"),
            ("first_name", "text(40)"),
            ("last_name", "text(40)"),
            ("date_of_birth", "date"),
            ("phone", "text(30)"),
            ("email", "text(120)"),
            ("address", "text(120)"),
            ("notes", "text(300)"),
            ("created_at", "datetime"),
        ],
        fks: &[("tenant_id", "tenants")],
        rows: (1..=60)
            .map(|p| {
                let (f, l) = patient_name(p);
                vec![
                    V::I(p as i64),
                    V::I(((p - 1) / 15 + 1) as i64),
                    s(patient_mrn(p)),
                    s(f),
                    s(l),
                    s(patient_dob(p)),
                    opt(patient_phone(p)),
                    opt(patient_email(p)),
                    s(patient_address(p)),
                    opt(patient_notes(p)),
                    ts(at(NaiveDate::from_ymd_opt(2025, 3, 1).unwrap() + Duration::days(p as i64 * 4), 9, 0)),
                ]
            })
            .collect(),
    };
    let mut appointment_rows = Vec::new();
    let mut n = 0;
    for w in 0..WEEKS {
        for t in 1..=4usize {
            let per_week = match (t, w) {
                (3, w) if w == WEEKS - 1 => 0,
                (3, _) => 8,
                _ => 4,
            };
            let clinicians: Vec<usize> = (1..=USERS.len()).filter(|&u| user_tenant(u) == t && u % 5 != 0).collect();
            for k in 0..per_week {
                n += 1;
                let day = week_start(w) + Duration::days(((k * 2 + t) % 6) as i64);
                let when = at(day, 8 + ((k * 3 + t) % 9) as u32, if k % 2 == 0 { 0 } else { 30 });
                let patient = (t - 1) * 15 + 1 + (k * 7 + w as usize * 3) % 15;
                let status = match n % 10 {
                    0 => 3,
                    1 => 4,
                    _ if w == WEEKS - 1 && k % 2 == 0 => 1,
                    _ => 2,
                };
                appointment_rows.push(vec![
                    V::I(n as i64),
                    V::I(t as i64),
                    V::I(patient as i64),
                    V::I(clinicians[k % clinicians.len()] as i64),
                    V::I(status),
                    ts(when),
                    ts(when - Duration::days((k % 5 + 1) as i64)),
                ]);
            }
        }
    }
    // Three appointments for patients who don't exist.
    for missing in [997, 998, 999] {
        n += 1;
        let when = at(week_start(1) + Duration::days(missing % 3), 14, 0);
        appointment_rows.push(vec![V::I(n as i64), V::I(4), V::I(missing), V::I(19), V::I(2), ts(when), ts(when - Duration::days(2))]);
    }
    let appointments = Table {
        name: "appointments",
        columns: &[
            ("id", "int pk"),
            ("tenant_id", "int"),
            ("patient_id", "int"),
            ("clinician_user_id", "int"),
            ("status_id", "int"),
            ("scheduled_at", "datetime"),
            ("created_at", "datetime"),
        ],
        // No key on patient_id: that is how the orphans got in.
        fks: &[("tenant_id", "tenants"), ("status_id", "appointment_statuses")],
        rows: appointment_rows,
    };
    let observations = Table {
        name: "observations",
        columns: &[("id", "int pk"), ("patient_id", "int"), ("kind", "text(30)"), ("reading", "decimal"), ("recorded_at", "datetime")],
        fks: &[("patient_id", "patients")],
        rows: (1..=60)
            .flat_map(|p| {
                let day = week_start((p % WEEKS as usize) as i64) + Duration::days((p % 5) as i64);
                [
                    vec![V::I(p as i64 * 2 - 1), V::I(p as i64), s("heart_rate"), s(format!("{}.0", 58 + (p * 7) % 40)), ts(at(day, 9, 10))],
                    vec![V::I(p as i64 * 2), V::I(p as i64), s("systolic_bp"), s(format!("{}.5", 104 + (p * 11) % 50)), ts(at(day, 9, 12))],
                ]
            })
            .collect(),
    };
    let mut audit_rows = Vec::new();
    let mut add = |tenant: usize, user: usize, action: &str, entity: &str, id: i64, when: NaiveDateTime, detail: Option<String>| {
        let n = audit_rows.len() as i64 + 1;
        audit_rows.push(vec![V::I(n), V::I(tenant as i64), V::I(user as i64), s(action), s(entity), V::I(id), opt(detail), s(user_ip(user)), ts(when)]);
    };
    add(2, 7, "ownership.transfer", "tenant", 2, at(NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(), 10, 15), Some("from user 7 to user 9".into()));
    for w in 0..WEEKS {
        for u in 1..=USERS.len() {
            if (u + w as usize).is_multiple_of(3) {
                add(user_tenant(u), u, "login", "user", u as i64, at(week_start(w) + Duration::days((u % 5) as i64), 8, (u * 2) as u32), None);
            }
        }
        let p = (w as usize * 4) % 60 + 1;
        let t = (p - 1) / 15 + 1;
        add(t, OWNERS[t - 1], "patient.update", "patient", p as i64, at(week_start(w) + Duration::days(2), 15, 5), Some("changed phone".into()));
    }
    for i in 1..=7 {
        let t = (i - 1) % 4 + 1;
        add(t, OWNERS[t - 1], "member.invite", "invite", i as i64, at(week_start(i as i64), 11, 0), None);
    }
    let audit_log = Table {
        name: "audit_log",
        columns: &[
            ("id", "int pk"),
            ("tenant_id", "int"),
            ("user_id", "int"),
            ("action", "text(40)"),
            ("entity", "text(40)"),
            ("entity_id", "int"),
            ("detail", "text(200)"),
            ("ip_address", "text(45)"),
            ("created_at", "datetime"),
        ],
        fks: &[("tenant_id", "tenants")],
        rows: audit_rows,
    };
    vec![statuses, tenants, users, memberships, invites, patients, appointments, observations, audit_log]
}

fn column_type(kind: Kind, t: &str) -> String {
    let (base, pk) = t.split_once(' ').map_or((t, false), |(b, rest)| (b, rest == "pk"));
    let ty = match (base, kind) {
        ("int", Kind::Sqlite) => "INTEGER".to_string(),
        ("int", _) => "INT".to_string(),
        (b, Kind::Sqlite) if b.starts_with("text") => "TEXT".to_string(),
        (b, Kind::Mssql) if b.starts_with("text") => format!("NVARCHAR{}", &b[4..]),
        (b, _) if b.starts_with("text") => format!("VARCHAR{}", &b[4..]),
        ("datetime", Kind::Sqlite) | ("date", Kind::Sqlite) => "TEXT".to_string(),
        ("datetime", Kind::Postgres) => "TIMESTAMP".to_string(),
        ("datetime", Kind::Mysql) => "DATETIME".to_string(),
        ("datetime", Kind::Mssql) => "DATETIME2(0)".to_string(),
        ("date", _) => "DATE".to_string(),
        ("decimal", Kind::Sqlite) => "NUMERIC".to_string(),
        ("decimal", _) => "DECIMAL(6,1)".to_string(),
        (other, _) => panic!("unknown column type {other}"),
    };
    if pk { format!("{ty} PRIMARY KEY") } else { ty }
}

fn literal(kind: Kind, v: &V) -> String {
    match v {
        V::I(i) => i.to_string(),
        V::Null => "NULL".into(),
        V::S(s) if kind == Kind::Mssql => format!("N'{}'", s.replace('\'', "''")),
        V::S(s) => format!("'{}'", s.replace('\'', "''")),
    }
}

/// The statements that create and fill the fixture in `kind`'s dialect.
pub fn statements(kind: Kind) -> Vec<String> {
    let mut out = Vec::new();
    for t in tables() {
        let mut cols: Vec<String> = t.columns.iter().map(|(c, ty)| format!("{c} {}", column_type(kind, ty))).collect();
        cols.extend(t.fks.iter().map(|(c, r)| format!("FOREIGN KEY ({c}) REFERENCES {r}(id)")));
        out.push(format!("CREATE TABLE {} (\n  {}\n)", t.name, cols.join(",\n  ")));
        let names: Vec<&str> = t.columns.iter().map(|(c, _)| *c).collect();
        // SQL Server takes at most 1,000 rows per VALUES list.
        for chunk in t.rows.chunks(200) {
            let rows: Vec<String> =
                chunk.iter().map(|r| format!("({})", r.iter().map(|v| literal(kind, v)).collect::<Vec<_>>().join(", "))).collect();
            out.push(format!("INSERT INTO {} ({}) VALUES\n{}", t.name, names.join(", "), rows.join(",\n")));
        }
    }
    out
}

/// The fixture as a SQLite file at `path`, opened as SayQL opens a connection.
pub async fn sqlite(path: &Path) -> DbPool {
    let o = SqliteConnectOptions::new().filename(path).create_if_missing(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(o).await.unwrap();
    for sql in statements(Kind::Sqlite) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&pool).await.unwrap();
    }
    pool.close().await;
    let cfg = ConnConfig {
        id: "tenant".into(),
        name: "Tenant fixture".into(),
        kind: Kind::Sqlite,
        host: String::new(),
        port: None,
        database: String::new(),
        user: String::new(),
        file: path.to_string_lossy().into_owned(),
        ssl: false,
        trust_cert: false,
    };
    connect(&cfg, None).await.unwrap()
}

/// The fixture in a `sayql_tenant` database on each server whose SAYQL_TEST_*_URL variable
/// is set (see integration_tests.rs). Loaded fresh once per test run, for every test that asks.
pub async fn servers() -> Vec<DbPool> {
    static LOADED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    LOADED.get_or_init(load_servers).await;
    let mut out = Vec::new();
    for (var, kind) in [("SAYQL_TEST_PG_URL", Kind::Postgres), ("SAYQL_TEST_MYSQL_URL", Kind::Mysql), ("SAYQL_TEST_MSSQL_URL", Kind::Mssql)] {
        if let Some(t) = env_target(var) {
            out.push(open(kind, &t, DATABASE).await);
        }
    }
    out
}

/// Drops and recreates `sayql_tenant` on each server that is set, and fills it.
async fn load_servers() {
    if let Some(t) = env_target("SAYQL_TEST_PG_URL") {
        let base = if t.database.is_empty() { "postgres".to_string() } else { t.database.clone() };
        let admin = open(Kind::Postgres, &t, &base).await;
        exec(&admin, &format!("DROP DATABASE IF EXISTS {DATABASE} WITH (FORCE)")).await;
        exec(&admin, &format!("CREATE DATABASE {DATABASE}")).await;
        fill(&open(Kind::Postgres, &t, DATABASE).await).await;
    }
    if let Some(t) = env_target("SAYQL_TEST_MYSQL_URL") {
        let admin = open(Kind::Mysql, &t, &t.database).await;
        exec(&admin, &format!("DROP DATABASE IF EXISTS {DATABASE}; CREATE DATABASE {DATABASE} CHARACTER SET utf8mb4")).await;
        fill(&open(Kind::Mysql, &t, DATABASE).await).await;
    }
    if let Some(t) = env_target("SAYQL_TEST_MSSQL_URL") {
        let admin = open(Kind::Mssql, &t, &t.database).await;
        exec(
            &admin,
            &format!(
                "IF DB_ID('{DATABASE}') IS NOT NULL BEGIN ALTER DATABASE {DATABASE} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE {DATABASE}; END"
            ),
        )
        .await;
        exec(&admin, &format!("CREATE DATABASE {DATABASE}")).await;
        fill(&open(Kind::Mssql, &t, DATABASE).await).await;
    }
}

async fn fill(pool: &DbPool) {
    for sql in statements(pool.kind()) {
        exec(pool, &sql).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn scalar(pool: &DbPool, sql: &str) -> serde_json::Value {
        super::super::run_query(pool, sql, true).await.unwrap_or_else(|e| panic!("{sql}: {e}")).rows.remove(0).remove(0)
    }

    /// The plants are where the module docs say, on SQLite and on each server that is set.
    #[tokio::test]
    async fn fixture_loads_with_its_plants() {
        let dir = std::env::temp_dir().join(format!("sayql-tenant-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut pools = vec![sqlite(&dir.join("tenant.sqlite")).await];
        pools.extend(servers().await);
        for pool in &pools {
            let k = pool.kind();
            let owner = "SELECT u.full_name FROM memberships m JOIN users u ON u.id = m.user_id JOIN tenants t ON t.id = m.tenant_id \
                         WHERE t.name = 'Northfield Family Practice' AND m.role = 'owner'";
            assert_eq!(scalar(pool, owner).await, json!("Priya Natarajan"), "{k:?}");
            let owners = "SELECT COUNT(*) FROM memberships WHERE role = 'owner'";
            assert_eq!(scalar(pool, owners).await, json!(4), "{k:?}");
            let orphans = "SELECT COUNT(*) FROM appointments a LEFT JOIN patients p ON p.id = a.patient_id WHERE p.id IS NULL";
            assert_eq!(scalar(pool, orphans).await, json!(3), "{k:?}");
            let last_week = "SELECT COUNT(*) FROM appointments WHERE tenant_id = 3 AND scheduled_at >= '2026-08-24'";
            assert_eq!(scalar(pool, last_week).await, json!(0), "{k:?}");
            assert_eq!(scalar(pool, "SELECT label FROM appointment_statuses WHERE id = 3").await, json!("Cancelled"), "{k:?}");
            assert_eq!(scalar(pool, "SELECT full_name FROM users WHERE id = 6").await, json!("Tomás Ruiz"), "{k:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
