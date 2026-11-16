use anyhow::{ anyhow, Context, Result };
use base64::{ engine::general_purpose::URL_SAFE_NO_PAD, Engine as _ };
use chrono::{ DateTime, Duration, Utc };
use rand::{ distributions::Alphanumeric, Rng };
use serde::{ Deserialize, Serialize };
use sha2::{ Digest, Sha256 };
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tokio::io::{ AsyncReadExt, AsyncWriteExt };
use tokio::net::TcpListener;
use url::Url;

/// Legacy Microsoft Azure CLI Public Client ID (blocked by SharePoint Online when using `user_impersonation`).
pub const LEGACY_AZURE_CLI_CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46";
/// Default Public Client ID (admin-consented in the tenant for Microsoft Graph + SharePoint Online `AllSites.FullControl`).
pub const DEFAULT_CLIENT_ID: &str = "6cdea627-9710-4946-9544-afa7d24fd12f";
pub const DEFAULT_TENANT: &str = "f5fd9e16-114f-4820-8de1-6312daf9aebb";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAccessToken {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

impl CachedAccessToken {
    pub fn is_valid(&self) -> bool {
        Utc::now() + Duration::seconds(90) < self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub tenant_id: String,
    pub client_id: String,
    pub user_upn: Option<String>,
    pub user_display_name: Option<String>,
    pub root_site_url: Option<String>,
    pub refresh_token: String,
    pub tokens: HashMap<String, CachedAccessToken>, // resource -> token
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodePrompt {
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenEndpointResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
    error_description: Option<String>,
}

#[derive(Clone)]
pub struct AuthManager {
    http: reqwest::Client,
    session_path: PathBuf,
}

impl AuthManager {
    pub fn new() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        fs::create_dir_all(&config_dir)?;
        let session_path = config_dir.join("session.json");
        let http = reqwest::Client
            ::builder()
            .user_agent(concat!("spoiberia/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, session_path })
    }

    pub fn config_dir() -> Result<PathBuf> {
        if let Ok(custom) = std::env::var("SPOIBERIA_CONFIG_DIR") {
            return Ok(PathBuf::from(custom));
        }
        let base = dirs
            ::config_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("No se pudo determinar el directorio de configuración"))?;
        Ok(base.join("spoiberia"))
    }

    pub fn data_dir() -> Result<PathBuf> {
        if let Ok(custom) = std::env::var("SPOIBERIA_DATA_DIR") {
            return Ok(PathBuf::from(custom));
        }
        let base = dirs
            ::data_local_dir()
            .or_else(dirs::config_dir)
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("No se pudo determinar el directorio de datos locales"))?;
        Ok(base.join("spoiberia"))
    }

    /// Detect default tenant and client_id from env vars or existing `~/.cli-m365-connection.json` if present.
    pub fn default_tenant_and_client() -> (String, String) {
        let mut tenant = std::env
            ::var("SPOIBERIA_TENANT_ID")
            .unwrap_or_else(|_| DEFAULT_TENANT.to_string());
        let mut client_id = std::env
            ::var("SPOIBERIA_CLIENT_ID")
            .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());

        if let Some(home) = dirs::home_dir() {
            let m365_conn = home.join(".cli-m365-connection.json");
            if let Ok(raw) = fs::read_to_string(m365_conn) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if tenant == DEFAULT_TENANT {
                        if
                            let Some(t) = val
                                .get("identityTenantId")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("common"))
                        {
                            tenant = t.to_string();
                        }
                    }
                    if client_id == DEFAULT_CLIENT_ID {
                        if
                            let Some(c) = val
                                .get("appId")
                                .and_then(|v| v.as_str())
                                .filter(|s| {
                                    !s.is_empty()
                                        && !s.eq_ignore_ascii_case(LEGACY_AZURE_CLI_CLIENT_ID)
                                })
                        {
                            client_id = c.to_string();
                        }
                    }
                }
            }
        }
        (tenant, client_id)
    }

    pub fn load_session(&self) -> Result<Option<AuthSession>> {
        if !self.session_path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.session_path)?;
        let session: AuthSession = serde_json
            ::from_str(&raw)
            .context("Formato inválido en session.json")?;
        if session.client_id.eq_ignore_ascii_case(LEGACY_AZURE_CLI_CLIENT_ID) {
            return Ok(None);
        }
        Ok(Some(session))
    }

    pub fn save_session(&self, session: &AuthSession) -> Result<()> {
        if let Some(parent) = self.session_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(session)?;
        fs::write(&self.session_path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&self.session_path)?.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&self.session_path, perms);
        }
        Ok(())
    }

    pub fn logout(&self) -> Result<()> {
        if self.session_path.exists() {
            fs::remove_file(&self.session_path)?;
        }
        Ok(())
    }

    /// Generate PKCE verifier and S256 challenge.
    pub fn generate_pkce() -> (String, String) {
        let verifier: String = rand
            ::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(digest);
        (verifier, challenge)
    }

    /// Interactive Browser OAuth2 login with PKCE over localhost loopback.
    pub async fn login_interactive_browser(
        &self,
        tenant: Option<&str>,
        client_id: Option<&str>
    ) -> Result<AuthSession> {
        let (def_tenant, def_client) = Self::default_tenant_and_client();
        let tenant = tenant.unwrap_or(&def_tenant);
        let client_id = client_id.unwrap_or(&def_client);

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://localhost:{}", port);

        let (verifier, challenge) = Self::generate_pkce();
        let state: String = rand
            ::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(24)
            .map(char::from)
            .collect();

        let mut auth_url = Url::parse(
            &format!("https://login.microsoftonline.com/{}/oauth2/v2.0/authorize", tenant)
        )?;
        auth_url
            .query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_mode", "query")
            .append_pair("scope", "https://graph.microsoft.com/.default offline_access openid profile")
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("prompt", "select_account");

        let _ = open::that(auth_url.as_str());

        let (mut stream, _) = tokio::time
            ::timeout(std::time::Duration::from_secs(180), listener.accept()).await
            .context("Tiempo de espera agotado esperando la autenticación en el navegador")??;

        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let request_str = String::from_utf8_lossy(&buf[..n]);

        let first_line = request_str.lines().next().unwrap_or("");
        let path = first_line.split_whitespace().nth(1).unwrap_or("/");
        let full_redirect = format!("http://localhost:{}{}", port, path);
        let parsed = Url::parse(&full_redirect)?;

        let mut code: Option<String> = None;
        let mut returned_state: Option<String> = None;
        let mut error_msg: Option<String> = None;

        for (k, v) in parsed.query_pairs() {
            match k.as_ref() {
                "code" => {
                    code = Some(v.to_string());
                }
                "state" => {
                    returned_state = Some(v.to_string());
                }
                "error_description" | "error" => {
                    error_msg = Some(v.to_string());
                }
                _ => {}
            }
        }

        let html_body = if code.is_some() {
            "<html><head><meta charset='utf-8'><title>SPOIberia - Autenticación Completada</title></head>\
            <body style='font-family:sans-serif;background:#0f172a;color:#f8fafc;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;'>\
            <div style='background:#1e293b;padding:32px;border-radius:12px;border:1px solid #334155;text-align:center;'>\
            <h2 style='color:#4ade80;margin-top:0;'>✓ Autenticación completada en SPOIberia</h2>\
            <p>Ya puedes cerrar esta pestaña y volver a la terminal.</p></div></body></html>"
        } else {
            "<html><head><meta charset='utf-8'><title>SPOIberia - Error</title></head>\
            <body style='font-family:sans-serif;background:#0f172a;color:#f8fafc;padding:40px;'>\
            <h2 style='color:#f87171;'>✗ Error en la autenticación</h2><p>Vuelve a la terminal para más detalles.</p></body></html>"
        };

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html_body.len(),
            html_body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.flush().await;

        if let Some(err) = error_msg {
            return Err(anyhow!("Error devuelto por Entra ID: {}", err));
        }

        if returned_state.as_deref() != Some(state.as_str()) {
            return Err(anyhow!("El parámetro OAuth2 'state' no coincide"));
        }

        let auth_code = code.ok_or_else(||
            anyhow!("No se recibió código de autorización OAuth2")
        )?;

        let token_url = format!("https://login.microsoftonline.com/{}/oauth2/v2.0/token", tenant);
        let params = [
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", auth_code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
            ("scope", "https://graph.microsoft.com/.default offline_access openid profile"),
        ];

        let resp = self.http.post(&token_url).form(&params).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Error canjeando el código OAuth2: {}", body));
        }

        let tok: TokenEndpointResponse = resp.json().await?;
        let refresh_token = tok.refresh_token.ok_or_else(|| {
            anyhow!("Entra ID no devolvió refresh_token (asegúrate de permitir offline_access)")
        })?;

        let mut tokens = HashMap::new();
        tokens.insert("https://graph.microsoft.com".to_string(), CachedAccessToken {
            access_token: tok.access_token,
            expires_at: Utc::now() + Duration::seconds(tok.expires_in),
        });

        let session = AuthSession {
            tenant_id: tenant.to_string(),
            client_id: client_id.to_string(),
            user_upn: None,
            user_display_name: None,
            root_site_url: None,
            refresh_token,
            tokens,
        };
        self.save_session(&session)?;
        Ok(session)
    }

    /// Request a Device Code from Microsoft Entra ID.
    pub async fn start_device_code(
        &self,
        tenant: Option<&str>,
        client_id: Option<&str>
    ) -> Result<(DeviceCodePrompt, String, String)> {
        let (def_tenant, def_client) = Self::default_tenant_and_client();
        let tenant = tenant.unwrap_or(&def_tenant).to_string();
        let client_id = client_id.unwrap_or(&def_client).to_string();

        let url = format!("https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode", tenant);
        let params = [
            ("client_id", client_id.as_str()),
            ("scope", "https://graph.microsoft.com/.default offline_access openid profile"),
        ];

        let resp = self.http.post(&url).form(&params).send().await?;
        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Error solicitando Device Code: {}", err_txt));
        }
        let prompt: DeviceCodePrompt = resp.json().await?;
        Ok((prompt, tenant, client_id))
    }

    /// Poll the token endpoint until the user completes Device Code authentication.
    pub async fn poll_device_code(
        &self,
        prompt: &DeviceCodePrompt,
        tenant: &str,
        client_id: &str
    ) -> Result<AuthSession> {
        let token_url = format!("https://login.microsoftonline.com/{}/oauth2/v2.0/token", tenant);
        let interval = std::time::Duration::from_secs(prompt.interval.max(3));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(prompt.expires_in);

        while std::time::Instant::now() < deadline {
            tokio::time::sleep(interval).await;
            let params = [
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", prompt.device_code.as_str()),
            ];

            let resp = self.http.post(&token_url).form(&params).send().await?;
            if resp.status().is_success() {
                let tok: TokenEndpointResponse = resp.json().await?;
                let refresh_token = tok.refresh_token.ok_or_else(|| {
                    anyhow!("No se recibió refresh_token tras Device Code")
                })?;
                let mut tokens = HashMap::new();
                tokens.insert("https://graph.microsoft.com".to_string(), CachedAccessToken {
                    access_token: tok.access_token,
                    expires_at: Utc::now() + Duration::seconds(tok.expires_in),
                });
                let session = AuthSession {
                    tenant_id: tenant.to_string(),
                    client_id: client_id.to_string(),
                    user_upn: None,
                    user_display_name: None,
                    root_site_url: None,
                    refresh_token,
                    tokens,
                };
                self.save_session(&session)?;
                return Ok(session);
            } else {
                let err: TokenErrorResponse = resp.json().await.unwrap_or(TokenErrorResponse {
                    error: "unknown".to_string(),
                    error_description: None,
                });
                match err.error.as_str() {
                    "authorization_pending" | "slow_down" => {
                        continue;
                    }
                    _ => {
                        return Err(
                            anyhow!(
                                "Error en Device Code Login: {} ({:?})",
                                err.error,
                                err.error_description
                            )
                        );
                    }
                }
            }
        }

        Err(anyhow!("El código de dispositivo ha expirado"))
    }

    /// Obtain a valid access token for a given resource base URL (e.g. `https://graph.microsoft.com` or `https://acciona365.sharepoint.com`).
    pub async fn get_token_for_resource(&self, resource_origin: &str) -> Result<String> {
        let normalized = resource_origin.trim_end_matches('/').to_string();
        let mut session = self
            .load_session()?
            .ok_or_else(|| {
                anyhow!(
                    "No hay sesión activa en SPOIberia. Ejecuta `spoiberia auth login` o inicia sesión en la TUI."
                )
            })?;

        if let Some(cached) = session.tokens.get(&normalized) {
            if cached.is_valid() {
                return Ok(cached.access_token.clone());
            }
        }

        let scope = format!("{}/.default offline_access", normalized);
        let token_url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            session.tenant_id
        );
        let params = [
            ("client_id", session.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", session.refresh_token.as_str()),
            ("scope", scope.as_str()),
        ];

        let resp = self.http.post(&token_url).form(&params).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(
                anyhow!(
                    "Error al renovar token para '{}': {}. Vuelve a iniciar sesión con `spoiberia auth login`.",
                    normalized,
                    body
                )
            );
        }

        let tok: TokenEndpointResponse = resp.json().await?;
        if let Some(new_rt) = tok.refresh_token {
            session.refresh_token = new_rt;
        }
        let access_token = tok.access_token.clone();
        session.tokens.insert(normalized, CachedAccessToken {
            access_token: tok.access_token,
            expires_at: Utc::now() + Duration::seconds(tok.expires_in),
        });
        self.save_session(&session)?;
        Ok(access_token)
    }

    /// Decode JWT claims and return `true` if this is an Azure CLI token with only `user_impersonation`,
    /// which SharePoint Online rejects with `x-ms-diagnostics: 3001003`.
    pub fn is_blocked_spo_token(access_token: &str) -> bool {
        let Some(payload_b64) = access_token.split('.').nth(1) else {
            return false;
        };
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload_b64.trim_end_matches('=')) else {
            return false;
        };
        let Ok(claims) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return false;
        };
        let appid = claims.get("appid").and_then(|v| v.as_str()).unwrap_or("");
        let scp = claims.get("scp").and_then(|v| v.as_str()).unwrap_or("");
        appid.eq_ignore_ascii_case(LEGACY_AZURE_CLI_CLIENT_ID) && scp == "user_impersonation"
    }

    /// Extract origin (`https://host`) from any SharePoint site URL and fetch its access token.
    pub async fn get_spo_token_for_url(&self, site_url: &str) -> Result<String> {
        let parsed = Url::parse(site_url).with_context(||
            format!("URL de sitio inválida: {}", site_url)
        )?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("URL de sitio sin host: {}", site_url))?;
        let origin = format!("{}://{}", parsed.scheme(), host);
        self.get_token_for_resource(&origin).await
    }

    pub fn update_user_metadata(
        &self,
        upn: Option<String>,
        display_name: Option<String>,
        root_site_url: Option<String>
    ) -> Result<()> {
        if let Some(mut session) = self.load_session()? {
            if upn.is_some() {
                session.user_upn = upn;
            }
            if display_name.is_some() {
                session.user_display_name = display_name;
            }
            if root_site_url.is_some() {
                session.root_site_url = root_site_url;
            }
            self.save_session(&session)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let (verifier, challenge) = AuthManager::generate_pkce();
        assert_eq!(verifier.len(), 64);
        assert!(!challenge.is_empty());
        assert!(!challenge.contains('='));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
    }

    #[test]
    fn test_cached_token_validity() {
        let valid = CachedAccessToken {
            access_token: "tok".to_string(),
            expires_at: Utc::now() + Duration::minutes(10),
        };
        assert!(valid.is_valid());

        let expiring_soon = CachedAccessToken {
            access_token: "tok".to_string(),
            expires_at: Utc::now() + Duration::seconds(30),
        };
        assert!(!expiring_soon.is_valid());
    }

    #[test]
    fn test_is_blocked_spo_token() {
        let make_jwt = |claims: serde_json::Value| {
            let hdr = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\"}");
            let pay = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
            format!("{}.{}.sig", hdr, pay)
        };

        let blocked = make_jwt(serde_json::json!({
            "appid": LEGACY_AZURE_CLI_CLIENT_ID,
            "scp": "user_impersonation"
        }));
        assert!(AuthManager::is_blocked_spo_token(&blocked));

        let spo_app = make_jwt(serde_json::json!({
            "appid": DEFAULT_CLIENT_ID,
            "scp": "AllSites.FullControl"
        }));
        assert!(!AuthManager::is_blocked_spo_token(&spo_app));

        let other_scope = make_jwt(serde_json::json!({
            "appid": LEGACY_AZURE_CLI_CLIENT_ID,
            "scp": "AllSites.Manage"
        }));
        assert!(!AuthManager::is_blocked_spo_token(&other_scope));
        assert!(!AuthManager::is_blocked_spo_token("not-a-jwt"));
    }
}
