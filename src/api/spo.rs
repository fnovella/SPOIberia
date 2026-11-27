use crate::api::graph::GraphClient;
use crate::api::{check_json_response, send_with_retry};
use crate::auth::AuthManager;
use crate::models::{
    DirectoryUser, GroupKind, LibrarySyncStatus, SiteRole, SiteSyncStatus, SpoGroup, SpoSite,
    SpoUser,
};
use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

pub(crate) const SPO_JSON_ACCEPT: &str = "application/json;odata=nometadata";
pub(crate) const SPO_JSON_CONTENT: &str = "application/json;odata=nometadata";
pub(crate) const SPO_JSON_VERBOSE: &str = "application/json;odata=verbose";

#[derive(Clone)]
pub struct SpoClient {
    pub(crate) http: reqwest::Client,
    pub(crate) auth: AuthManager,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SpoValueList<T> {
    pub(crate) value: Vec<T>,
    #[serde(rename = "odata.nextLink", alias = "@odata.nextLink", default)]
    pub(crate) next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebRoleCheckResponse {
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "Url")]
    url: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "CurrentUser")]
    current_user: Option<WebCurrentUser>,
    #[serde(rename = "EffectiveBasePermissions")]
    effective_base_permissions: Option<BasePermissionsMask>,
    #[serde(rename = "AssociatedOwnerGroup")]
    associated_owner_group: Option<IdOnly>,
}

#[derive(Debug, Deserialize)]
struct WebCurrentUser {
    #[serde(rename = "Id")]
    id: Option<i64>,
    #[serde(rename = "IsSiteAdmin", default)]
    is_site_admin: bool,
}

#[derive(Debug, Deserialize)]
pub struct BasePermissionsMask {
    #[serde(rename = "High")]
    pub high: serde_json::Value,
    #[serde(rename = "Low")]
    pub low: serde_json::Value,
}

impl BasePermissionsMask {
    /// SharePoint Site Collection Administrators have all high permission bits set (`0x7FFF_FFFF`).
    pub fn is_full_site_admin(&self) -> bool {
        let high_u64 = Self::val_to_u64(&self.high);
        (high_u64 & 0x7FFF_FFFF) == 0x7FFF_FFFF
    }

    /// Check SharePoint site ownership/permission management bits:
    /// `CreateGroups` (bit 25 -> `1 << 24`), `ManagePermissions` (bit 26 -> `1 << 25`),
    /// `ManageWeb` (bit 31 -> `1 << 30`), or `EnumeratePermissions` (`High` `1 << 30`).
    pub fn has_manage_permissions(&self) -> bool {
        let low_u64 = Self::val_to_u64(&self.low);
        let high_u64 = Self::val_to_u64(&self.high);
        let create_groups_bit: u64 = 1 << 24;      // PermissionKind.CreateGroups = 25
        let manage_permissions_bit: u64 = 1 << 25; // PermissionKind.ManagePermissions = 26
        let manage_web_bit: u64 = 1 << 30;         // PermissionKind.ManageWeb = 31
        let enumerate_perms_high: u64 = 1 << 30;   // PermissionKind.EnumeratePermissions = 63

        (low_u64 & manage_permissions_bit) != 0
            || (low_u64 & manage_web_bit) != 0
            || (low_u64 & create_groups_bit) != 0
            || (high_u64 & enumerate_perms_high) != 0
    }

