use crate::i18n::Lang;
use crate::models::{
    SpoLibrary, StagingOp,
};


use super::*;

impl App {
    pub fn open_sync_menu(&mut self, scope: SyncMenuScope) {
        if scope == SyncMenuScope::SelectedLibrary && self.selected_library().is_none() {
            self.status_toast = Some((
                self.lang
                    .tr(
                        "No hay ninguna biblioteca seleccionada",
                        "No library selected",
                    )
                    .to_string(),
                true,
            ));
            return;
        }
        self.modal = Modal::SyncControlMenu {
            scope,
            selected_idx: 0,
        };
    }

    pub fn effective_library_sync_excluded(&self, lib: &SpoLibrary) -> bool {
        let Some(site_url) = self.current_site.as_ref().map(|s| s.url.as_str()) else {
            return lib.exclude_from_offline_client;
        };
        self.staging
            .library_sync_staging_state(site_url, &lib.id)
            .unwrap_or(lib.exclude_from_offline_client)
    }

    pub fn effective_library_shortcuts_hidden(&self, lib: &SpoLibrary) -> bool {
        let Some(site_url) = self.current_site.as_ref().map(|s| s.url.as_str()) else {
            return lib.shortcut_button_hidden;
        };
        self.staging
            .library_shortcut_staging_state(site_url, &lib.id)
            .unwrap_or(lib.shortcut_button_hidden)
    }

    pub fn effective_site_sync_excluded(&self) -> bool {
        let Some(site_url) = self.current_site.as_ref().map(|s| s.url.as_str()) else {
            return self.site_exclude_from_offline_client.unwrap_or(false);
        };
        self.staging
            .site_sync_staging_state(site_url)
            .or(self.site_exclude_from_offline_client)
            .unwrap_or(false)
    }

    pub fn effective_all_libraries_sync_excluded(&self) -> bool {
        self.effective_site_sync_excluded()
            && self
                .libraries
                .iter()
                .all(|l| self.effective_library_sync_excluded(l))
    }

    pub fn effective_all_libraries_shortcuts_hidden(&self) -> bool {
        !self.libraries.is_empty()
            && self
                .libraries
                .iter()
                .all(|l| self.effective_library_shortcuts_hidden(l))
    }

    /// Toggle SharePoint offline synchronization (`ExcludeFromOfflineClient`) for the currently selected document library in Staging.
    pub fn toggle_selected_library_offline_sync(&mut self) {
        let Some(site_url) = self.current_site.as_ref().map(|s| s.url.clone()) else {
            return;
        };
        let Some(lib) = self.selected_library().cloned() else {
            self.status_toast = Some((
                self.lang
                    .tr("No hay ninguna biblioteca seleccionada", "No library selected")
                    .to_string(),
                true,
            ));
            return;
        };
        let effective_excluded = self.effective_library_sync_excluded(&lib);
        let target_exclude = !effective_excluded;

        self.staging.push(StagingOp::SetLibraryOfflineSync {
            site_url: site_url.clone(),
            library_id: lib.id.clone(),
            library_title: lib.title.clone(),
            exclude_from_offline_client: target_exclude,
            apply_view_formatting: false,
        });

        self.status_toast = Some((
            match (self.staging.library_sync_staging_state(&site_url, &lib.id), self.lang) {
                (Some(true), Lang::Es) => format!(
                    "Encolado bloqueo de sync SharePoint en '{}'. Pulsa 'A' para Aplicar.",
                    lib.title
                ),
                (Some(true), Lang::En) => format!(
                    "Queued SharePoint sync block on '{}'. Press 'A' to Apply.",
                    lib.title
                ),
                (Some(false), Lang::Es) => format!(
                    "Encolada activación de sync SharePoint en '{}'. Pulsa 'A' para Aplicar.",
                    lib.title
                ),
                (Some(false), Lang::En) => format!(
                    "Queued SharePoint sync enable on '{}'. Press 'A' to Apply.",
                    lib.title
                ),
                (None, Lang::Es) => format!(
                    "Cancelado cambio pendiente de sync SharePoint en '{}' (neutralizado en Staging).",
                    lib.title
                ),
                (None, Lang::En) => format!(
                    "Cancelled pending SharePoint sync change on '{}' (neutralized in Staging).",
                    lib.title
                ),
            },
            false,
        ));
    }

