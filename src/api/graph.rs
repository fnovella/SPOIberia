use crate::api::{check_json_response, send_with_retry};
use crate::auth::AuthManager;
use crate::models::{DirectoryUser, SiteRole, SpoSite};
use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde::Deserialize;

#[derive(Clone)]
pub struct GraphClient {
    http: reqwest::Client,
    auth: AuthManager,
}

#[derive(Debug, Deserialize)]
pub struct MeProfile {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "userPrincipalName")]
    pub user_principal_name: Option<String>,
    pub mail: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GraphSiteRoot {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "webUrl")]
    pub web_url: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphListResponse<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OwnedGroupItem {
    id: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "groupTypes", default)]
    group_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GraphUserItem {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    user_principal_name: Option<String>,
    #[serde(rename = "jobTitle")]
    job_title: Option<String>,
    department: Option<String>,
}

impl GraphClient {
    pub fn new(auth: AuthManager) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("spoiberia/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, auth })
    }

    async fn graph_token(&self) -> Result<String> {
        self.auth
            .get_token_for_resource("https://graph.microsoft.com")
            .await
    }

    pub async fn get_me_and_root_site(&self) -> Result<(MeProfile, GraphSiteRoot)> {
        let token = self.graph_token().await?;

        let me_resp = send_with_retry(|| {
            self.http
                .get("https://graph.microsoft.com/v1.0/me?$select=id,displayName,userPrincipalName,mail")
                .bearer_auth(&token)
        })
        .await?;
        let me: MeProfile = check_json_response(me_resp, "Error obteniendo /v1.0/me").await?;

        let root_resp = send_with_retry(|| {
            self.http
                .get("https://graph.microsoft.com/v1.0/sites/root?$select=id,displayName,webUrl,description")
                .bearer_auth(&token)
        })
        .await?;
        let root: GraphSiteRoot =
            check_json_response(root_resp, "Error obteniendo /v1.0/sites/root").await?;

        let upn = me.user_principal_name.clone().or_else(|| me.mail.clone());
        let _ = self.auth.update_user_metadata(
            upn,
            me.display_name.clone(),
            Some(root.web_url.clone()),
        );

        Ok((me, root))
    }

    async fn fetch_paged_groups(
        &self,
        token: &str,
        initial_url: &str,
    ) -> Result<Vec<OwnedGroupItem>> {
        let mut next_url = Some(initial_url.to_string());
        let mut items = Vec::new();
        let mut pages = 0usize;

        while let Some(url) = next_url.take() {
            if pages >= 10 {
                break;
            }
            pages += 1;
            let resp = send_with_retry(|| self.http.get(&url).bearer_auth(token)).await?;
            if !resp.status().is_success() {
                break;
            }
            let page: GraphListResponse<OwnedGroupItem> = match resp.json().await {
                Ok(p) => p,
                Err(_) => break,
            };
            items.extend(page.value);
            next_url = page.next_link;
        }

        Ok(items)
    }

    async fn resolve_group_sites(
        &self,
        token: &str,
        groups: Vec<OwnedGroupItem>,
        role: SiteRole,
    ) -> Vec<SpoSite> {
        let client = self.http.clone();
        let tok = token.to_string();

        stream::iter(groups)
            .map(|grp| {
                let c = client.clone();
                let t = tok.clone();
                let r_role = role.clone();
                async move {
                    let url = format!(
                        "https://graph.microsoft.com/v1.0/groups/{}/sites/root?$select=id,displayName,webUrl,description",
                        grp.id
                    );
                    let r = send_with_retry(|| c.get(&url).bearer_auth(&t)).await.ok()?;
                    if !r.status().is_success() {
                        return None;
                    }
                    let site: GraphSiteRoot = r.json().await.ok()?;
                    Some(SpoSite {
                        title: site
                            .display_name
                            .or(grp.display_name)
                            .unwrap_or_else(|| site.web_url.clone()),
                        url: site.web_url.trim_end_matches('/').to_string(),
                        description: site.description,
                        role: r_role,
                        is_site_admin: false,
                    })
                }
            })
            .buffer_unordered(10)
            .filter_map(|x| async move { x })
            .collect()
            .await
    }