    fn val_to_u64(v: &serde_json::Value) -> u64 {
        match v {
            serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
            serde_json::Value::String(s) => s.parse::<u64>().unwrap_or(0),
            _ => 0,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WebAssociatedGroupsResponse {
    #[serde(rename = "AssociatedOwnerGroup")]
    associated_owner_group: Option<IdOnly>,
    #[serde(rename = "AssociatedMemberGroup")]
    associated_member_group: Option<IdOnly>,
    #[serde(rename = "AssociatedVisitorGroup")]
    associated_visitor_group: Option<IdOnly>,
}

#[derive(Debug, Deserialize)]
struct IdOnly {
    #[serde(rename = "Id")]
    id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawSpoGroup {
    #[serde(rename = "Id")]
    id: i64,
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "OwnerTitle")]
    owner_title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRoleAssignment {
    #[serde(rename = "PrincipalId")]
    principal_id: i64,
    #[serde(rename = "RoleDefinitionBindings", default)]
    role_definition_bindings: Vec<RawRoleDefBinding>,
}

#[derive(Debug, Deserialize)]
struct RawRoleDefBinding {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawSpoUser {
    #[serde(rename = "Id")]
    id: i64,
    #[serde(rename = "Title")]
    title: Option<String>,
    #[serde(rename = "Email")]
    email: Option<String>,
    #[serde(rename = "LoginName")]
    login_name: String,
    #[serde(rename = "UserPrincipalName")]
    upn: Option<String>,
    #[serde(rename = "IsSiteAdmin", default)]
    is_site_admin: bool,
    #[serde(rename = "PrincipalType", default)]
    principal_type: i32,
}

#[derive(Debug, Deserialize)]
struct RawWebOfflineSync {
    #[serde(rename = "Title")]
    title: Option<String>,
    #[serde(rename = "ExcludeFromOfflineClient", default)]
    exclude_from_offline_client: bool,
}

#[derive(Debug, Deserialize)]
struct RawSubWeb {
    #[serde(rename = "Url")]
    url: String,
    #[serde(rename = "Title")]
    title: Option<String>,
}

impl SpoClient {
    pub fn new(auth: AuthManager) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("spoiberia/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(45))
            .build()?;
        Ok(Self { http, auth })
    }

    pub(crate) async fn token(&self, site_url: &str) -> Result<String> {
        self.auth.get_spo_token_for_url(site_url).await
    }

    /// Search SharePoint sites via `/_api/search/query`.
    pub async fn search_sites(
        &self,
        root_site_url: &str,
        keyword_filter: Option<&str>,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        self.search_sites_for_user(root_site_url, keyword_filter, None)
            .await
    }

    async fn search_sites_with_query(
        &self,
        root_site_url: &str,
        kql: &str,
        start_row: usize,
        row_limit: usize,
        sort_list: Option<&str>,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let base = root_site_url.trim_end_matches('/');
        let token = self.token(base).await?;

        let mut url = format!(
            "{}/_api/search/query?querytext='{}'&selectproperties='Title,Path,Description'&startrow={}&rowlimit={}&trimduplicates=true",
            base,
            urlencoding::encode(kql),
            start_row,
            row_limit
        );
        if let Some(sort) = sort_list {
            url.push_str(&format!("&sortlist='{}'", urlencoding::encode(sort)));
        }

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let val: serde_json::Value = resp.json().await?;
        Ok(Self::extract_search_rows(&val))
    }

    /// Search SharePoint sites via `/_api/search/query` combining default, recent, and user-related queries.
    pub async fn search_sites_for_user(
        &self,
        root_site_url: &str,
        keyword_filter: Option<&str>,
        user_upn: Option<&str>,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        if let Some(kw) = keyword_filter.map(str::trim).filter(|s| !s.is_empty()) {
            let clean = kw.replace(['\'', '"'], "");
            let kql = format!("contentclass:STS_Site AND ({}* OR Path:*{})", clean, clean);
            return self
                .search_sites_with_query(root_site_url, &kql, 0, 500, None)
                .await;
        }

        let user_kql = user_upn
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|upn| {
                let clean_upn = upn.replace(['\'', '"'], "");
                let mut terms = vec![format!("\"{}\"", clean_upn)];
                if let Some(local) = clean_upn.split('@').next() {
                    let mut tokens: Vec<&str> = local
                        .split(['.', '_', '-'])
                        .filter(|t| t.len() >= 4)
                        .collect();
                    tokens.sort_by_key(|t| std::cmp::Reverse(t.len()));
                    for tok in tokens.into_iter().take(3) {
                        let quoted = format!("\"{}\"", tok);
                        if !terms.contains(&quoted) {
                            terms.push(quoted);
                        }
                    }
                }
                format!("contentclass:STS_Site AND ({})", terms.join(" OR "))
            });

        let (res_default, res_recent, res_user) = tokio::join!(
            self.search_sites_with_query(root_site_url, "contentclass:STS_Site", 0, 500, None),
            self.search_sites_with_query(
                root_site_url,
                "contentclass:STS_Site",
                0,
                500,
                Some("LastModifiedTime:descending"),
            ),
            async {
                match user_kql {
                    Some(ref kql) => {
                        self.search_sites_with_query(root_site_url, kql, 0, 500, None)
                            .await
                    }
                    None => Ok(Vec::new()),
                }
            }
        );

        let mut dedup: HashMap<String, (String, String, Option<String>)> = HashMap::new();
        for rows in [res_default, res_recent, res_user].into_iter().flatten() {
            for (title, path, desc) in rows {
                dedup.entry(path.to_lowercase()).or_insert((title, path, desc));
            }
        }
        Ok(dedup.into_values().collect())
    }

    pub(crate) fn extract_search_rows(
        val: &serde_json::Value,
    ) -> Vec<(String, String, Option<String>)> {
        let mut results = Vec::new();
        let rows = val
            .pointer("/PrimaryQueryResult/RelevantResults/Table/Rows")
            .and_then(|v| v.as_array());

        if let Some(rows_arr) = rows {
            for row in rows_arr {
                if let Some(cells) = row.get("Cells").and_then(|c| c.as_array()) {
                    let mut title = String::new();
                    let mut path = String::new();
                    let mut desc: Option<String> = None;

                    for cell in cells {
                        let key = cell.get("Key").and_then(|k| k.as_str()).unwrap_or("");
                        let value = cell
                            .get("Value")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        match key {
                            "Title" => title = value,
                            "Path" => path = value.trim_end_matches('/').to_string(),
                            "Description" if !value.is_empty() => desc = Some(value),
                            _ => {}
                        }
                    }
                    if !path.is_empty()
                        && !path.contains("-my.sharepoint.com")
                        && !path.contains("/portals/personal/")
                        && !path.contains("/personal/")
                    {
                        if title.is_empty() {
                            title = path.clone();
                        }
                        results.push((title, path, desc));
                    }
                }
            }
        }
        results
    }

    /// Check if the current user is Site Collection Admin or Owner (`ManagePermissions`) on a given site URL.
    pub async fn check_user_site_role(&self, site_url: &str) -> Result<Option<SpoSite>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web?$select=Title,Url,Description,CurrentUser/Id,CurrentUser/IsSiteAdmin,EffectiveBasePermissions,AssociatedOwnerGroup/Id&$expand=CurrentUser,AssociatedOwnerGroup",
            base
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let info: WebRoleCheckResponse = resp.json().await?;
        let is_admin = info
            .current_user
            .as_ref()
            .map(|u| u.is_site_admin)
            .unwrap_or(false)
            || info
                .effective_base_permissions
                .as_ref()
                .map(|p| p.is_full_site_admin())
                .unwrap_or(false);
        let has_manage_perms = info
            .effective_base_permissions
            .as_ref()
            .map(|p| p.has_manage_permissions())
            .unwrap_or(false);

        if !is_admin && !has_manage_perms {
            return Ok(None);
        }

        let role = if is_admin {
            let mut in_owner_group = false;
            if let (Some(uid), Some(_owner_grp_id)) = (
                info.current_user.as_ref().and_then(|u| u.id),
                info.associated_owner_group.as_ref().and_then(|g| g.id),
            ) {
                let owners_url = format!("{}/_api/web/AssociatedOwnerGroup/Users?$select=Id", base);
                if let Ok(owners_resp) = send_with_retry(|| {
                    self.http
                        .get(&owners_url)
                        .bearer_auth(&token)
                        .header("Accept", SPO_JSON_ACCEPT)
                })
                .await
                {
                    if owners_resp.status().is_success() {
                        if let Ok(users) = owners_resp.json::<SpoValueList<IdOnly>>().await {
                            in_owner_group = users.value.iter().any(|u| u.id == Some(uid));
                        }
                    }
                }
            }
            if in_owner_group {
                SiteRole::AdminAndOwner
            } else {
                SiteRole::SiteAdmin
            }
        } else {
            SiteRole::Owner
        };

        Ok(Some(SpoSite {
            title: info.title,
            url: info.url.trim_end_matches('/').to_string(),
            description: info.description.filter(|s| !s.is_empty()),
            role,
            is_site_admin: is_admin,
        }))
    }

    /// Hybrid discovery of all SharePoint sites where the user is Administrator or Owner.
    pub async fn discover_admin_and_owner_sites(
        &self,
        graph: &GraphClient,
        keyword_filter: Option<&str>,
    ) -> Result<Vec<SpoSite>> {
        let (me, root_site) = graph.get_me_and_root_site().await?;
        let user_upn = me.user_principal_name.as_deref().or(me.mail.as_deref());

        let (m365_owned_res, m365_member_res, followed_res, search_candidates_res) = tokio::join!(
            graph.get_owned_m365_group_sites(),
            graph.get_member_m365_group_sites(),
            graph.get_followed_sites(),
            self.search_sites_for_user(&root_site.web_url, keyword_filter, user_upn)
        );

        let kw_low = keyword_filter
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(|k| k.to_lowercase());
        let matches_kw = |title: &str, url: &str| -> bool {
            match &kw_low {
                Some(kw) => title.to_lowercase().contains(kw) || url.to_lowercase().contains(kw),
                None => true,
            }
        };

        let mut by_url: HashMap<String, SpoSite> = HashMap::new();
        let mut candidate_urls: HashMap<String, String> = HashMap::new();

        if let Ok(m365_sites) = m365_owned_res {
            for s in m365_sites {
                if !matches_kw(&s.title, &s.url) {
                    continue;
                }
                let key = s.url.to_lowercase();
                candidate_urls
                    .entry(key.clone())
                    .or_insert_with(|| s.url.clone());
                by_url.insert(key, s);
            }
        }

        if let Ok(member_sites) = m365_member_res {
            for s in member_sites {
                if !matches_kw(&s.title, &s.url) {
                    continue;
                }
                candidate_urls.entry(s.url.to_lowercase()).or_insert(s.url);
            }
        }

        if let Ok(followed_sites) = followed_res {
            for s in followed_sites {
                if !matches_kw(&s.title, &s.url) {
                    continue;
                }
                candidate_urls.entry(s.url.to_lowercase()).or_insert(s.url);
            }
        }

        if let Ok(search_rows) = search_candidates_res {
            for (_title, path, _desc) in search_rows {
                candidate_urls.entry(path.to_lowercase()).or_insert(path);
            }
        }

        let root_url = root_site.web_url.trim_end_matches('/').to_string();
        let root_title = root_site.display_name.as_deref().unwrap_or(&root_url);
        if !root_url.is_empty() && matches_kw(root_title, &root_url) {
            candidate_urls
                .entry(root_url.to_lowercase())
                .or_insert(root_url);
        }

        let spo_clone = self.clone();
        let urls_to_check: Vec<String> = candidate_urls.into_values().collect();
        let verified: Vec<SpoSite> = stream::iter(urls_to_check)
            .map(|path| {
                let c = spo_clone.clone();
                async move { c.check_user_site_role(&path).await.ok().flatten() }
            })
            .buffer_unordered(12)
            .filter_map(|x| async move { x })
            .collect()
            .await;

        for site in verified {
            let key = site.url.to_lowercase();
            if let Some(existing) = by_url.get_mut(&key) {
                if site.is_site_admin {
                    existing.is_site_admin = true;
                    existing.role = SiteRole::AdminAndOwner;
                }
            } else {
                by_url.insert(key, site);
            }
        }

        let mut all_sites: Vec<SpoSite> = by_url.into_values().collect();
        all_sites.sort_by_key(|a| a.title.to_lowercase());
        Ok(all_sites)
    }

    /// List all SharePoint groups in a site, enriched with their associated role (Owner/Member/Visitor/Custom)
    /// and site-level permission bindings.
    pub async fn get_site_groups(&self, site_url: &str) -> Result<Vec<SpoGroup>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;

        let assoc_url = format!(
            "{}/_api/web?$select=AssociatedOwnerGroup/Id,AssociatedMemberGroup/Id,AssociatedVisitorGroup/Id&$expand=AssociatedOwnerGroup,AssociatedMemberGroup,AssociatedVisitorGroup",
            base
        );
        let groups_url = format!(
            "{}/_api/web/sitegroups?$select=Id,Title,Description,OwnerTitle",
            base
        );
        let roles_url = format!(
            "{}/_api/web/roleassignments?$select=PrincipalId,RoleDefinitionBindings/Name&$expand=RoleDefinitionBindings",
            base
        );

        let (assoc_resp, groups_resp, roles_resp) = tokio::try_join!(
            send_with_retry(|| {
                self.http
                    .get(&assoc_url)
                    .bearer_auth(&token)
                    .header("Accept", SPO_JSON_ACCEPT)
            }),
            send_with_retry(|| {
                self.http
                    .get(&groups_url)
                    .bearer_auth(&token)
                    .header("Accept", SPO_JSON_ACCEPT)
            }),
            send_with_retry(|| {
                self.http
                    .get(&roles_url)
                    .bearer_auth(&token)
                    .header("Accept", SPO_JSON_ACCEPT)
            })
        )?;

        let assoc: WebAssociatedGroupsResponse = assoc_resp.json().await.unwrap_or(
            WebAssociatedGroupsResponse {
                associated_owner_group: None,
                associated_member_group: None,
                associated_visitor_group: None,
            },
        );
        let owner_id = assoc.associated_owner_group.and_then(|g| g.id);
        let member_id = assoc.associated_member_group.and_then(|g| g.id);
        let visitor_id = assoc.associated_visitor_group.and_then(|g| g.id);

        let raw_groups: SpoValueList<RawSpoGroup> =
            check_json_response(groups_resp, "Error listando grupos del sitio").await?;

        let mut perms_by_principal: HashMap<i64, Vec<String>> = HashMap::new();
        if roles_resp.status().is_success() {
            if let Ok(ra_list) = roles_resp.json::<SpoValueList<RawRoleAssignment>>().await {
                for ra in ra_list.value {
                    let roles: Vec<String> = ra
                        .role_definition_bindings
                        .into_iter()
                        .map(|r| r.name)
                        .filter(|n| n != "Limited Access" && n != "Acceso limitado")
                        .collect();
                    if !roles.is_empty() {
                        perms_by_principal.insert(ra.principal_id, roles);
                    }
                }
            }
        }

        let mut out: Vec<SpoGroup> = raw_groups
            .value
            .into_iter()
            .filter(|g| !g.title.starts_with("SharingLinks."))
            .map(|g| {
                let kind = if Some(g.id) == owner_id {
                    GroupKind::AssociatedOwner
                } else if Some(g.id) == member_id {
                    GroupKind::AssociatedMember
                } else if Some(g.id) == visitor_id {
                    GroupKind::AssociatedVisitor
                } else {
                    GroupKind::Custom
                };
                let permission_levels = perms_by_principal.remove(&g.id).unwrap_or_default();
                SpoGroup {
                    id: g.id,
                    title: g.title,
                    description: g.description.unwrap_or_default(),
                    owner_title: g.owner_title.unwrap_or_default(),
                    kind,
                    permission_levels,
                }
            })
            .collect();

        out.sort_by(|a, b| {
            a.kind
                .sort_rank()
                .cmp(&b.kind.sort_rank())
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        Ok(out)
    }

    /// Resolve a group by numeric ID or exact/case-insensitive title.
    pub async fn resolve_group(&self, site_url: &str, id_or_name: &str) -> Result<SpoGroup> {
        let groups = self.get_site_groups(site_url).await?;
        if let Ok(id) = id_or_name.trim().parse::<i64>() {
            if let Some(g) = groups.iter().find(|g| g.id == id) {
                return Ok(g.clone());
            }
        }
        let target = id_or_name.trim().to_lowercase();
        groups
            .into_iter()
            .find(|g| g.title.to_lowercase() == target)
            .ok_or_else(|| {
                anyhow!(
                    "No se encontró el grupo '{}' en el sitio '{}'",
                    id_or_name,
                    site_url
                )
            })
    }

    /// List members of a SharePoint group.
    pub async fn get_group_members(&self, site_url: &str, group_id: i64) -> Result<Vec<SpoUser>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/sitegroups({})/users?$select=Id,Title,Email,LoginName,UserPrincipalName,IsSiteAdmin,PrincipalType",
            base, group_id
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: SpoValueList<RawSpoUser> =
            check_json_response(resp, "Error obteniendo miembros del grupo").await?;

        let mut users: Vec<SpoUser> = raw
            .value
            .into_iter()
            .filter(|u| !u.login_name.to_lowercase().contains("sharepoint\\system"))
            .map(|u| SpoUser {
                id: u.id,
                title: u.title.unwrap_or_else(|| u.login_name.clone()),
                email: u.email.unwrap_or_default(),
                login_name: u.login_name,
                upn: u.upn,
                is_site_admin: u.is_site_admin,
                principal_type: u.principal_type,
            })
            .collect();

        users.sort_by_key(|a| a.title.to_lowercase());
        Ok(users)
    }

    /// Search users via SharePoint People Picker (`ClientPeoplePickerSearchUser`).
    pub async fn search_people_picker(
        &self,
        site_url: &str,
        query: &str,
    ) -> Result<Vec<DirectoryUser>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/SP.UI.ApplicationPages.ClientPeoplePickerWebServiceInterface.clientPeoplePickerSearchUser",
            base
        );

        let body = json!({
            "queryParams": {
                "AllowEmailAddresses": true,
                "AllowMultipleEntities": false,
                "AllUrlZones": false,
                "MaximumEntitySuggestions": 20,
                "PrincipalSource": 15,
                "PrincipalType": 1,
                "QueryString": q
            }
        });

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Type", SPO_JSON_CONTENT)
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let outer: serde_json::Value = resp.json().await?;
        let inner_str = outer
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("[]");
        let entities: Vec<serde_json::Value> = serde_json::from_str(inner_str).unwrap_or_default();

        let mut out = Vec::new();
        for ent in entities {
            let key = ent
                .get("Key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let disp = ent
                .get("DisplayText")
                .and_then(|v| v.as_str())
                .unwrap_or(&key)
                .to_string();
            let email = ent
                .pointer("/EntityData/Email")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let dept = ent
                .pointer("/EntityData/Department")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            let title = ent
                .pointer("/EntityData/Title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);

            if !key.is_empty() {
                let upn = key.split('|').next_back().unwrap_or(&key).to_string();
                out.push(DirectoryUser {
                    display_name: disp,
                    email: if email.is_empty() { upn.clone() } else { email },
                    upn,
                    login_name: key,
                    job_title: title,
                    department: dept,
                });
            }
        }
        Ok(out)
    }

    /// Ensure a user exists in the SharePoint site's User Information List and return their `SpoUser` (with `Id` and canonical `LoginName`).
    pub async fn ensure_user(&self, site_url: &str, logon_name: &str) -> Result<SpoUser> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/ensureuser", base);
        let body = json!({ "logonName": logon_name });

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Type", SPO_JSON_CONTENT)
                .json(&body)
        })
        .await?;