    /// Toggle OneDrive shortcut button visibility (`CustomFormatter`) for the currently selected document library in Staging.
    pub fn toggle_selected_library_onedrive_shortcuts(&mut self) {
        let Some(site_url) = self.current_site.as_ref().map(|s| s.url.clone()) else {
            return;
        };
        let Some(lib) = self.selected_library().cloned() else {
            self.status_toast = Some((
                self.lang
                    .tr("No hay ninguna biblioteca seleccionada", "No library selected")
                    .to_string(),
                true,
            ));
            return;
        };
        let effective_hidden = self.effective_library_shortcuts_hidden(&lib);
        let target_hide = !effective_hidden;

        self.staging.push(StagingOp::SetLibraryOneDriveShortcuts {
            site_url: site_url.clone(),
            library_id: lib.id.clone(),
            library_title: lib.title.clone(),
            hide_shortcut_button: target_hide,
        });

        self.status_toast = Some((
            match (self.staging.library_shortcut_staging_state(&site_url, &lib.id), self.lang) {
                (Some(true), Lang::Es) => format!(
                    "Encolado ocultar acceso directo OneDrive en '{}'. Pulsa 'A' para Aplicar.",
                    lib.title
                ),
                (Some(true), Lang::En) => format!(
                    "Queued hiding OneDrive shortcut on '{}'. Press 'A' to Apply.",
                    lib.title
                ),
                (Some(false), Lang::Es) => format!(
                    "Encolado mostrar acceso directo OneDrive en '{}'. Pulsa 'A' para Aplicar.",
                    lib.title
                ),
                (Some(false), Lang::En) => format!(
                    "Queued showing OneDrive shortcut on '{}'. Press 'A' to Apply.",
                    lib.title
                ),
                (None, Lang::Es) => format!(
                    "Cancelado cambio pendiente de accesos OneDrive en '{}' (neutralizado en Staging).",
                    lib.title
                ),
                (None, Lang::En) => format!(
                    "Cancelled pending OneDrive shortcut change on '{}' (neutralized in Staging).",
                    lib.title
                ),
            },
            false,
        ));
    }

    /// Toggle both SharePoint offline sync and OneDrive shortcut buttons for the selected library.
    pub fn toggle_selected_library_sync_and_shortcuts(&mut self) {
        let Some(site_url) = self.current_site.as_ref().map(|s| s.url.clone()) else {
            return;
        };
        let Some(lib) = self.selected_library().cloned() else {
            self.status_toast = Some((
                self.lang
                    .tr("No hay ninguna biblioteca seleccionada", "No library selected")
                    .to_string(),
                true,
            ));
            return;
        };
        let sync_eff = self.effective_library_sync_excluded(&lib);
        let od_eff = self.effective_library_shortcuts_hidden(&lib);
        let target_block = !(sync_eff && od_eff);

        if sync_eff != target_block {
            self.staging.push(StagingOp::SetLibraryOfflineSync {
                site_url: site_url.clone(),
                library_id: lib.id.clone(),
                library_title: lib.title.clone(),
                exclude_from_offline_client: target_block,
                apply_view_formatting: false,
            });
        }
        if od_eff != target_block {
            self.staging.push(StagingOp::SetLibraryOneDriveShortcuts {
                site_url: site_url.clone(),
                library_id: lib.id.clone(),
                library_title: lib.title.clone(),
                hide_shortcut_button: target_block,
            });
        }

        let any_pending = self
            .staging
            .library_sync_staging_state(&site_url, &lib.id)
            .is_some()
            || self
                .staging
                .library_shortcut_staging_state(&site_url, &lib.id)
                .is_some();

        let msg = match (any_pending, target_block, self.lang) {
            (false, _, Lang::Es) => format!(
                "Cancelados cambios pendientes de sync/OneDrive en '{}' (neutralizado en Staging).",
                lib.title
            ),
            (false, _, Lang::En) => format!(
                "Cancelled pending sync/OneDrive changes on '{}' (neutralized in Staging).",
                lib.title
            ),
            (true, true, Lang::Es) => format!(
                "Encolado bloqueo de sync SharePoint y acceso directo OneDrive en '{}'. Pulsa 'A' para Aplicar.",
                lib.title
            ),
            (true, true, Lang::En) => format!(
                "Queued blocking SharePoint sync and OneDrive shortcut on '{}'. Press 'A' to Apply.",
                lib.title
            ),
            (true, false, Lang::Es) => format!(
                "Encolada activación de sync SharePoint y acceso directo OneDrive en '{}'. Pulsa 'A' para Aplicar.",
                lib.title
            ),
            (true, false, Lang::En) => format!(
                "Queued enabling SharePoint sync and OneDrive shortcut on '{}'. Press 'A' to Apply.",
                lib.title
            ),
        };
        self.status_toast = Some((msg, false));
    }

