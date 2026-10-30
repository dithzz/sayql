//! The code an audit reads, per connection: local folders and git repositories (any host).
//! Only where the code is and how its last fetch went is kept here; a repository's token, if
//! it needs one, is in the keychain ([`crate::store::secrets::repo_token`]) and its copy is in
//! `repos/<source id>` next to this file (see [`crate::repos`]).
//!
//! Replaces `audit-folders.json` (one folder per connection), whose folders become local
//! sources the first time this file is read.

use super::Store;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const FILE: &str = "code-sources.json";
/// The one-folder-per-connection file this replaces.
const OLD_FILE: &str = "audit-folders.json";
/// Where git sources are copied, next to the settings files.
const REPOS_DIR: &str = "repos";

/// Where a source's code is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SourceKind {
    /// A folder on this computer, read where it is.
    Local { path: String },
    /// A git repository, copied by SayQL with the person's own git setup.
    Git {
        url: String,
        /// Empty: the repository's default branch.
        #[serde(default)]
        branch: String,
        /// Only this folder of the repository is read.
        #[serde(default)]
        subdir: Option<String>,
        /// A read-only token is saved in the keychain for it.
        #[serde(default)]
        has_token: bool,
        /// The user name sent with the token, when its host isn't one SayQL knows.
        #[serde(default)]
        token_user: Option<String>,
    },
}

/// One code source of a connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeSource {
    /// `src_` and 12 hex digits: also the name of its copy's folder.
    pub id: String,
    /// A short name the audit cites files by (`backend:src/db.ts`), unique per connection.
    pub label: String,
    pub kind: SourceKind,
    /// When it was last fetched (git) or found (a folder), in ms since the Unix epoch.
    #[serde(default)]
    pub last_fetch: Option<u64>,
    /// The commit read last time, when the source is a git repository.
    #[serde(default)]
    pub last_commit: Option<String>,
    /// Why the last fetch or check failed; `None` when it worked.
    #[serde(default)]
    pub error: Option<String>,
}

impl CodeSource {
    /// A new source with a fresh id; `label` is made unique among `others`.
    pub fn new(kind: SourceKind, label: &str, others: &[CodeSource]) -> Self {
        let id = format!("src_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        CodeSource {
            id,
            label: unique_label(label, others),
            kind,
            last_fetch: None,
            last_commit: None,
            error: None,
        }
    }

    pub fn is_git(&self) -> bool {
        matches!(self.kind, SourceKind::Git { .. })
    }
}

/// A label from a folder path or repository URL: its last part, without `.git`.
pub fn default_label(path_or_url: &str) -> String {
    let last = path_or_url
        .trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or("");
    let last = last.strip_suffix(".git").unwrap_or(last);
    let clean: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let clean = clean.trim_matches(['-', '.']).to_string();
    if clean.is_empty() {
        "code".into()
    } else {
        clean
    }
}

/// `label` cleaned up, with `-2`, `-3`… added when another source has it.
fn unique_label(label: &str, others: &[CodeSource]) -> String {
    let base = default_label(label);
    let taken = |l: &str| others.iter().any(|o| o.label.eq_ignore_ascii_case(l));
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|l| !taken(l))
        .expect("some number is free")
}

/// Ids are made here, but the file can be edited: only these name a folder.
fn valid_id(id: &str) -> bool {
    id.len() > 4
        && id.len() <= 40
        && id.starts_with("src_")
        && id[4..].chars().all(|c| c.is_ascii_alphanumeric())
}

impl Store {
    fn all_sources(&self) -> AppResult<BTreeMap<String, Vec<CodeSource>>> {
        if !self.dir.join(FILE).exists() {
            return self.migrate_audit_folders();
        }
        self.read(FILE)
    }

    /// The first read: each connection's audit folder becomes its first source. The old file
    /// is left in place for an older SayQL.
    fn migrate_audit_folders(&self) -> AppResult<BTreeMap<String, Vec<CodeSource>>> {
        let old: BTreeMap<String, String> = self.read(OLD_FILE)?;
        let all: BTreeMap<String, Vec<CodeSource>> = old
            .into_iter()
            .filter(|(_, path)| !path.trim().is_empty())
            .map(|(conn, path)| {
                let source = CodeSource::new(
                    SourceKind::Local { path: path.clone() },
                    &default_label(&path),
                    &[],
                );
                (conn, vec![source])
            })
            .collect();
        self.write(FILE, &all)?;
        Ok(all)
    }