        let raw: RawSpoUser = check_json_response(
            resp,
            &format!("Error asegurando usuario '{}' en el sitio", logon_name),
        )
        .await?;

        Ok(SpoUser {
            id: raw.id,
            title: raw.title.unwrap_or_else(|| raw.login_name.clone()),
            email: raw.email.unwrap_or_default(),
            login_name: raw.login_name,
            upn: raw.upn,
            is_site_admin: raw.is_site_admin,
            principal_type: raw.principal_type,
        })
    }

    /// Add a user to a SharePoint group by ensuring the user first, then posting their canonical `LoginName`.
    pub async fn add_user_to_group(
        &self,
        site_url: &str,
        group_id: i64,
        user_identifier: &str,
    ) -> Result<SpoUser> {
        let ensured = self.ensure_user(site_url, user_identifier).await?;
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/sitegroups({})/users", base, group_id);
        let body = json!({ "LoginName": ensured.login_name });

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Type", SPO_JSON_CONTENT)
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error añadiendo '{}' al grupo {} (HTTP {}): {}",
                user_identifier,
                group_id,
                status,
                err_txt
            ));
        }

        Ok(ensured)
    }

    /// Remove a user from a SharePoint group by `user_id` or `login_name`/`upn`.
    pub async fn remove_user_from_group(
        &self,
        site_url: &str,
        group_id: i64,
        user_id: Option<i64>,
        user_identifier: &str,
    ) -> Result<String> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;

        let (resolved_id, canonical_login) = if let Some(uid) = user_id {
            (uid, user_identifier.to_string())
        } else {
            let ensured = self.ensure_user(site_url, user_identifier).await?;
            (ensured.id, ensured.login_name)
        };

        let url = format!(
            "{}/_api/web/sitegroups({})/users/removebyid({})",
            base, group_id, resolved_id
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error removiendo '{}' (Id {}) del grupo {} (HTTP {}): {}",
                user_identifier,
                resolved_id,
                group_id,
                status,
                err_txt
            ));
        }

        Ok(canonical_login)
    }

    /// Create a custom SharePoint group in the site (`POST /_api/web/sitegroups`).
    pub async fn create_group(
        &self,
        site_url: &str,
        title: &str,
        description: &str,
    ) -> Result<SpoGroup> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/sitegroups", base);
        let body = json!({
            "Title": title,
            "Description": description
        });

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Type", SPO_JSON_CONTENT)
                .json(&body)
        })
        .await?;

        let raw: RawSpoGroup =
            check_json_response(resp, &format!("Error creando grupo '{}'", title)).await?;

        Ok(SpoGroup {
            id: raw.id,
            title: raw.title,
            description: raw.description.unwrap_or_default(),
            owner_title: raw.owner_title.unwrap_or_default(),
            kind: GroupKind::Custom,
            permission_levels: Vec::new(),
        })
    }

    /// Delete a SharePoint group by ID (`POST /_api/web/sitegroups/removebyid({id})`).
    pub async fn delete_group(&self, site_url: &str, group_id: i64) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/sitegroups/removebyid({})", base, group_id);

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error eliminando grupo Id {} en '{}': {}",
                group_id,
                site_url,
                err_txt
            ));
        }
        Ok(())
    }

    /// Read site title and `ExcludeFromOfflineClient` flag (`GET /_api/web?$select=Title,ExcludeFromOfflineClient`).
    pub async fn get_web_offline_sync(&self, site_url: &str) -> Result<(String, bool)> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web?$select=Title,ExcludeFromOfflineClient", base);

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: RawWebOfflineSync = check_json_response(
            resp,
            &format!("Error leyendo ExcludeFromOfflineClient de '{}'", base),
        )
        .await?;
        let title = raw
            .title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| base.to_string());
        Ok((title, raw.exclude_from_offline_client))
    }

    /// Enable (`exclude=false`) or disable (`exclude=true`) offline sync / OneDrive shortcuts at the SharePoint site (`SP.Web`) level.
    pub async fn set_web_offline_sync(&self, site_url: &str, exclude: bool) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web", base);
        let payload = serde_json::to_string(&json!({
            "__metadata": { "type": "SP.Web" },
            "ExcludeFromOfflineClient": exclude
        }))?;

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_VERBOSE)
                .header("Content-Type", SPO_JSON_VERBOSE)
                .header("X-HTTP-Method", "MERGE")
                .header("If-Match", "*")
                .body(payload.clone())
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error actualizando ExcludeFromOfflineClient={} en el sitio '{}' (HTTP {}): {}",
                exclude,
                base,
                status,
                err_txt
            ));
        }
        Ok(())
    }

    /// List direct subsites (`/_api/web/webs?$select=Url,Title`) of a SharePoint site.
    pub async fn get_subwebs(&self, site_url: &str) -> Result<Vec<(String, String)>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/webs?$select=Url,Title", base);

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: SpoValueList<RawSubWeb> =
            check_json_response(resp, &format!("Error listando subsitios de '{}'", base)).await?;

        Ok(raw
            .value
            .into_iter()
            .map(|w| {
                let clean_url = w.url.trim_end_matches('/').to_string();
                let title = w.title.unwrap_or_else(|| clean_url.clone());
                (clean_url, title)
            })
            .collect())
    }

    /// Populate OneDrive shortcut button view formatting status (`views_total`, `views_hiding_shortcut`,
    /// `shortcut_button_hidden`) for a list of libraries in parallel.
    pub async fn populate_libraries_shortcut_status(
        &self,
        site_url: &str,
        libs: Vec<crate::models::SpoLibrary>,
    ) -> Vec<crate::models::SpoLibrary> {
        let base = site_url.trim_end_matches('/');
        futures::future::join_all(libs.into_iter().map(|mut l| async move {
            let views = self
                .get_library_views(base, &l.id)
                .await
                .unwrap_or_default();
            let views_total = views.len();
            let views_hiding_shortcut = views
                .iter()
                .filter(|v| Self::view_formatter_hides_shortcut(v.custom_formatter.as_deref()))
                .count();
            let views_hiding_sync = views
                .iter()
                .filter(|v| Self::view_formatter_hides_sync(v.custom_formatter.as_deref()))
                .count();
            l.views_total = views_total;
            l.views_hiding_shortcut = views_hiding_shortcut;
            l.views_hiding_sync = views_hiding_sync;
            l.shortcut_button_hidden = views_total > 0 && views_hiding_shortcut == views_total;
            l
        }))
        .await
    }

    /// Inspect offline sync status (`ExcludeFromOfflineClient`) for a site, its document libraries, and optionally its subsites.
    pub async fn get_site_sync_status(
        &self,
        site_url: &str,
        include_subsites: bool,
    ) -> Result<SiteSyncStatus> {
        let mut visited = HashSet::new();
        self.collect_site_sync_status_recursive(
            site_url.trim_end_matches('/').to_string(),
            include_subsites,
            &mut visited,
        )
        .await
    }

    fn collect_site_sync_status_recursive<'a>(
        &'a self,
        site_url: String,
        include_subsites: bool,
        visited: &'a mut HashSet<String>,
    ) -> Pin<Box<dyn Future<Output = Result<SiteSyncStatus>> + Send + 'a>> {
        Box::pin(async move {
            let base = site_url.trim_end_matches('/').to_string();
            visited.insert(base.to_lowercase());

            let ((site_title, web_exclude), libs) = tokio::try_join!(
                self.get_web_offline_sync(&base),
                self.get_document_libraries(&base)
            )?;

            let populated = self.populate_libraries_shortcut_status(&base, libs).await;
            let libraries = populated
                .into_iter()
                .map(|l| LibrarySyncStatus {
                    id: l.id,
                    title: l.title,
                    server_relative_url: l.server_relative_url,
                    is_default_documents: l.is_default_documents,
                    exclude_from_offline_client: l.exclude_from_offline_client,
                    sync_enabled: !l.exclude_from_offline_client,
                    views_total: l.views_total,
                    views_hiding_shortcut: l.views_hiding_shortcut,
                    views_hiding_sync: l.views_hiding_sync,
                    shortcut_button_hidden: l.shortcut_button_hidden,
                })
                .collect();

            let mut subsites = Vec::new();
            if include_subsites {
                for (sub_url, _) in self.get_subwebs(&base).await.unwrap_or_default() {
                    let clean_sub = sub_url.trim_end_matches('/').to_string();
                    if clean_sub.is_empty() || visited.contains(&clean_sub.to_lowercase()) {
                        continue;
                    }
                    if let Ok(sub_status) = self
                        .collect_site_sync_status_recursive(clean_sub, true, visited)
                        .await
                    {
                        subsites.push(sub_status);
                    }
                }
            }

            Ok(SiteSyncStatus {
                site_url: base,
                site_title,
                exclude_from_offline_client: web_exclude,
                sync_enabled: !web_exclude,
                libraries,
                subsites,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_base_permissions_mask() {
        // Owner with ManagePermissions (1 << 25) and EnumeratePermissions (High 1 << 30)
        let owner_mask = BasePermissionsMask {
            high: serde_json::json!("1073742256"),
            low: serde_json::json!("1044583151"),
        };
        assert!(owner_mask.has_manage_permissions());
        assert!(!owner_mask.is_full_site_admin());

        // Edit / Member role (High = 432 has CreateAlerts + EditMyUserInfo, but NOT ManagePermissions)
        let edit_member_mask = BasePermissionsMask {
            high: serde_json::json!("432"),
            low: serde_json::json!("1011028719"),
        };
        assert!(!edit_member_mask.has_manage_permissions());
        assert!(!edit_member_mask.is_full_site_admin());

        // Site Collection Admin (High = 0x7FFF_FFFF = 2147483647)
        let admin_mask = BasePermissionsMask {
            high: serde_json::json!("2147483647"),
            low: serde_json::json!("4294967295"),
        };
        assert!(admin_mask.has_manage_permissions());
        assert!(admin_mask.is_full_site_admin());

        // Reader only (Low = 138612833, High = 48)
        let reader_mask = BasePermissionsMask {
            high: serde_json::json!("48"),
            low: serde_json::json!("138612833"),
        };
        assert!(!reader_mask.has_manage_permissions());
        assert!(!reader_mask.is_full_site_admin());
    }

    #[test]
    fn test_extract_search_rows() {
        let sample = serde_json::json!({
            "PrimaryQueryResult": {
                "RelevantResults": {
                    "Table": {
                        "Rows": [
                            {
                                "Cells": [
                                    {"Key": "Title", "Value": "Obra Madrid Norte"},
                                    {"Key": "Path", "Value": "https://contoso.sharepoint.com/sites/ObraMadridNorte/"},
                                    {"Key": "Description", "Value": "Sitio de obra"}
                                ]
                            }
                        ]
                    }
                }
            }
        });
        let rows = SpoClient::extract_search_rows(&sample);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "Obra Madrid Norte");
        assert_eq!(
            rows[0].1,
            "https://contoso.sharepoint.com/sites/ObraMadridNorte"
        );
        assert_eq!(rows[0].2.as_deref(), Some("Sitio de obra"));
    }
}
