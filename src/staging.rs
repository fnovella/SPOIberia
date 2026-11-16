use crate::api::spo::SpoClient;
use crate::db::Database;
use crate::models::{ReversePayload, StagingOp, TransactionOpRecord, TransactionRecord};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// Queue of pending changes in memory before the user clicks "Aplicar" (Apply) or executes via CLI.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StagingQueue {
    pub ops: Vec<StagingOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub status: String,
    pub dry_run: bool,
    pub transaction_id: Option<i64>,
    pub site_url: String,
    pub summary: String,
    pub operations: Vec<ExecutedOpSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedOpSummary {
    pub op_id: Option<i64>,
    pub seq_order: i32,
    pub op_type: String,
    pub target: String,
    pub status: String,
    pub error: Option<String>,
}

impl StagingQueue {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn clear(&mut self) {
        self.ops.clear();
    }

    pub fn remove_at(&mut self, index: usize) -> Option<StagingOp> {
        if index < self.ops.len() {
            Some(self.ops.remove(index))
        } else {
            None
        }
    }

    /// Push a new operation onto the staging queue, automatically neutralizing opposite operations
    /// (e.g. adding and then removing the same user in the same group before pressing Apply)
    /// and avoiding exact duplicates.
    pub fn push(&mut self, op: StagingOp) {
        match &op {
            StagingOp::AddUserToGroup {
                site_url,
                group_id,
                user_identifier,
                ..
            } => {
                // If there's a pending Remove for the same user & group, cancel it out
                if let Some(pos) = self.ops.iter().position(|existing| {
                    matches!(
                        existing,
                        StagingOp::RemoveUserFromGroup {
                            site_url: s,
                            group_id: g,
                            user_identifier: u,
                            ..
                        } if s.eq_ignore_ascii_case(site_url)
                            && *g == *group_id
                            && u.eq_ignore_ascii_case(user_identifier)
                    )
                }) {
                    self.ops.remove(pos);
                    return;
                }
            }
            StagingOp::RemoveUserFromGroup {
                site_url,
                group_id,
                user_identifier,
                ..
            } => {
                // If there's a pending Add for the same user & group, cancel it out
                if let Some(pos) = self.ops.iter().position(|existing| {
                    matches!(
                        existing,
                        StagingOp::AddUserToGroup {
                            site_url: s,
                            group_id: g,
                            user_identifier: u,
                            ..
                        } if s.eq_ignore_ascii_case(site_url)
                            && *g == *group_id
                            && u.eq_ignore_ascii_case(user_identifier)
                    )
                }) {
                    self.ops.remove(pos);
                    return;
                }
            }
            _ => {}
        }

        if !self.ops.contains(&op) {
            self.ops.push(op);
        }
    }