    /// This connection's code sources, in the order added (the first is where the audit runs).
    pub fn code_sources(&self, connection_id: &str) -> AppResult<Vec<CodeSource>> {
        Ok(self
            .all_sources()?
            .remove(connection_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| valid_id(&s.id))
            .collect())
    }

    pub fn code_source(&self, connection_id: &str, source_id: &str) -> AppResult<CodeSource> {
        self.code_sources(connection_id)?
            .into_iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| AppError::User("That code source was removed.".into()))
    }

    /// Adds a source for this connection; the same folder or repository twice is refused.
    pub fn add_code_source(
        &self,
        connection_id: &str,
        kind: SourceKind,
        label: &str,
    ) -> AppResult<CodeSource> {
        let mut all = self.all_sources()?;
        let list = all.entry(connection_id.to_string()).or_default();
        let same = |a: &SourceKind, b: &SourceKind| match (a, b) {
            (SourceKind::Local { path: x }, SourceKind::Local { path: y }) => x == y,
            (
                SourceKind::Git {
                    url: u1,
                    branch: b1,
                    subdir: s1,
                    ..
                },
                SourceKind::Git {
                    url: u2,
                    branch: b2,
                    subdir: s2,
                    ..
                },
            ) => u1 == u2 && b1 == b2 && s1 == s2,
            _ => false,
        };
        if list.iter().any(|s| same(&s.kind, &kind)) {
            return Err("This connection already has that code source.".into());
        }
        let source = CodeSource::new(kind, label, list);
        list.push(source.clone());
        self.write(FILE, &all)?;
        Ok(source)
    }

    /// Changes one source in place (its status after a fetch); a removed one is left alone.
    pub fn update_code_source(
        &self,
        connection_id: &str,
        source_id: &str,
        change: impl FnOnce(&mut CodeSource),
    ) -> AppResult<Option<CodeSource>> {
        let mut all = self.all_sources()?;
        let Some(source) = all
            .get_mut(connection_id)
            .and_then(|l| l.iter_mut().find(|s| s.id == source_id))
        else {
            return Ok(None);
        };
        change(source);
        let out = source.clone();
        self.write(FILE, &all)?;
        Ok(Some(out))
    }

    /// Removes a source with its token and its copy.
    pub fn remove_code_source(&self, connection_id: &str, source_id: &str) -> AppResult<()> {
        let mut all = self.all_sources()?;
        let Some(list) = all.get_mut(connection_id) else {
            return Ok(());
        };
        let Some(i) = list.iter().position(|s| s.id == source_id) else {
            return Ok(());
        };
        let gone = list.remove(i);
        if list.is_empty() {
            all.remove(connection_id);
        }
        self.write(FILE, &all)?;
        self.drop_source_files(&gone)
    }

    /// Where a git source's copy is kept: `repos/<source id>`.
    pub fn repo_dir(&self, source_id: &str) -> AppResult<PathBuf> {
        if !valid_id(source_id) {
            return Err("That code source isn't valid.".into());
        }
        Ok(self.dir.join(REPOS_DIR).join(source_id))
    }

    /// A removed source's token and copy. The keychain is only touched when a token was saved.
    fn drop_source_files(&self, source: &CodeSource) -> AppResult<()> {
        if let SourceKind::Git { has_token, .. } = source.kind {
            if has_token {
                super::secrets::delete_repo_token(&source.id)?;
            }
            if let Ok(dir) = self.repo_dir(&source.id)
                && dir.exists()
            {
                std::fs::remove_dir_all(dir)?;
            }
        }
        Ok(())
    }

    /// Forgets a deleted connection's sources, their tokens and copies.
    pub(super) fn forget_code_sources(&self, connection_id: &str) -> AppResult<()> {
        let mut all = self.all_sources()?;
        let Some(gone) = all.remove(connection_id) else {
            return Ok(());
        };
        self.write(FILE, &all)?;
        gone.iter()
            .map(|s| self.drop_source_files(s))
            .fold(Ok(()), AppResult::and)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tag: &str) -> (PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("sayql-{tag}-{}", uuid::Uuid::new_v4()));
        (dir.clone(), Store::new(dir))
    }

    fn git(url: &str) -> SourceKind {
        SourceKind::Git {
            url: url.into(),
            branch: "main".into(),
            subdir: None,
            has_token: false,
            token_user: None,
        }
    }

    #[test]
    fn audit_folders_become_sources_once() {
        let (dir, s) = store("sources-migrate");
        std::fs::write(
            dir.join(OLD_FILE),
            r#"{"c1":"/Users/me/code/shop","c2":"/Users/me/code/crm/"}"#,
        )
        .unwrap();
        let c1 = s.code_sources("c1").unwrap();
        assert_eq!(c1.len(), 1);
        assert_eq!(
            c1[0].kind,
            SourceKind::Local {
                path: "/Users/me/code/shop".into()
            }
        );
        assert_eq!(c1[0].label, "shop");
        assert_eq!(s.code_sources("c2").unwrap()[0].label, "crm");
        // Once: a later change to the old file isn't read again, and removing isn't undone.
        s.remove_code_source("c1", &c1[0].id).unwrap();
        std::fs::write(dir.join(OLD_FILE), r#"{"c3":"/x"}"#).unwrap();
        assert!(s.code_sources("c1").unwrap().is_empty());
        assert!(s.code_sources("c3").unwrap().is_empty());
        assert_eq!(
            Store::new(dir.clone()).code_sources("c2").unwrap()[0].id,
            s.code_sources("c2").unwrap()[0].id
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sources_per_connection_with_unique_labels() {
        let (dir, s) = store("sources");
        let a = s
            .add_code_source("c1", git("https://github.com/acme/api.git"), "api")
            .unwrap();
        let b = s
            .add_code_source("c1", git("git@gitlab.com:acme/api.git"), "api")
            .unwrap();
        assert_eq!((a.label.as_str(), b.label.as_str()), ("api", "api-2"));
        assert!(valid_id(&a.id) && a.id != b.id);
        assert!(
            s.add_code_source("c1", git("https://github.com/acme/api.git"), "x")
                .is_err(),
            "the same repository twice"
        );
        s.add_code_source(
            "c2",
            SourceKind::Local {
                path: "/code".into(),
            },
            "code",
        )
        .unwrap();
        assert_eq!(s.code_sources("c1").unwrap().len(), 2);

        let updated = s
            .update_code_source("c1", &a.id, |x| {
                x.last_commit = Some("abc123".into());
                x.error = None;
            })
            .unwrap()
            .unwrap();
        assert_eq!(updated.last_commit.as_deref(), Some("abc123"));
        assert_eq!(
            s.update_code_source("c1", "src_gone", |_| {}).unwrap(),
            None
        );

        // A source's copy goes with it, and so do a deleted connection's.
        let copy = s.repo_dir(&a.id).unwrap();
        std::fs::create_dir_all(copy.join("src")).unwrap();
        s.remove_code_source("c1", &a.id).unwrap();
        assert!(!copy.exists());
        let copy_b = s.repo_dir(&b.id).unwrap();
        std::fs::create_dir_all(&copy_b).unwrap();
        s.forget_agent("c1").unwrap();
        assert!(s.code_sources("c1").unwrap().is_empty());
        assert!(!copy_b.exists());
        assert_eq!(s.code_sources("c2").unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_removed_source_takes_its_token_with_it() {
        let (dir, s) = store("sources-token");
        let mut kind = git("https://github.com/acme/private.git");
        if let SourceKind::Git { has_token, .. } = &mut kind {
            *has_token = true;
        }
        let src = s.add_code_source("c1", kind, "private").unwrap();
        super::super::secrets::set_repo_token(&src.id, "ghp_test").unwrap();
        assert!(super::super::secrets::repo_token(&src.id).is_some());
        s.forget_agent("c1").unwrap();
        assert_eq!(super::super::secrets::repo_token(&src.id), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn labels_and_ids() {
        assert_eq!(
            default_label("https://github.com/acme/billing-api.git"),
            "billing-api"
        );
        assert_eq!(default_label("git@github.com:acme/shop.git"), "shop");
        assert_eq!(default_label("/Users/me/My Code/"), "My-Code");
        assert_eq!(default_label(r"C:\code\crm"), "crm");
        assert_eq!(default_label("///"), "code");
        for bad in ["src_", "src_../x", "../src_a", "src_a/b", "x_abc"] {
            assert!(!valid_id(bad), "{bad}");
        }
        let (dir, s) = store("sources-ids");
        assert!(s.repo_dir("src_../../etc").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
