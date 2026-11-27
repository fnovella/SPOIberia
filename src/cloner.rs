use crate::api::spo::SpoClient;
use crate::models::{CloneConfig, CloneMode, GroupKind, SpoGroup, StagingOp};
use crate::staging::StagingQueue;
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};

pub struct SiteClonerEngine {
    spo: SpoClient,
}

impl SiteClonerEngine {
    pub fn new(spo: SpoClient) -> Self {
        Self { spo }
    }

    /// Build a complete `StagingQueue` of atomic operations to clone `source_site_url` into `target_site_url`.
    pub async fn plan_clone(&self, cfg: &CloneConfig) -> Result<StagingQueue> {
        let mut queue = StagingQueue::new();
        let src_url = cfg.source_site_url.trim_end_matches('/');
        let tgt_url = cfg.target_site_url.trim_end_matches('/');

        if src_url.eq_ignore_ascii_case(tgt_url) {
            return Err(anyhow!(
                "El sitio origen y el sitio destino no pueden ser el mismo ('{}')",
                src_url
            ));
        }

        let src_groups = self.spo.get_site_groups(src_url).await?;
        let tgt_groups = self.spo.get_site_groups(tgt_url).await?;
        let src_libs = self.spo.get_document_libraries(src_url).await?;
        let tgt_libs = self.spo.get_document_libraries(tgt_url).await?;

        let mut deleted_tgt_lib_ids: HashSet<String> = HashSet::new();
        let base_sub_clean = cfg.target_base_folder.trim_matches('/');

        // Step 0 (Destructive Mode only): Clean target groups/members and target libraries/folders/files
        if cfg.mode == CloneMode::Destructive {
            if cfg.include_groups {
                let src_custom_names: HashSet<String> = src_groups
                    .iter()
                    .filter(|g| g.kind == GroupKind::Custom)
                    .map(|g| g.title.to_lowercase())
                    .collect();

                for tgt_g in &tgt_groups {
                    if tgt_g.kind == GroupKind::Custom
                        && !src_custom_names.contains(&tgt_g.title.to_lowercase())
                    {
                        queue.push(StagingOp::DeleteGroup {
                            site_url: tgt_url.to_string(),
                            group_id: tgt_g.id,
                            group_title: tgt_g.title.clone(),
                        });
                    } else {
                        let members = self
                            .spo
                            .get_group_members(tgt_url, tgt_g.id)
                            .await
                            .unwrap_or_default();
                        for m in members {
                            let ident = m.clean_identifier();
                            if ident.is_empty() {
                                continue;
                            }
                            // Protect the current operator from being removed from AssociatedOwner
                            if tgt_g.kind == GroupKind::AssociatedOwner {
                                if let Some(ref actor) = cfg.current_actor_upn {
                                    if ident.eq_ignore_ascii_case(actor.trim()) {
                                        continue;
                                    }
                                }
                            }
                            queue.push(StagingOp::RemoveUserFromGroup {
                                site_url: tgt_url.to_string(),
                                group_id: tgt_g.id,
                                group_title: tgt_g.title.clone(),
                                user_id: Some(m.id),
                                user_identifier: ident,
                                user_display_name: m.title,
                            });
                        }
                    }
                }
            }

            if cfg.include_files {
                if base_sub_clean.is_empty() {
                    for tgt_lib in &tgt_libs {
                        if !tgt_lib.is_default_documents {
                            deleted_tgt_lib_ids.insert(tgt_lib.id.to_lowercase());
                            queue.push(StagingOp::DeleteLibrary {
                                site_url: tgt_url.to_string(),
                                library_id: tgt_lib.id.clone(),
                                library_title: tgt_lib.title.clone(),
                            });
                        } else {
                            let root_items = self
                                .spo
                                .list_library_root_items(tgt_url, &tgt_lib.server_relative_url)
                                .await
                                .unwrap_or_default();
                            for item in root_items {
                                if item.is_folder {
                                    queue.push(StagingOp::DeleteFolder {
                                        site_url: tgt_url.to_string(),
                                        folder_server_relative_url: item.server_relative_url,
                                    });
                                } else {
                                    queue.push(StagingOp::DeleteFile {
                                        site_url: tgt_url.to_string(),
                                        file_server_relative_url: item.server_relative_url,
                                    });
                                }
                            }
                        }
                    }
                } else {
                    for tgt_lib in &tgt_libs {
                        let sub_url = format!(
                            "{}/{}",
                            tgt_lib.server_relative_url.trim_end_matches('/'),
                            base_sub_clean
                        );
                        if self.spo.folder_exists(tgt_url, &sub_url).await.unwrap_or(false) {
                            queue.push(StagingOp::DeleteFolder {
                                site_url: tgt_url.to_string(),
                                folder_server_relative_url: sub_url,
                            });
                        }
                    }
                }
            }
        }

        let tgt_owner_grp = tgt_groups
            .iter()
            .find(|g| g.kind == GroupKind::AssociatedOwner);
        let tgt_member_grp = tgt_groups
            .iter()
            .find(|g| g.kind == GroupKind::AssociatedMember);
        let tgt_visitor_grp = tgt_groups
            .iter()
            .find(|g| g.kind == GroupKind::AssociatedVisitor);

        let tgt_group_by_title: HashMap<String, SpoGroup> = tgt_groups
            .iter()
            .map(|g| (g.title.to_lowercase(), g.clone()))
            .collect();

        // Map source group title -> target group title (handling associated Owner/Member/Visitor groups automatically)
        let mut group_name_map: HashMap<String, String> = HashMap::new();

        // Step 1: Clone Groups, Members, and Site-Level Role Assignments
        if cfg.include_groups {
            for src_grp in &src_groups {
                match src_grp.kind {
                    GroupKind::AssociatedOwner => {
                        if let Some(tgt_g) = tgt_owner_grp {
                            group_name_map.insert(src_grp.title.clone(), tgt_g.title.clone());
                            self.stage_group_members_sync(
                                src_url,
                                tgt_url,
                                src_grp.id,
                                tgt_g.id,
                                &tgt_g.title,
                                &mut queue,
                            )
                            .await?;
                        }
                    }
                    GroupKind::AssociatedMember => {
                        if let Some(tgt_g) = tgt_member_grp {
                            group_name_map.insert(src_grp.title.clone(), tgt_g.title.clone());
                            self.stage_group_members_sync(
                                src_url,
                                tgt_url,
                                src_grp.id,
                                tgt_g.id,
                                &tgt_g.title,
                                &mut queue,
                            )
                            .await?;
                        }
                    }
                    GroupKind::AssociatedVisitor => {
                        if let Some(tgt_g) = tgt_visitor_grp {
                            group_name_map.insert(src_grp.title.clone(), tgt_g.title.clone());
                            self.stage_group_members_sync(
                                src_url,
                                tgt_url,
                                src_grp.id,
                                tgt_g.id,
                                &tgt_g.title,
                                &mut queue,
                            )
                            .await?;
                        }
                    }
                    GroupKind::Custom => {
                        group_name_map.insert(src_grp.title.clone(), src_grp.title.clone());
                        if !tgt_group_by_title.contains_key(&src_grp.title.to_lowercase()) {
                            queue.push(StagingOp::CreateGroup {
                                site_url: tgt_url.to_string(),
                                group_title: src_grp.title.clone(),
                                description: src_grp.description.clone(),
                            });
                        }
                        for role in &src_grp.permission_levels {
                            queue.push(StagingOp::AssignSiteRole {
                                site_url: tgt_url.to_string(),
                                group_title: src_grp.title.clone(),
                                role_name: role.clone(),
                            });
                        }
                        if let Some(existing_tgt) =
                            tgt_group_by_title.get(&src_grp.title.to_lowercase())
                        {
                            self.stage_group_members_sync(
                                src_url,
                                tgt_url,
                                src_grp.id,
                                existing_tgt.id,
                                &existing_tgt.title,
                                &mut queue,
                            )
                            .await?;
                        }
                    }
                }
            }
        }

        // Step 2: Document Libraries & Server-Side Copy of Folders and Files
        let tgt_default_lib = tgt_libs
            .iter()
            .find(|l| l.is_default_documents && !deleted_tgt_lib_ids.contains(&l.id.to_lowercase()));

        for src_lib in &src_libs {
            if let Some(ref filter) = cfg.libraries_filter {
                if !filter.is_empty()
                    && !filter
                        .iter()
                        .any(|f| f.eq_ignore_ascii_case("all") || f.eq_ignore_ascii_case(&src_lib.title))
                {
                    continue;
                }
            }

            // Map default Documents library across languages (e.g. "Documentos" <-> "Documents")
            let mapped_tgt_lib = if src_lib.is_default_documents {
                tgt_default_lib.or_else(|| {
                    tgt_libs.iter().find(|l| {
                        !deleted_tgt_lib_ids.contains(&l.id.to_lowercase())
                            && l.title.eq_ignore_ascii_case(&src_lib.title)
                    })
                })
            } else {
                tgt_libs.iter().find(|l| {
                    !deleted_tgt_lib_ids.contains(&l.id.to_lowercase())
                        && l.title.eq_ignore_ascii_case(&src_lib.title)
                })
            };

            let (tgt_lib_title, tgt_lib_root_url) = match mapped_tgt_lib {
                Some(tlib) => (tlib.title.clone(), tlib.server_relative_url.clone()),
                None => {
                    queue.push(StagingOp::CreateLibrary {
                        site_url: tgt_url.to_string(),
                        library_title: src_lib.title.clone(),
                        description: format!("Biblioteca clonada desde {}", src_url),
                    });
                    let tgt_site_path = url::Url::parse(tgt_url)
                        .map(|u| u.path().trim_end_matches('/').to_string())
                        .unwrap_or_default();
                    (
                        src_lib.title.clone(),
                        format!("{}/{}", tgt_site_path, src_lib.title),
                    )
                }
            };

            let base_sub = cfg.target_base_folder.trim_matches('/');
            let effective_tgt_root = if base_sub.is_empty() {
                tgt_lib_root_url.trim_end_matches('/').to_string()
            } else {
                queue.push(StagingOp::CreateFolder {
                    site_url: tgt_url.to_string(),
                    library_title: tgt_lib_title.clone(),
                    relative_folder_path: base_sub.to_string(),
                });
                format!("{}/{}", tgt_lib_root_url.trim_end_matches('/'), base_sub)
            };

            if cfg.include_files {
                let root_items = self
                    .spo
                    .list_library_root_items(src_url, &src_lib.server_relative_url)
                    .await
                    .unwrap_or_default();

                for item in root_items {
                    queue.push(StagingOp::ServerSideCopy {
                        source_site_url: src_url.to_string(),
                        target_site_url: tgt_url.to_string(),
                        source_server_relative_url: item.server_relative_url,
                        target_folder_server_relative_url: effective_tgt_root.clone(),
                        is_folder: item.is_folder,
                        overwrite: cfg.overwrite,
                    });
                }
            }

            // Step 3: Replicate Unique Folder Permissions (Broken Inheritance) top-down by depth
            if cfg.include_unique_permissions {
                let unique_folders = self
                    .spo
                    .get_folders_with_unique_permissions(
                        src_url,
                        &src_lib.title,
                        &src_lib.server_relative_url,
                    )
                    .await
                    .unwrap_or_default();

                for ufolder in unique_folders {
                    let target_rel_path = if base_sub.is_empty() {
                        ufolder.relative_path.clone()
                    } else {
                        format!("{}/{}", base_sub, ufolder.relative_path.trim_matches('/'))
                    };

                    queue.push(StagingOp::BreakFolderInheritance {
                        site_url: tgt_url.to_string(),
                        library_title: tgt_lib_title.clone(),
                        relative_folder_path: target_rel_path.clone(),
                        copy_parent_roles: false,
                        clear_subscopes: true,
                    });

                    for ra in ufolder.role_assignments {
                        let mapped_principal = if ra.principal_type == "SharePointGroup" {
                            group_name_map
                                .get(&ra.principal_title)
                                .cloned()
                                .unwrap_or(ra.principal_title.clone())
                        } else {
                            ra.principal_login_name
                                .split('|')
                                .next_back()
                                .unwrap_or(&ra.principal_login_name)
                                .to_string()
                        };

                        for role in ra.roles {
                            queue.push(StagingOp::AssignFolderRole {
                                site_url: tgt_url.to_string(),
                                library_title: tgt_lib_title.clone(),
                                relative_folder_path: target_rel_path.clone(),
                                principal_name: mapped_principal.clone(),
                                principal_type: ra.principal_type.clone(),
                                role_name: role,
                                create_group_if_missing: true,
                            });
                        }
                    }
                }
            }
        }

        Ok(queue)
    }

    async fn stage_group_members_sync(
        &self,
        src_url: &str,
        tgt_url: &str,
        src_group_id: i64,
        tgt_group_id: i64,
        tgt_group_title: &str,
        queue: &mut StagingQueue,
    ) -> Result<()> {
        let members = self
            .spo
            .get_group_members(src_url, src_group_id)
            .await
            .unwrap_or_default();
        for user in members {
            let ident = user.clean_identifier();
            if ident.is_empty() {
                continue;
            }
            queue.push(StagingOp::AddUserToGroup {
                site_url: tgt_url.to_string(),
                group_id: tgt_group_id,
                group_title: tgt_group_title.to_string(),
                user_identifier: ident,
                user_display_name: user.title,
            });
        }
        Ok(())
    }
}
