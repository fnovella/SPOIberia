use crate::auth::AuthManager;
use crate::models::{ReversePayload, SpoSite, TransactionOpRecord, TransactionRecord};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open_default() -> Result<Self> {
        let path = Self::default_db_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn default_db_path() -> Result<PathBuf> {
        if let Ok(custom) = std::env::var("SPOIBERIA_DB_PATH") {
            return Ok(PathBuf::from(custom));
        }
        let data_dir = AuthManager::data_dir()?;
        Ok(data_dir.join("spoiberia.db"))
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS transactions (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at      TEXT NOT NULL,
                actor_upn       TEXT NOT NULL,
                site_url        TEXT NOT NULL,
                site_title      TEXT NOT NULL,
                source          TEXT NOT NULL,
                note            TEXT,
                status          TEXT NOT NULL,
                reverted_at     TEXT,
                summary         TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS transaction_operations (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                transaction_id  INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
                seq_order       INTEGER NOT NULL,
                op_type         TEXT NOT NULL,
                target_label    TEXT NOT NULL,
                forward_payload TEXT NOT NULL,
                reverse_payload TEXT NOT NULL,
                status          TEXT NOT NULL,
                error_message   TEXT
            );

            CREATE TABLE IF NOT EXISTS cached_sites (
                url             TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                description     TEXT,
                role_json       TEXT NOT NULL,
                is_site_admin   INTEGER NOT NULL,
                updated_at      TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    pub fn create_transaction(
        &self,
        actor_upn: &str,
        site_url: &str,
        site_title: &str,
        source: &str,
        note: Option<&str>,
        summary: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transactions (created_at, actor_upn, site_url, site_title, source, note, status, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'IN_PROGRESS', ?7)",
            params![now, actor_upn, site_url, site_title, source, note, summary],
        )?;
        Ok(conn.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_operation(
        &self,
        transaction_id: i64,
        seq_order: i32,
        op_type: &str,
        target_label: &str,
        forward_payload: &str,
        reverse_payload: &ReversePayload,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<i64> {
        let rev_json = serde_json::to_string(reverse_payload)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transaction_operations
             (transaction_id, seq_order, op_type, target_label, forward_payload, reverse_payload, status, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                transaction_id,
                seq_order,
                op_type,
                target_label,
                forward_payload,
                rev_json,
                status,
                error_message
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_transaction_status(&self, transaction_id: i64, status: &str) -> Result<()> {
        let reverted_at = if status == "REVERTED" {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        let conn = self.conn.lock().unwrap();
        if let Some(rev_time) = reverted_at {
            conn.execute(
                "UPDATE transactions SET status = ?1, reverted_at = ?2 WHERE id = ?3",
                params![status, rev_time, transaction_id],
            )?;
        } else {
            conn.execute(
                "UPDATE transactions SET status = ?1 WHERE id = ?2",
                params![status, transaction_id],
            )?;
        }
        Ok(())
    }

    pub fn update_operation_status(
        &self,
        op_id: i64,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE transaction_operations SET status = ?1, error_message = ?2 WHERE id = ?3",
            params![status, error_message, op_id],
        )?;
        Ok(())
    }

    pub fn list_transactions(
        &self,
        site_url_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TransactionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();

        if let Some(site_url) = site_url_filter {
            let mut stmt = conn.prepare(
                "SELECT id, created_at, actor_upn, site_url, site_title, source, note, status, reverted_at, summary
                 FROM transactions
                 WHERE site_url = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![site_url, limit as i64], Self::map_tx_row)?;
            for r in rows {
                out.push(r?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, created_at, actor_upn, site_url, site_title, source, note, status, reverted_at, summary
                 FROM transactions
                 ORDER BY id DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], Self::map_tx_row)?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    pub fn get_transaction(&self, transaction_id: i64) -> Result<Option<TransactionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, actor_upn, site_url, site_title, source, note, status, reverted_at, summary
             FROM transactions
             WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![transaction_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::map_tx_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Returns operations for a transaction ordered by `seq_order ASC`.
    pub fn get_transaction_operations(
        &self,
        transaction_id: i64,
    ) -> Result<Vec<TransactionOpRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, transaction_id, seq_order, op_type, target_label, forward_payload, reverse_payload, status, error_message
             FROM transaction_operations
             WHERE transaction_id = ?1
             ORDER BY seq_order ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![transaction_id], |row| {
            Ok(TransactionOpRecord {
                id: row.get(0)?,
                transaction_id: row.get(1)?,
                seq_order: row.get(2)?,
                op_type: row.get(3)?,
                target_label: row.get(4)?,
                forward_payload: row.get(5)?,
                reverse_payload: row.get(6)?,
                status: row.get(7)?,
                error_message: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_cached_sites(&self, sites: &[SpoSite]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        if sites.len() > 1 {
            tx.execute("DELETE FROM cached_sites", [])?;
        }
        for s in sites {
            let role_json = serde_json::to_string(&s.role)?;
            tx.execute(
                "INSERT INTO cached_sites (url, title, description, role_json, is_site_admin, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(url) DO UPDATE SET
                    title = excluded.title,
                    description = excluded.description,
                    role_json = excluded.role_json,
                    is_site_admin = excluded.is_site_admin,
                    updated_at = excluded.updated_at",
                params![
                    s.url,
                    s.title,
                    s.description,
                    role_json,
                    if s.is_site_admin { 1 } else { 0 },
                    now
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_cached_sites(&self) -> Result<Vec<SpoSite>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT url, title, description, role_json, is_site_admin
             FROM cached_sites
             ORDER BY title COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let url: String = row.get(0)?;
            let title: String = row.get(1)?;
            let description: Option<String> = row.get(2)?;
            let role_json: String = row.get(3)?;
            let is_admin_int: i32 = row.get(4)?;
            let role = serde_json::from_str(&role_json).unwrap_or(crate::models::SiteRole::Owner);
            Ok(SpoSite {
                title,
                url,
                description,
                role,
                is_site_admin: is_admin_int != 0,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn map_tx_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionRecord> {
        Ok(TransactionRecord {
            id: row.get(0)?,
            created_at: row.get(1)?,
            actor_upn: row.get(2)?,
            site_url: row.get(3)?,
            site_title: row.get(4)?,
            source: row.get(5)?,
            note: row.get(6)?,
            status: row.get(7)?,
            reverted_at: row.get(8)?,
            summary: row.get(9)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_transaction_lifecycle() {
        let db = Database::open_in_memory().expect("in-memory sqlite");
        let tx_id = db
            .create_transaction(
                "admin@contoso.com",
                "https://contoso.sharepoint.com/sites/Obra1",
                "Obra 1",
                "CLI",
                Some("Alta inicial"),
                "+1 user in Owners",
            )
            .unwrap();

        let rev = ReversePayload::RemoveAddedUser {
            site_url: "https://contoso.sharepoint.com/sites/Obra1".to_string(),
            group_id: 5,
            user_id: Some(14),
            login_name: "i:0#.f|membership|user1@contoso.com".to_string(),
        };

        let op_id = db
            .record_operation(
                tx_id,
                1,
                "ADD_USER_TO_GROUP",
                "Añadir user1 a Owners",
                r#"{"user":"user1@contoso.com"}"#,
                &rev,
                "APPLIED",
                None,
            )
            .unwrap();
        db.update_transaction_status(tx_id, "APPLIED").unwrap();

        let txs = db.list_transactions(None, 10).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].status, "APPLIED");

        let ops = db.get_transaction_operations(tx_id).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].id, op_id);
        assert_eq!(ops[0].status, "APPLIED");

        db.update_operation_status(op_id, "REVERTED", None).unwrap();
        db.update_transaction_status(tx_id, "REVERTED").unwrap();

        let updated = db.get_transaction(tx_id).unwrap().unwrap();
        assert_eq!(updated.status, "REVERTED");
        assert!(updated.reverted_at.is_some());
    }

    #[test]
    fn test_sqlite_offline_sync_reverse_payload_roundtrip() {
        use crate::models::ViewFormatterBackup;

        let db = Database::open_in_memory().expect("in-memory sqlite");
        let site = "https://contoso.sharepoint.com/sites/Obra1";
        let lib_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

        let tx_id = db
            .create_transaction(
                "admin@contoso.com",
                site,
                "Obra 1",
                "CLI",
                Some("Disable offline sync"),
                "2 cambio(s) sync offline",
            )
            .unwrap();

        let rev_site = ReversePayload::RestoreSiteOfflineSync {
            site_url: site.to_string(),
            previous_exclude_from_offline_client: false,
        };
        let rev_lib = ReversePayload::RestoreLibraryOfflineSync {
            site_url: site.to_string(),
            library_id: lib_id.to_string(),
            previous_exclude_from_offline_client: true,
            restore_exclude_flag: true,
            previous_view_formatters: vec![ViewFormatterBackup {
                view_id: "11111111-1111-1111-1111-111111111111".to_string(),
                view_title: "Todos los documentos".to_string(),
                previous_custom_formatter: r#"{"additionalRowClass":"custom"}"#.to_string(),
            }],
        };

        db.record_operation(
            tx_id,
            1,
            "SET_SITE_OFFLINE_SYNC",
            "[-Sync] Desactivar sincronización offline en el sitio 'Obra 1'",
            "{}",
            &rev_site,
            "APPLIED",
            None,
        )
        .unwrap();
        db.record_operation(
            tx_id,
            2,
            "SET_LIBRARY_OFFLINE_SYNC",
            "[+Sync] Activar sincronización offline en biblioteca 'Documentos'",
            "{}",
            &rev_lib,
            "APPLIED",
            None,
        )
        .unwrap();
        db.update_transaction_status(tx_id, "APPLIED").unwrap();

        let ops = db.get_transaction_operations(tx_id).unwrap();
        assert_eq!(ops.len(), 2);

        let parsed_site: ReversePayload = serde_json::from_str(&ops[0].reverse_payload).unwrap();
        match parsed_site {
            ReversePayload::RestoreSiteOfflineSync {
                site_url,
                previous_exclude_from_offline_client,
            } => {
                assert_eq!(site_url, site);
                assert!(!previous_exclude_from_offline_client);
            }
            other => panic!("unexpected reverse payload: {:?}", other),
        }

        let parsed_lib: ReversePayload = serde_json::from_str(&ops[1].reverse_payload).unwrap();
        match parsed_lib {
            ReversePayload::RestoreLibraryOfflineSync {
                site_url,
                library_id,
                previous_exclude_from_offline_client,
                restore_exclude_flag,
                previous_view_formatters,
            } => {
                assert_eq!(site_url, site);
                assert_eq!(library_id, lib_id);
                assert!(previous_exclude_from_offline_client);
                assert!(restore_exclude_flag);
                assert_eq!(previous_view_formatters.len(), 1);
                assert_eq!(
                    previous_view_formatters[0].view_title,
                    "Todos los documentos"
                );
                assert_eq!(
                    previous_view_formatters[0].previous_custom_formatter,
                    r#"{"additionalRowClass":"custom"}"#
                );
            }
            other => panic!("unexpected reverse payload: {:?}", other),
        }

        // Verify legacy v0.8.0 RestoreLibraryOfflineSync JSON still deserializes cleanly with defaults
        let legacy_json = format!(
            r#"{{"reverse_type":"RestoreLibraryOfflineSync","site_url":"{}","library_id":"{}","previous_exclude_from_offline_client":false}}"#,
            site, lib_id
        );
        let legacy_parsed: ReversePayload = serde_json::from_str(&legacy_json).unwrap();
        match legacy_parsed {
            ReversePayload::RestoreLibraryOfflineSync {
                restore_exclude_flag,
                previous_view_formatters,
                previous_exclude_from_offline_client,
                ..
            } => {
                assert!(!previous_exclude_from_offline_client);
                assert!(restore_exclude_flag);
                assert!(previous_view_formatters.is_empty());
            }
            other => panic!("unexpected legacy payload: {:?}", other),
        }

        // Verify RestoreLibraryOneDriveShortcuts roundtrip
        let rev_od = ReversePayload::RestoreLibraryOneDriveShortcuts {
            site_url: site.to_string(),
            library_id: lib_id.to_string(),
            previous_view_formatters: vec![ViewFormatterBackup {
                view_id: "22222222-2222-2222-2222-222222222222".to_string(),
                view_title: "All Documents".to_string(),
                previous_custom_formatter: String::new(),
            }],
        };
        db.record_operation(
            tx_id,
            3,
            "SET_LIBRARY_ONEDRIVE_SHORTCUTS",
            "[-AtajoOD] Ocultar acceso directo a OneDrive en biblioteca 'Documentos'",
            "{}",
            &rev_od,
            "APPLIED",
            None,
        )
        .unwrap();
        let ops_after = db.get_transaction_operations(tx_id).unwrap();
        assert_eq!(ops_after.len(), 3);
        let parsed_od: ReversePayload = serde_json::from_str(&ops_after[2].reverse_payload).unwrap();
        match parsed_od {
            ReversePayload::RestoreLibraryOneDriveShortcuts {
                site_url,
                library_id,
                previous_view_formatters,
            } => {
                assert_eq!(site_url, site);
                assert_eq!(library_id, lib_id);
                assert_eq!(previous_view_formatters.len(), 1);
                assert_eq!(previous_view_formatters[0].view_title, "All Documents");
                assert_eq!(previous_view_formatters[0].previous_custom_formatter, "");
            }
            other => panic!("unexpected OneDrive reverse payload: {:?}", other),
        }
    }

    #[test]
    fn test_sqlite_folder_inheritance_reverse_payload_roundtrip() {
        use crate::models::FolderPermissionEntry;

        let db = Database::open_in_memory().unwrap();
        let site = "https://contoso.sharepoint.com/sites/Obra1";
        let tx_id = db
            .create_transaction("admin@contoso.com", site, "Obra1", "TUI", None, "Folder perms")
            .unwrap();

        let rev_restore_role = ReversePayload::RestoreFolderRoleAssignment {
            site_url: site.to_string(),
            folder_server_relative_url: "/sites/Obra1/Shared Documents/01_Dir/Sub".to_string(),
            principal_id: 5,
            role_def_id: 1073741830,
        };
        db.record_operation(
            tx_id,
            1,
            "REMOVE_FOLDER_ROLE",
            "[-Rol] Revocar 'Edit' de 'Members' en 'Documents/01_Dir/Sub'",
            "{}",
            &rev_restore_role,
            "APPLIED",
            None,
        )
        .unwrap();

        let rev_restore_unique = ReversePayload::RestoreFolderUniqueRoles {
            site_url: site.to_string(),
            folder_server_relative_url: "/sites/Obra1/Shared Documents/01_Dir/Sub".to_string(),
            had_unique_roles_before: true,
            previous_assignments: vec![FolderPermissionEntry {
                principal_id: 7,
                principal_title: "Equipo_Sub".to_string(),
                principal_login_name: "Equipo_Sub".to_string(),
                principal_type: "SharePointGroup".to_string(),
                roles: vec!["Read".to_string()],
            }],
        };
        db.record_operation(
            tx_id,
            2,
            "RESET_INHERITANCE",
            "[Herencia] Restaurar herencia del padre en 'Documents/01_Dir/Sub'",
            "{}",
            &rev_restore_unique,
            "APPLIED",
            None,
        )
        .unwrap();

        let ops = db.get_transaction_operations(tx_id).unwrap();
        assert_eq!(ops.len(), 2);

        let p1: ReversePayload = serde_json::from_str(&ops[0].reverse_payload).unwrap();
        assert!(matches!(
            p1,
            ReversePayload::RestoreFolderRoleAssignment {
                principal_id: 5,
                role_def_id: 1073741830,
                ..
            }
        ));

        let p2: ReversePayload = serde_json::from_str(&ops[1].reverse_payload).unwrap();
        match p2 {
            ReversePayload::RestoreFolderUniqueRoles {
                had_unique_roles_before,
                previous_assignments,
                ..
            } => {
                assert!(had_unique_roles_before);
                assert_eq!(previous_assignments.len(), 1);
                assert_eq!(previous_assignments[0].principal_title, "Equipo_Sub");
                assert_eq!(previous_assignments[0].roles, vec!["Read".to_string()]);
            }
            other => panic!("unexpected payload: {:?}", other),
        }
    }
}