    /// Retrieve all SharePoint team sites corresponding to M365 Unified Groups owned by the user.
    pub async fn get_owned_m365_group_sites(&self) -> Result<Vec<SpoSite>> {
        let token = self.graph_token().await?;
        let all_groups = self
            .fetch_paged_groups(
                &token,
                "https://graph.microsoft.com/v1.0/me/ownedObjects/microsoft.graph.group?$select=id,displayName,groupTypes&$top=200",
            )
            .await?;

        let unified_groups: Vec<OwnedGroupItem> = all_groups
            .into_iter()
            .filter(|g| g.group_types.iter().any(|t| t == "Unified") || g.group_types.is_empty())
            .collect();

        Ok(self
            .resolve_group_sites(&token, unified_groups, SiteRole::M365GroupOwner)
            .await)
    }

    /// Retrieve SharePoint sites corresponding to M365 Unified Groups where the user is a transitive member.
    pub async fn get_member_m365_group_sites(&self) -> Result<Vec<SpoSite>> {
        let token = self.graph_token().await?;
        let all_groups = self
            .fetch_paged_groups(
                &token,
                "https://graph.microsoft.com/v1.0/me/transitiveMemberOf/microsoft.graph.group?$select=id,displayName,groupTypes&$top=200",
            )
            .await?;

        let unified_groups: Vec<OwnedGroupItem> = all_groups
            .into_iter()
            .filter(|g| g.group_types.iter().any(|t| t == "Unified"))
            .collect();

        Ok(self
            .resolve_group_sites(&token, unified_groups, SiteRole::Owner)
            .await)
    }

    /// Retrieve SharePoint sites followed by the current user via Microsoft Graph (`/me/followedSites`).
    pub async fn get_followed_sites(&self) -> Result<Vec<SpoSite>> {
        let token = self.graph_token().await?;
        let mut next_url = Some(
            "https://graph.microsoft.com/v1.0/me/followedSites?$select=id,displayName,webUrl,description&$top=200"
                .to_string(),
        );
        let mut sites = Vec::new();
        let mut pages = 0usize;

        while let Some(url) = next_url.take() {
            if pages >= 10 {
                break;
            }
            pages += 1;
            let resp = send_with_retry(|| self.http.get(&url).bearer_auth(&token)).await?;
            if !resp.status().is_success() {
                break;
            }
            let page: GraphListResponse<GraphSiteRoot> = match resp.json().await {
                Ok(p) => p,
                Err(_) => break,
            };
            for s in page.value {
                if s.web_url.is_empty() {
                    continue;
                }
                sites.push(SpoSite {
                    title: s.display_name.unwrap_or_else(|| s.web_url.clone()),
                    url: s.web_url.trim_end_matches('/').to_string(),
                    description: s.description,
                    role: SiteRole::Owner,
                    is_site_admin: false,
                });
            }
            next_url = page.next_link;
        }

        Ok(sites)
    }

