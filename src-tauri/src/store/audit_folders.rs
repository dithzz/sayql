//! The project folder an audit last ran in, per connection, so "Audit with…" opens on it
//! again. Only the path is kept; nothing is read from the folder here.

use super::Store;
use crate::error::AppResult;
use std::collections::BTreeMap;

const FILE: &str = "audit-folders.json";

impl Store {
    /// The folder last used to audit this connection, if it's still a folder.
    pub fn audit_folder(&self, connection_id: &str) -> AppResult<Option<String>> {
        let all: BTreeMap<String, String> = self.read(FILE)?;
        Ok(all
            .get(connection_id)
            .filter(|p| std::path::Path::new(p).is_dir())
            .cloned())
    }

    /// Remembers `folder` for this connection.
    pub fn set_audit_folder(&self, connection_id: &str, folder: &str) -> AppResult<()> {
        let mut all: BTreeMap<String, String> = self.read(FILE)?;
        if all.get(connection_id).map(String::as_str) == Some(folder) {
            return Ok(());
        }
        all.insert(connection_id.to_string(), folder.to_string());
        self.write(FILE, &all)
    }

    /// Forgets a deleted connection's folder.
    pub(super) fn forget_audit_folder(&self, connection_id: &str) -> AppResult<()> {
        let mut all: BTreeMap<String, String> = self.read(FILE)?;
        if all.remove(connection_id).is_some() {
            self.write(FILE, &all)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_a_folder_per_connection() {
        let dir = std::env::temp_dir().join(format!("sayql-folders-{}", uuid::Uuid::new_v4()));
        let s = Store::new(dir.clone());
        assert_eq!(s.audit_folder("c1").unwrap(), None);
        let project = dir.join("shop");
        std::fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy().into_owned();
        s.set_audit_folder("c1", &project).unwrap();
        s.set_audit_folder("c2", "/no/such/folder/anywhere")
            .unwrap();
        assert_eq!(s.audit_folder("c1").unwrap(), Some(project));
        assert_eq!(s.audit_folder("c2").unwrap(), None, "a folder that's gone");
        s.forget_agent("c1").unwrap();
        assert_eq!(s.audit_folder("c1").unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