    /// Toggle SharePoint offline synchronization (`ExcludeFromOfflineClient`) for the whole site (`SP.Web`) and all its loaded libraries in Staging.
    pub fn toggle_site_and_libraries_offline_sync(&mut self) {
        let Some((site_url, site_title)) = self
            .current_site
            .as_ref()
            .map(|s| (s.url.clone(), s.title.clone()))
        else {
            return;
        };

        let site_eff = self.effective_site_sync_excluded();
        let all_libs_excluded = self
            .libraries
            .iter()
            .all(|l| self.effective_library_sync_excluded(l));
        let target_exclude = !(site_eff && all_libs_excluded);

        if site_eff != target_exclude {
            self.staging.push(StagingOp::SetSiteOfflineSync {
                site_url: site_url.clone(),
                site_title: site_title.clone(),
                exclude_from_offline_client: target_exclude,
            });
        }

        for lib in self.libraries.clone() {
            let lib_eff = self.effective_library_sync_excluded(&lib);
            if lib_eff != target_exclude {
                self.staging.push(StagingOp::SetLibraryOfflineSync {
                    site_url: site_url.clone(),
                    library_id: lib.id,
                    library_title: lib.title,
                    exclude_from_offline_client: target_exclude,
                    apply_view_formatting: false,
                });
            }
        }

        let any_sync_pending = self.staging.site_sync_staging_state(&site_url).is_some()
            || self
                .libraries
                .iter()
                .any(|l| self.staging.library_sync_staging_state(&site_url, &l.id).is_some());

        let msg = match (any_sync_pending, target_exclude, self.lang) {
            (false, _, Lang::Es) => format!(
                "Cancelados los cambios pendientes de sync SharePoint en '{}' (Staging neutralizado).",
                site_title
            ),
            (false, _, Lang::En) => format!(
                "Cancelled pending SharePoint sync changes on '{}' (Staging neutralized).",
                site_title
            ),
            (true, true, Lang::Es) => format!(
                "Encolado bloqueo de sync SharePoint en sitio '{}' y sus bibliotecas. Pulsa 'A' para Aplicar.",
                site_title
            ),
            (true, true, Lang::En) => format!(
                "Queued blocking SharePoint sync on site '{}' and its libraries. Press 'A' to Apply.",
                site_title
            ),
            (true, false, Lang::Es) => format!(
                "Encolada activación de sync SharePoint en sitio '{}' y sus bibliotecas. Pulsa 'A' para Aplicar.",
                site_title
            ),
            (true, false, Lang::En) => format!(
                "Queued enabling SharePoint sync on site '{}' and its libraries. Press 'A' to Apply.",
                site_title
            ),
        };
        self.status_toast = Some((msg, false));
    }

    /// Toggle OneDrive shortcut buttons (`CustomFormatter`) for all loaded libraries of the current site in Staging.
    pub fn toggle_site_and_libraries_onedrive_shortcuts(&mut self) {
        let Some((site_url, site_title)) = self
            .current_site
            .as_ref()
            .map(|s| (s.url.clone(), s.title.clone()))
        else {
            return;
        };

        if self.libraries.is_empty() {
            self.status_toast = Some((
                self.lang
                    .tr(
                        "No hay bibliotecas cargadas en este sitio para gestionar accesos directos OneDrive",
                        "No libraries loaded in this site to manage OneDrive shortcuts",
                    )
                    .to_string(),
                true,
            ));
            return;
        }

        let all_hidden = self.effective_all_libraries_shortcuts_hidden();
        let target_hide = !all_hidden;

        for lib in self.libraries.clone() {
            let lib_eff = self.effective_library_shortcuts_hidden(&lib);
            if lib_eff != target_hide {
                self.staging.push(StagingOp::SetLibraryOneDriveShortcuts {
                    site_url: site_url.clone(),
                    library_id: lib.id,
                    library_title: lib.title,
                    hide_shortcut_button: target_hide,
                });
            }
        }

        let any_pending = self
            .libraries
            .iter()
            .any(|l| self.staging.library_shortcut_staging_state(&site_url, &l.id).is_some());

        let msg = match (any_pending, target_hide, self.lang) {
            (false, _, Lang::Es) => format!(
                "Cancelados cambios pendientes de accesos OneDrive en '{}' (Staging neutralizado).",
                site_title
            ),
            (false, _, Lang::En) => format!(
                "Cancelled pending OneDrive shortcut changes on '{}' (Staging neutralized).",
                site_title
            ),
            (true, true, Lang::Es) => format!(
                "Encolado ocultar accesos directos OneDrive en bibliotecas de '{}'. Pulsa 'A' para Aplicar.",
                site_title
            ),
            (true, true, Lang::En) => format!(
                "Queued hiding OneDrive shortcuts on libraries of '{}'. Press 'A' to Apply.",
                site_title
            ),
            (true, false, Lang::Es) => format!(
                "Encolado mostrar accesos directos OneDrive en bibliotecas de '{}'. Pulsa 'A' para Aplicar.",
                site_title
            ),
            (true, false, Lang::En) => format!(
                "Queued showing OneDrive shortcuts on libraries of '{}'. Press 'A' to Apply.",
                site_title
            ),
        };
        self.status_toast = Some((msg, false));
    }