    /// Check whether every whitespace-separated token in `query` appears in the user's
    /// display name, email, or UPN (case-insensitive).
    pub fn matches_all_query_tokens(user: &DirectoryUser, query: &str) -> bool {
        let tokens: Vec<String> = query
            .trim()
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return false;
        }
        let disp = user.display_name.to_lowercase();
        let mail = user.email.to_lowercase();
        let upn = user.upn.to_lowercase();
        tokens
            .iter()
            .all(|t| disp.contains(t) || mail.contains(t) || upn.contains(t))
    }

    async fn query_users_search(
        &self,
        token: &str,
        search_clause: &str,
    ) -> Result<Option<Vec<DirectoryUser>>> {
        let url = format!(
            "https://graph.microsoft.com/v1.0/users?$search={}&$select=displayName,mail,userPrincipalName,jobTitle,department&$top=50",
            urlencoding::encode(search_clause)
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(token)
                .header("ConsistencyLevel", "eventual")
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let list: GraphListResponse<GraphUserItem> = resp.json().await?;
        Ok(Some(Self::map_users(list.value)))
    }

    /// Search users in the tenant directory via Microsoft Graph (supporting multi-word queries and emails).
    pub async fn search_directory_users(&self, query: &str) -> Result<Vec<DirectoryUser>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let token = self.graph_token().await?;
        let clean_q = q.replace(['"', '\''], "");
        let raw_tokens: Vec<&str> = clean_q.split_whitespace().collect();
        if raw_tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut search_tokens: Vec<&str> = raw_tokens
            .iter()
            .copied()
            .filter(|t| t.chars().count() >= 2)
            .collect();
        if search_tokens.is_empty() {
            search_tokens = raw_tokens.clone();
        }

        let single_clause = |t: &str| -> String {
            format!(
                "\"displayName:{}\" OR \"mail:{}\" OR \"userPrincipalName:{}\"",
                t, t, t
            )
        };

        let search_clause = if search_tokens.len() == 1 {
            single_clause(search_tokens[0])
        } else {
            search_tokens
                .iter()
                .map(|t| format!("({})", single_clause(t)))
                .collect::<Vec<_>>()
                .join(" AND ")
        };

        if let Some(users) = self.query_users_search(&token, &search_clause).await? {
            let filtered: Vec<DirectoryUser> = users
                .into_iter()
                .filter(|u| Self::matches_all_query_tokens(u, q))
                .collect();
            if !filtered.is_empty() || raw_tokens.len() == 1 {
                return Ok(filtered);
            }
        }

        if raw_tokens.len() > 1 {
            if let Some(longest) = search_tokens
                .iter()
                .copied()
                .max_by_key(|t| t.chars().count())
            {
                let fallback_clause = single_clause(longest);
                if let Some(users) = self.query_users_search(&token, &fallback_clause).await? {
                    let filtered: Vec<DirectoryUser> = users
                        .into_iter()
                        .filter(|u| Self::matches_all_query_tokens(u, q))
                        .collect();
                    if !filtered.is_empty() {
                        return Ok(filtered);
                    }
                }
            }
        }

        // Fallback to $filter=startsWith on first token
        let esc = raw_tokens[0].replace('\'', "''");
        let fallback_filter = format!(
            "startsWith(displayName,'{}') or startsWith(mail,'{}') or startsWith(userPrincipalName,'{}')",
            esc, esc, esc
        );
        let fb_url = format!(
            "https://graph.microsoft.com/v1.0/users?$filter={}&$select=displayName,mail,userPrincipalName,jobTitle,department&$top=50",
            urlencoding::encode(&fallback_filter)
        );
        let fb_resp = send_with_retry(|| self.http.get(&fb_url).bearer_auth(&token)).await?;
        if !fb_resp.status().is_success() {
            return Ok(Vec::new());
        }
        let list: GraphListResponse<GraphUserItem> = fb_resp.json().await?;
        Ok(Self::map_users(list.value)
            .into_iter()
            .filter(|u| Self::matches_all_query_tokens(u, q))
            .collect())
    }

    fn map_users(items: Vec<GraphUserItem>) -> Vec<DirectoryUser> {
        items
            .into_iter()
            .filter_map(|u| {
                let upn = u.user_principal_name.unwrap_or_default();
                let email = u.mail.unwrap_or_else(|| upn.clone());
                if upn.is_empty() && email.is_empty() {
                    return None;
                }
                let login = if !upn.is_empty() {
                    upn.clone()
                } else {
                    email.clone()
                };
                Some(DirectoryUser {
                    display_name: u.display_name.unwrap_or_else(|| login.clone()),
                    email,
                    upn,
                    login_name: login,
                    job_title: u.job_title,
                    department: u.department,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_all_query_tokens_multi_word_and_email() {
        let user = DirectoryUser {
            display_name: "Novella Benavent, Francisco Javier".to_string(),
            email: "franciscojavier.novella.benavent@contoso.com".to_string(),
            upn: "franciscojavier.novella.benavent@contoso.com".to_string(),
            login_name: "franciscojavier.novella.benavent@contoso.com".to_string(),
            job_title: None,
            department: None,
        };

        assert!(GraphClient::matches_all_query_tokens(&user, "novella benavent"));
        assert!(GraphClient::matches_all_query_tokens(&user, "francisco novella"));
        assert!(GraphClient::matches_all_query_tokens(&user, "novella ben"));
        assert!(GraphClient::matches_all_query_tokens(
            &user,
            "novella.benavent@contoso.com"
        ));
        assert!(!GraphClient::matches_all_query_tokens(&user, "novella garcia"));
    }
}