    /// Check if a user is currently staged to be added (`Some(true)`) or removed (`Some(false)`) in a given group.
    pub fn user_group_staging_state(
        &self,
        site_url: &str,
        group_id: i64,
        user_identifier: &str,
    ) -> Option<bool> {
        for op in &self.ops {
            match op {
                StagingOp::AddUserToGroup {
                    site_url: s,
                    group_id: g,
                    user_identifier: u,
                    ..
                } if s.eq_ignore_ascii_case(site_url)
                    && *g == group_id
                    && u.eq_ignore_ascii_case(user_identifier) =>
                {
                    return Some(true);
                }
                StagingOp::RemoveUserFromGroup {
                    site_url: s,
                    group_id: g,
                    user_identifier: u,
                    ..
                } if s.eq_ignore_ascii_case(site_url)
                    && *g == group_id
                    && u.eq_ignore_ascii_case(user_identifier) =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        None
    }

    /// Return pending additions for a specific group so the UI can render them with `[+]` in green.
    pub fn pending_additions_for_group(
        &self,
        site_url: &str,
        group_id: i64,
    ) -> Vec<(String, String)> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                StagingOp::AddUserToGroup {
                    site_url: s,
                    group_id: g,
                    user_identifier,
                    user_display_name,
                    ..
                } if s.eq_ignore_ascii_case(site_url) && *g == group_id => {
                    Some((user_display_name.clone(), user_identifier.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Count pending operations for a specific group.
    pub fn count_for_group(&self, site_url: &str, group_id: i64) -> usize {
        self.ops
            .iter()
            .filter(|op| match op {
                StagingOp::AddUserToGroup {
                    site_url: s,
                    group_id: g,
                    ..
                }
                | StagingOp::RemoveUserFromGroup {
                    site_url: s,
                    group_id: g,
                    ..
                } => s.eq_ignore_ascii_case(site_url) && *g == group_id,
                _ => false,
            })
            .count()
    }

    pub fn build_summary(&self) -> String {
        if self.ops.is_empty() {
            return "Sin operaciones pendientes".to_string();
        }
        let mut adds = 0usize;
        let mut rems = 0usize;
        let mut groups = 0usize;
        let mut folders = 0usize;
        let mut perms = 0usize;
        let mut copies = 0usize;

        for op in &self.ops {
            match op {
                StagingOp::AddUserToGroup { .. } => adds += 1,
                StagingOp::RemoveUserFromGroup { .. } => rems += 1,
                StagingOp::CreateGroup { .. } => groups += 1,
                StagingOp::CreateLibrary { .. } | StagingOp::CreateFolder { .. } => folders += 1,
                StagingOp::BreakFolderInheritance { .. }
                | StagingOp::AssignFolderRole { .. }
                | StagingOp::AssignSiteRole { .. } => perms += 1,
                StagingOp::ServerSideCopy { .. } => copies += 1,
            }
        }

        let mut parts = Vec::new();
        if adds > 0 {
            parts.push(format!("+{} alta(s)", adds));
        }
        if rems > 0 {
            parts.push(format!("-{} baja(s)", rems));
        }
        if groups > 0 {
            parts.push(format!("+{} grupo(s)", groups));
        }
        if folders > 0 {
            parts.push(format!("+{} carpeta(s)/bib", folders));
        }
        if perms > 0 {
            parts.push(format!("{} permiso(s)", perms));
        }
        if copies > 0 {
            parts.push(format!("{} copia(s) server-side", copies));
        }
        parts.join(", ")
    }
}

pub struct TransactionExecutor {
    spo: SpoClient,
    db: Database,
}

impl TransactionExecutor {
    pub fn new(spo: SpoClient, db: Database) -> Self {
        Self { spo, db }
    }

    /// Execute (or dry-run) all operations in a `StagingQueue`, persisting the transaction and reverse payloads in SQLite.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_queue(
        &self,
        queue: &StagingQueue,
        actor_upn: &str,
        site_url: &str,
        site_title: &str,
        source: &str,
        note: Option<&str>,
        dry_run: bool,
    ) -> Result<ExecutionReport> {
        let summary = queue.build_summary();
        if dry_run {
            let operations = queue
                .ops
                .iter()
                .enumerate()
                .map(|(i, op)| ExecutedOpSummary {
                    op_id: None,
                    seq_order: (i + 1) as i32,
                    op_type: op.op_type_code().to_string(),
                    target: op.label(),
                    status: "DRY_RUN".to_string(),
                    error: None,
                })
                .collect();
            return Ok(ExecutionReport {
                status: "DRY_RUN".to_string(),
                dry_run: true,
                transaction_id: None,
                site_url: site_url.to_string(),
                summary,
                operations,
            });
        }

        if queue.is_empty() {
            return Err(anyhow!("No hay cambios pendientes en la cola para aplicar"));
        }

        let tx_id = self.db.create_transaction(
            actor_upn,
            site_url,
            site_title,
            source,
            note,
            &summary,
        )?;

        let mut op_summaries = Vec::new();
        let mut success_count = 0usize;
        let mut fail_count = 0usize;

        for (idx, op) in queue.ops.iter().enumerate() {
            let seq = (idx + 1) as i32;
            let op_type = op.op_type_code();
            let label = op.label();
            let forward_json = serde_json::to_string(op).unwrap_or_else(|_| "{}".to_string());

            match self.execute_single_forward(op).await {
                Ok(rev_payload) => {
                    success_count += 1;
                    let op_id = self.db.record_operation(
                        tx_id,
                        seq,
                        op_type,
                        &label,
                        &forward_json,
                        &rev_payload,
                        "APPLIED",
                        None,
                    )?;
                    op_summaries.push(ExecutedOpSummary {
                        op_id: Some(op_id),
                        seq_order: seq,
                        op_type: op_type.to_string(),
                        target: label,
                        status: "APPLIED".to_string(),
                        error: None,
                    });
                }
                Err(err) => {
                    fail_count += 1;
                    let err_str = err.to_string();
                    let noop = ReversePayload::NoOp {
                        reason: "Operación fallida durante la aplicación".to_string(),
                    };
                    let op_id = self.db.record_operation(
                        tx_id,
                        seq,
                        op_type,
                        &label,
                        &forward_json,
                        &noop,
                        "FAILED",
                        Some(&err_str),
                    )?;
                    op_summaries.push(ExecutedOpSummary {
                        op_id: Some(op_id),
                        seq_order: seq,
                        op_type: op_type.to_string(),
                        target: label,
                        status: "FAILED".to_string(),
                        error: Some(err_str),
                    });
                }
            }
        }

        let final_status = match (success_count, fail_count) {
            (_, 0) => "APPLIED",
            (0, _) => "FAILED",
            _ => "PARTIAL",
        };
        self.db.update_transaction_status(tx_id, final_status)?;

        Ok(ExecutionReport {
            status: final_status.to_string(),
            dry_run: false,
            transaction_id: Some(tx_id),
            site_url: site_url.to_string(),
            summary,
            operations: op_summaries,
        })
    }

    async fn execute_single_forward(&self, op: &StagingOp) -> Result<ReversePayload> {
        match op {
            StagingOp::AddUserToGroup {
                site_url,
                group_id,
                user_identifier,
                ..
            } => {
                let added = self
                    .spo
                    .add_user_to_group(site_url, *group_id, user_identifier)
                    .await?;
                Ok(ReversePayload::RemoveAddedUser {
                    site_url: site_url.clone(),
                    group_id: *group_id,
                    user_id: Some(added.id),
                    login_name: added.login_name,
                })
            }
            StagingOp::RemoveUserFromGroup {
                site_url,
                group_id,
                user_id,
                user_identifier,
                ..
            } => {
                let canonical_login = self
                    .spo
                    .remove_user_from_group(site_url, *group_id, *user_id, user_identifier)
                    .await?;
                Ok(ReversePayload::RestoreRemovedUser {
                    site_url: site_url.clone(),
                    group_id: *group_id,
                    login_name: canonical_login,
                })
            }
            StagingOp::CreateLibrary {
                site_url,
                library_title,
                description,
            } => {
                if let Ok(existing) = self.spo.resolve_library(site_url, library_title).await {
                    if existing.title.eq_ignore_ascii_case(library_title) {
                        return Ok(ReversePayload::NoOp {
                            reason: format!("La biblioteca '{}' ya existía", library_title),
                        });
                    }
                }
                let created = self
                    .spo
                    .create_document_library(site_url, library_title, description)
                    .await?;
                Ok(ReversePayload::DeleteCreatedLibrary {
                    site_url: site_url.clone(),
                    library_id: created.id,
                })
            }
            StagingOp::CreateGroup {
                site_url,
                group_title,
                description,
            } => {
                if self.spo.resolve_group(site_url, group_title).await.is_ok() {
                    return Ok(ReversePayload::NoOp {
                        reason: format!("El grupo '{}' ya existía", group_title),
                    });
                }
                let created = self
                    .spo
                    .create_group(site_url, group_title, description)
                    .await?;
                Ok(ReversePayload::DeleteCreatedGroup {
                    site_url: site_url.clone(),
                    group_id: created.id,
                })
            }
            StagingOp::CreateFolder {
                site_url,
                library_title,
                relative_folder_path,
            } => {
                let (srv_url, created_new) = self
                    .spo
                    .ensure_folder_path(site_url, library_title, relative_folder_path)
                    .await?;
                if created_new {
                    Ok(ReversePayload::DeleteCreatedFolder {
                        site_url: site_url.clone(),
                        folder_server_relative_url: srv_url,
                    })
                } else {
                    Ok(ReversePayload::NoOp {
                        reason: format!("La carpeta '{}' ya existía", srv_url),
                    })
                }
            }
            StagingOp::BreakFolderInheritance {
                site_url,
                library_title,
                relative_folder_path,
                copy_parent_roles,
                clear_subscopes,
            } => {
                let (srv_url, _) = self
                    .spo
                    .ensure_folder_path(site_url, library_title, relative_folder_path)
                    .await?;
                let had_unique_before = self
                    .spo
                    .folder_has_unique_roles(site_url, &srv_url)
                    .await
                    .unwrap_or(false);
                self.spo
                    .break_folder_inheritance(
                        site_url,
                        &srv_url,
                        *copy_parent_roles,
                        *clear_subscopes,
                    )
                    .await?;
                Ok(ReversePayload::ResetFolderInheritance {
                    site_url: site_url.clone(),
                    folder_server_relative_url: srv_url,
                    had_unique_roles_before: had_unique_before,
                })
            }
            StagingOp::AssignFolderRole {
                site_url,
                library_title,
                relative_folder_path,
                principal_name,
                principal_type,
                role_name,
                create_group_if_missing,
            } => {
                let (srv_url, _) = self
                    .spo
                    .ensure_folder_path(site_url, library_title, relative_folder_path)
                    .await?;
                let role_def = self.spo.resolve_role_definition(site_url, role_name).await?;

                let principal_id = if principal_type.eq_ignore_ascii_case("SharePointGroup") {
                    match self.spo.resolve_group(site_url, principal_name).await {
                        Ok(g) => g.id,
                        Err(_) if *create_group_if_missing => {
                            let created = self
                                .spo
                                .create_group(
                                    site_url,
                                    principal_name,
                                    "Grupo creado automáticamente por SPOIberia",
                                )
                                .await?;
                            created.id
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    let ensured = self.spo.ensure_user(site_url, principal_name).await?;
                    ensured.id
                };

                self.spo
                    .add_folder_role_assignment(site_url, &srv_url, principal_id, role_def.id)
                    .await?;

                Ok(ReversePayload::RemoveFolderRoleAssignment {
                    site_url: site_url.clone(),
                    folder_server_relative_url: srv_url,
                    principal_id,
                    role_def_id: role_def.id,
                })
            }
            StagingOp::AssignSiteRole {
                site_url,
                group_title,
                role_name,
            } => {
                let grp = self.spo.resolve_group(site_url, group_title).await?;
                let role_def = self.spo.resolve_role_definition(site_url, role_name).await?;
                self.spo
                    .add_site_role_assignment(site_url, grp.id, role_def.id)
                    .await?;
                Ok(ReversePayload::RemoveSiteRoleAssignment {
                    site_url: site_url.clone(),
                    principal_id: grp.id,
                    role_def_id: role_def.id,
                })
            }
            StagingOp::ServerSideCopy {
                source_site_url,
                target_site_url,
                source_server_relative_url,
                target_folder_server_relative_url,
                is_folder,
                overwrite,
            } => {
                self.spo
                    .server_side_copy_item(
                        source_site_url,
                        source_server_relative_url,
                        target_site_url,
                        target_folder_server_relative_url,
                        *is_folder,
                        *overwrite,
                    )
                    .await?;
                let item_name = source_server_relative_url
                    .trim_end_matches('/')
                    .split('/')
                    .next_back()
                    .unwrap_or("item");
                let copied_dest = format!(
                    "{}/{}",
                    target_folder_server_relative_url.trim_end_matches('/'),
                    item_name
                );
                if *is_folder {
                    Ok(ReversePayload::DeleteCreatedFolder {
                        site_url: target_site_url.clone(),
                        folder_server_relative_url: copied_dest,
                    })
                } else {
                    Ok(ReversePayload::NoOp {
                        reason: format!("Archivo copiado server-side en '{}'", copied_dest),
                    })
                }
            }
        }
    }

    /// Revert an entire transaction (or a specific operation `only_op_id`) in LIFO order (`seq_order DESC`).
    pub async fn revert_transaction(
        &self,
        transaction_id: i64,
        only_op_id: Option<i64>,
        dry_run: bool,
    ) -> Result<(TransactionRecord, Vec<ExecutedOpSummary>)> {
        let tx = self
            .db
            .get_transaction(transaction_id)?
            .ok_or_else(|| anyhow!("No existe la transacción #{}", transaction_id))?;

        let mut ops: Vec<TransactionOpRecord> =
            self.db.get_transaction_operations(transaction_id)?;
        // Reverse order (LIFO) for safe rollback
        ops.reverse();

        if let Some(target_op) = only_op_id {
            ops.retain(|o| o.id == target_op);
            if ops.is_empty() {
                return Err(anyhow!(
                    "No se encontró la operación #{} dentro de la transacción #{}",
                    target_op,
                    transaction_id
                ));
            }
        }

        let mut summaries = Vec::new();

        for op in ops {
            if op.status != "APPLIED" {
                summaries.push(ExecutedOpSummary {
                    op_id: Some(op.id),
                    seq_order: op.seq_order,
                    op_type: op.op_type.clone(),
                    target: op.target_label.clone(),
                    status: format!("SKIPPED ({})", op.status),
                    error: None,
                });
                continue;
            }

            if dry_run {
                summaries.push(ExecutedOpSummary {
                    op_id: Some(op.id),
                    seq_order: op.seq_order,
                    op_type: op.op_type.clone(),
                    target: format!("REVERT: {}", op.target_label),
                    status: "DRY_RUN_REVERT".to_string(),
                    error: None,
                });
                continue;
            }

            let rev: ReversePayload = serde_json::from_str(&op.reverse_payload)?;
            match self.execute_single_reverse(&rev).await {
                Ok(()) => {
                    self.db.update_operation_status(op.id, "REVERTED", None)?;
                    summaries.push(ExecutedOpSummary {
                        op_id: Some(op.id),
                        seq_order: op.seq_order,
                        op_type: op.op_type.clone(),
                        target: op.target_label.clone(),
                        status: "REVERTED".to_string(),
                        error: None,
                    });
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.db
                        .update_operation_status(op.id, "APPLIED", Some(&msg))?;
                    summaries.push(ExecutedOpSummary {
                        op_id: Some(op.id),
                        seq_order: op.seq_order,
                        op_type: op.op_type.clone(),
                        target: op.target_label.clone(),
                        status: "REVERT_FAILED".to_string(),
                        error: Some(msg),
                    });
                }
            }
        }

        if !dry_run {
            let all_ops = self.db.get_transaction_operations(transaction_id)?;
            let any_applied = all_ops.iter().any(|o| o.status == "APPLIED");
            let any_reverted = all_ops.iter().any(|o| o.status == "REVERTED");
            let new_tx_status = match (any_applied, any_reverted) {
                (false, true) => "REVERTED",
                (true, true) => "PARTIAL",
                _ => &tx.status,
            };
            self.db
                .update_transaction_status(transaction_id, new_tx_status)?;
        }

        let updated_tx = self
            .db
            .get_transaction(transaction_id)?
            .unwrap_or(tx);
        Ok((updated_tx, summaries))
    }

    async fn execute_single_reverse(&self, rev: &ReversePayload) -> Result<()> {
        match rev {
            ReversePayload::RemoveAddedUser {
                site_url,
                group_id,
                user_id,
                login_name,
            } => {
                self.spo
                    .remove_user_from_group(site_url, *group_id, *user_id, login_name)
                    .await?;
                Ok(())
            }
            ReversePayload::RestoreRemovedUser {
                site_url,
                group_id,
                login_name,
            } => {
                self.spo
                    .add_user_to_group(site_url, *group_id, login_name)
                    .await?;
                Ok(())
            }
            ReversePayload::DeleteCreatedGroup { site_url, group_id } => {
                self.spo.delete_group(site_url, *group_id).await
            }
            ReversePayload::DeleteCreatedLibrary {
                site_url,
                library_id,
            } => self.spo.delete_document_library(site_url, library_id).await,
            ReversePayload::DeleteCreatedFolder {
                site_url,
                folder_server_relative_url,
            } => {
                self.spo
                    .delete_folder(site_url, folder_server_relative_url)
                    .await
            }
            ReversePayload::ResetFolderInheritance {
                site_url,
                folder_server_relative_url,
                had_unique_roles_before,
            } => {
                if !had_unique_roles_before {
                    self.spo
                        .reset_folder_inheritance(site_url, folder_server_relative_url)
                        .await?;
                }
                Ok(())
            }
            ReversePayload::RemoveFolderRoleAssignment {
                site_url,
                folder_server_relative_url,
                principal_id,
                role_def_id,
            } => {
                self.spo
                    .remove_folder_role_assignment(
                        site_url,
                        folder_server_relative_url,
                        *principal_id,
                        *role_def_id,
                    )
                    .await
            }
            ReversePayload::RemoveSiteRoleAssignment {
                site_url,
                principal_id,
                role_def_id,
            } => {
                self.spo
                    .remove_site_role_assignment(site_url, *principal_id, *role_def_id)
                    .await
            }
            ReversePayload::NoOp { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staging_queue_neutralization() {
        let mut q = StagingQueue::new();
        let site = "https://acciona365.sharepoint.com/sites/Test";

        q.push(StagingOp::AddUserToGroup {
            site_url: site.to_string(),
            group_id: 7,
            group_title: "Owners".to_string(),
            user_identifier: "ana@acciona.com".to_string(),
            user_display_name: "Ana".to_string(),
        });
        assert_eq!(q.len(), 1);
        assert_eq!(q.user_group_staging_state(site, 7, "ana@acciona.com"), Some(true));

        // Now push the opposite operation (Remove) for the same user & group -> should neutralize
        q.push(StagingOp::RemoveUserFromGroup {
            site_url: site.to_string(),
            group_id: 7,
            group_title: "Owners".to_string(),
            user_id: None,
            user_identifier: "ana@acciona.com".to_string(),
            user_display_name: "Ana".to_string(),
        });
        assert!(q.is_empty());
        assert_eq!(q.user_group_staging_state(site, 7, "ana@acciona.com"), None);
    }
}