    /// Toggle both SharePoint offline sync and OneDrive shortcut buttons for the whole site and all its libraries.
    pub fn toggle_site_and_libraries_sync_and_shortcuts(&mut self) {
        let Some((site_url, site_title)) = self
            .current_site
            .as_ref()
            .map(|s| (s.url.clone(), s.title.clone()))
        else {
            return;
        };

        let sync_all = self.effective_all_libraries_sync_excluded();
        let od_all = if self.libraries.is_empty() {
            sync_all
        } else {
            self.effective_all_libraries_shortcuts_hidden()
        };
        let target_block = !(sync_all && od_all);

        let site_eff = self.effective_site_sync_excluded();
        if site_eff != target_block {
            self.staging.push(StagingOp::SetSiteOfflineSync {
                site_url: site_url.clone(),
                site_title: site_title.clone(),
                exclude_from_offline_client: target_block,
            });
        }

        for lib in self.libraries.clone() {
            if self.effective_library_sync_excluded(&lib) != target_block {
                self.staging.push(StagingOp::SetLibraryOfflineSync {
                    site_url: site_url.clone(),
                    library_id: lib.id.clone(),
                    library_title: lib.title.clone(),
                    exclude_from_offline_client: target_block,
                    apply_view_formatting: false,
                });
            }
            if self.effective_library_shortcuts_hidden(&lib) != target_block {
                self.staging.push(StagingOp::SetLibraryOneDriveShortcuts {
                    site_url: site_url.clone(),
                    library_id: lib.id,
                    library_title: lib.title,
                    hide_shortcut_button: target_block,
                });
            }
        }

        let any_pending = self.staging.site_sync_staging_state(&site_url).is_some()
            || self.libraries.iter().any(|l| {
                self.staging
                    .library_sync_staging_state(&site_url, &l.id)
                    .is_some()
                    || self
                        .staging
                        .library_shortcut_staging_state(&site_url, &l.id)
                        .is_some()
            });

        let msg = match (any_pending, target_block, self.lang) {
            (false, _, Lang::Es) => format!(
                "Cancelados cambios pendientes de sync/OneDrive en '{}' (Staging neutralizado).",
                site_title
            ),
            (false, _, Lang::En) => format!(
                "Cancelled pending sync/OneDrive changes on '{}' (Staging neutralized).",
                site_title
            ),
            (true, true, Lang::Es) => format!(
                "Encolado bloqueo de sync SharePoint y accesos OneDrive en '{}'. Pulsa 'A' para Aplicar.",
                site_title
            ),
            (true, true, Lang::En) => format!(
                "Queued blocking SharePoint sync and OneDrive shortcuts on '{}'. Press 'A' to Apply.",
                site_title
            ),
            (true, false, Lang::Es) => format!(
                "Encolada activación de sync SharePoint y accesos OneDrive en '{}'. Pulsa 'A' para Aplicar.",
                site_title
            ),
            (true, false, Lang::En) => format!(
                "Queued enabling SharePoint sync and OneDrive shortcuts on '{}'. Press 'A' to Apply.",
                site_title
            ),
        };
        self.status_toast = Some((msg, false));
    }

}
