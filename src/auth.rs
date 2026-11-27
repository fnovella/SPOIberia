use anyhow::{ anyhow, Context, Result };
use base64::{ engine::general_purpose::URL_SAFE_NO_PAD, Engine as _ };
use chrono::{ DateTime, Duration, Utc };
use rand::{ distributions::Alphanumeric, Rng };
use serde::{ Deserialize, Serialize };
use sha2::{ Digest, Sha256 };
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{ AsyncReadExt, AsyncWriteExt };
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

/// Legacy Microsoft Azure CLI Public Client ID (blocked by SharePoint Online when using `user_impersonation`).
pub const LEGACY_AZURE_CLI_CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46";
/// Default Public Client ID (admin-consented in the tenant for Microsoft Graph + SharePoint Online `AllSites.FullControl`).
pub const DEFAULT_CLIENT_ID: &str = "6cdea627-9710-4946-9544-afa7d24fd12f";

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedAccessToken {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

impl fmt::Debug for CachedAccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CachedAccessToken")
            .field("access_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl CachedAccessToken {
    pub fn is_valid(&self) -> bool {
        Utc::now() + Duration::seconds(90) < self.expires_at
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub tenant_id: String,
    pub client_id: String,
    pub user_upn: Option<String>,
    pub user_display_name: Option<String>,
    pub root_site_url: Option<String>,
    pub refresh_token: String,
    pub tokens: HashMap<String, CachedAccessToken>, // resource -> token
}

impl fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthSession")
            .field("tenant_id", &self.tenant_id)
            .field("client_id", &self.client_id)
            .field("user_upn", &self.user_upn)
            .field("user_display_name", &self.user_display_name)
            .field("root_site_url", &self.root_site_url)
            .field("refresh_token", &"[REDACTED]")
            .field("tokens", &self.tokens)
            .finish()
    }
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
    refresh_lock: Arc<Mutex<()>>,
}

impl AuthManager {
    pub fn new() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        fs::create_dir_all(&config_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700));
        }
        let session_path = config_dir.join("session.json");
        let http = reqwest::Client
            ::builder()
            .user_agent(concat!("spoiberia/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            session_path,
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_session_path(session_path: PathBuf) -> Self {
        Self {
            http: reqwest::Client::new(),
            session_path,
            refresh_lock: Arc::new(Mutex::new(())),
        }
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

    /// Validate that a tenant ID or domain contains only safe hostname/GUID characters.
    fn is_valid_tenant_segment(s: &str) -> bool {
        !s.is_empty()
            && !s.starts_with('.')
            && !s.ends_with('.')
            && !s.starts_with('-')
            && !s.ends_with('-')
            && !s.contains("..")
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    }

    /// Parse an email address (`user@domain.com`), domain (`domain.com`), or tenant GUID into `(domain_or_tenant, optional_login_hint)`.
    pub fn parse_tenant_or_email(input: &str) -> Option<(String, Option<String>)> {
        let trimmed = input.trim();
        if trimmed.is_empty()
            || trimmed.contains(char::is_whitespace)
            || trimmed.eq_ignore_ascii_case("common")
            || trimmed.eq_ignore_ascii_case("organizations")
            || trimmed.eq_ignore_ascii_case("consumers")
        {
            return None;
        }
        if let Some((local, domain)) = trimmed.split_once('@') {
            let domain = domain.trim();
            if local.is_empty()
                || domain.is_empty()
                || domain.contains('@')
                || !Self::is_valid_tenant_segment(domain)
            {
                return None;
            }
            return Some((domain.to_ascii_lowercase(), Some(trimmed.to_string())));
        }
        if !Self::is_valid_tenant_segment(trimmed) {
            return None;
        }
        Some((trimmed.to_string(), None))
    }

    /// Extract the Entra ID tenant GUID from an OpenID Connect discovery JSON document.
    pub fn extract_tenant_guid_from_oidc(oidc_json: &serde_json::Value) -> Option<String> {
        for field in ["token_endpoint", "issuer"] {
            let Some(url_str) = oidc_json.get(field).and_then(|v| v.as_str()) else {
                continue;
            };
            let Ok(parsed) = Url::parse(url_str) else {
                continue;
            };
            if let Some(seg) = parsed
                .path_segments()
                .and_then(|mut segs| segs.find(|s| !s.is_empty()))
            {
                let lower = seg.to_ascii_lowercase();
                if !matches!(
                    lower.as_str(),
                    "common"
                        | "organizations"
                        | "consumers"
                        | "oauth2"
                        | "v2.0"
                        | "{tenantid}"
                        | "%7btenantid%7d"
                ) {
                    return Some(seg.to_string());
                }
            }
        }
        None
    }

    /// Detect default client_id from env vars or existing `~/.cli-m365-connection.json` if present.
    pub fn default_client_id() -> String {
        if let Ok(c) = std::env::var("SPOIBERIA_CLIENT_ID") {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        if let Some(home) = dirs::home_dir() {
            let m365_conn = home.join(".cli-m365-connection.json");
            if let Ok(raw) = fs::read_to_string(m365_conn) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(c) = val
                        .get("appId")
                        .and_then(|v| v.as_str())
                        .filter(|s| {
                            !s.is_empty()
                                && !s.eq_ignore_ascii_case(LEGACY_AZURE_CLI_CLIENT_ID)
                        })
                    {
                        return c.to_string();
                    }
                }
            }
        }
        DEFAULT_CLIENT_ID.to_string()
    }

    /// Detect a default corporate email, domain, or tenant ID from env vars, cached session, or CLI for M365 config.
    pub fn detect_default_login_hint(&self) -> Option<String> {
        for var in ["SPOIBERIA_USER_EMAIL", "SPOIBERIA_TENANT_ID"] {
            if let Ok(val) = std::env::var(var) {
                if Self::parse_tenant_or_email(&val).is_some() {
                    return Some(val.trim().to_string());
                }
            }
        }

        if let Ok(Some(sess)) = self.load_session() {
            if let Some(ref upn) = sess.user_upn {
                if Self::parse_tenant_or_email(upn).is_some() {
                    return Some(upn.trim().to_string());
                }
            }
            if Self::parse_tenant_or_email(&sess.tenant_id).is_some() {
                return Some(sess.tenant_id);
            }
        }

        if let Some(home) = dirs::home_dir() {
            let m365_conn = home.join(".cli-m365-connection.json");
            if let Ok(raw) = fs::read_to_string(m365_conn) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(name) = val
                        .get("identityName")
                        .and_then(|v| v.as_str())
                        .filter(|s| s.contains('@') && Self::parse_tenant_or_email(s).is_some())
                    {
                        return Some(name.trim().to_string());
                    }
                    if let Some(t) = val
                        .get("identityTenantId")
                        .and_then(|v| v.as_str())
                        .filter(|s| Self::parse_tenant_or_email(s).is_some())
                    {
                        return Some(t.trim().to_string());
                    }
                }
            }
        }

        None
    }

    /// Resolve the Entra ID tenant GUID and optional `login_hint` from an email address, domain, or tenant ID.
    pub async fn resolve_tenant_id(
        &self,
        tenant_or_email: Option<&str>,
    ) -> Result<(String, Option<String>)> {
        let parsed = tenant_or_email
            .and_then(Self::parse_tenant_or_email)
            .or_else(|| {
                self.detect_default_login_hint()
                    .as_deref()
                    .and_then(Self::parse_tenant_or_email)
            });

        let (domain_or_tenant, login_hint) = parsed.ok_or_else(|| {
            anyhow!(
                "No se conoce el correo corporativo o tenant de Microsoft 365. \
                Introduce tu correo corporativo (ej. usuario@dominio.com) o usa `--email` / `--tenant`."
            )
        })?;

        let oidc_url = format!(
            "https://login.microsoftonline.com/{}/.well-known/openid-configuration",
            domain_or_tenant
        );
        let resp = self
            .http
            .get(&oidc_url)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Error consultando la configuración OpenID de Entra ID para '{}'",
                    domain_or_tenant
                )
            })?;

        if resp.status().is_success() {
            let json_val: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
            let tenant_id = Self::extract_tenant_guid_from_oidc(&json_val)
                .unwrap_or_else(|| domain_or_tenant.clone());
            return Ok((tenant_id, login_hint));
        }

        if resp.status().is_client_error() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "No se encontró un tenant de Microsoft 365 válido para '{}': {}",
                domain_or_tenant,
                body
            ));
        }

        Ok((domain_or_tenant, login_hint))
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
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        let json = serde_json::to_string_pretty(session)?;
        let tmp_name = format!(
            "{}.tmp.{}",
            self.session_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("session.json"),
            std::process::id()
        );
        let tmp_path = self.session_path.with_file_name(tmp_name);

        let mut opts = fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        {
            let mut f = opts.open(&tmp_path)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&tmp_path, &self.session_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.session_path, fs::Permissions::from_mode(0o600))?;
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
        tenant_or_email: Option<&str>,
        client_id: Option<&str>
    ) -> Result<AuthSession> {
        let (tenant_id, login_hint) = self.resolve_tenant_id(tenant_or_email).await?;
        let def_client = Self::default_client_id();
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
            &format!("https://login.microsoftonline.com/{}/oauth2/v2.0/authorize", tenant_id)
        )?;
        {
            let mut pairs = auth_url.query_pairs_mut();
            pairs
                .append_pair("client_id", client_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("response_mode", "query")
                .append_pair("scope", "https://graph.microsoft.com/.default offline_access openid profile")
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256")
                .append_pair("state", &state)
                .append_pair("prompt", "select_account");
            if let Some(ref hint) = login_hint {
                pairs.append_pair("login_hint", hint);
            }
        }

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

        let state_ok = error_msg.is_none()
            && returned_state.as_deref() == Some(state.as_str())
            && code.is_some();

        let html_body = if state_ok {
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

        let token_url = format!("https://login.microsoftonline.com/{}/oauth2/v2.0/token", tenant_id);
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
            tenant_id,
            client_id: client_id.to_string(),
            user_upn: login_hint,
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
        tenant_or_email: Option<&str>,
        client_id: Option<&str>
    ) -> Result<(DeviceCodePrompt, String, String)> {
        let (tenant_id, _login_hint) = self.resolve_tenant_id(tenant_or_email).await?;
        let def_client = Self::default_client_id();
        let client_id = client_id.unwrap_or(&def_client).to_string();

        let url = format!("https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode", tenant_id);
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
        Ok((prompt, tenant_id, client_id))
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

    /// Obtain a valid access token for a given resource base URL (e.g. `https://graph.microsoft.com` or `https://contoso.sharepoint.com`).
    pub async fn get_token_for_resource(&self, resource_origin: &str) -> Result<String> {
        let _guard = self.refresh_lock.lock().await;
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
        if parsed.scheme() != "https" {
            return Err(anyhow!("La URL del sitio SharePoint debe usar HTTPS: {}", site_url));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(anyhow!(
                "La URL del sitio SharePoint no puede contener credenciales embebidas: {}",
                site_url
            ));
        }
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

    #[test]
    fn test_parse_tenant_or_email() {
        assert_eq!(
            AuthManager::parse_tenant_or_email("  Usuario@Contoso.COM "),
            Some(("contoso.com".to_string(), Some("Usuario@Contoso.COM".to_string())))
        );
        assert_eq!(
            AuthManager::parse_tenant_or_email("contoso.onmicrosoft.com"),
            Some(("contoso.onmicrosoft.com".to_string(), None))
        );
        assert_eq!(
            AuthManager::parse_tenant_or_email("11111111-2222-3333-4444-555555555555"),
            Some(("11111111-2222-3333-4444-555555555555".to_string(), None))
        );
        assert_eq!(AuthManager::parse_tenant_or_email(""), None);
        assert_eq!(AuthManager::parse_tenant_or_email("   "), None);
        assert_eq!(AuthManager::parse_tenant_or_email("common"), None);
        assert_eq!(AuthManager::parse_tenant_or_email("organizations"), None);
        assert_eq!(AuthManager::parse_tenant_or_email("@contoso.com"), None);
        assert_eq!(AuthManager::parse_tenant_or_email("user@"), None);
        assert_eq!(AuthManager::parse_tenant_or_email("user name@contoso.com"), None);
    }

    #[test]
    fn test_extract_tenant_guid_from_oidc() {
        let from_token_endpoint = serde_json::json!({
            "token_endpoint": "https://login.microsoftonline.com/11111111-2222-3333-4444-555555555555/oauth2/v2.0/token",
            "issuer": "https://sts.windows.net/11111111-2222-3333-4444-555555555555/"
        });
        assert_eq!(
            AuthManager::extract_tenant_guid_from_oidc(&from_token_endpoint),
            Some("11111111-2222-3333-4444-555555555555".to_string())
        );

        let only_issuer = serde_json::json!({
            "issuer": "https://sts.windows.net/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/"
        });
        assert_eq!(
            AuthManager::extract_tenant_guid_from_oidc(&only_issuer),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string())
        );

        let common_placeholder = serde_json::json!({
            "token_endpoint": "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            "issuer": "https://sts.windows.net/{tenantid}/"
        });
        assert_eq!(AuthManager::extract_tenant_guid_from_oidc(&common_placeholder), None);
    }

    #[test]
    fn test_parse_tenant_or_email_rejects_injection() {
        assert_eq!(
            AuthManager::parse_tenant_or_email("contoso.onmicrosoft.com"),
            Some(("contoso.onmicrosoft.com".to_string(), None))
        );
        assert_eq!(
            AuthManager::parse_tenant_or_email("user@contoso.com"),
            Some(("contoso.com".to_string(), Some("user@contoso.com".to_string())))
        );
        assert_eq!(AuthManager::parse_tenant_or_email("contoso.com/evil?x=1"), None);
        assert_eq!(AuthManager::parse_tenant_or_email("contoso.com#frag"), None);
        assert_eq!(AuthManager::parse_tenant_or_email(".."), None);
        assert_eq!(AuthManager::parse_tenant_or_email(""), None);
    }

    #[test]
    fn test_session_debug_redacts_tokens() {
        let session = AuthSession {
            tenant_id: "contoso.com".into(),
            client_id: DEFAULT_CLIENT_ID.into(),
            user_upn: Some("alice@contoso.com".into()),
            user_display_name: Some("Alice".into()),
            root_site_url: None,
            refresh_token: "super-secret-refresh-token".into(),
            tokens: HashMap::from([(
                "https://graph.microsoft.com/.default".into(),
                CachedAccessToken {
                    access_token: "super-secret-access-token".into(),
                    expires_at: Utc::now() + Duration::hours(1),
                },
            )]),
        };
        let dbg = format!("{:?}", session);
        assert!(!dbg.contains("super-secret-refresh-token"));
        assert!(!dbg.contains("super-secret-access-token"));
        assert!(dbg.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn test_get_spo_token_rejects_non_https_and_userinfo() {
        let dir = std::env::temp_dir().join(format!("spo_auth_test_{}", std::process::id()));
        let auth = AuthManager::with_session_path(dir.join("session.json"));
        assert!(auth.get_spo_token_for_url("http://contoso.sharepoint.com").await.is_err());
        assert!(auth
            .get_spo_token_for_url("https://user:pass@contoso.sharepoint.com")
            .await
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_save_session_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("spo_auth_perm_{}", std::process::id()));
        let session_path = dir.join("session.json");
        let auth = AuthManager::with_session_path(session_path.clone());
        let session = AuthSession {
            tenant_id: "contoso.com".into(),
            client_id: DEFAULT_CLIENT_ID.into(),
            user_upn: Some("alice@contoso.com".into()),
            user_display_name: Some("Alice".into()),
            root_site_url: None,
            refresh_token: "rt".into(),
            tokens: HashMap::new(),
        };
        auth.save_session(&session).unwrap();
        let mode = std::fs::metadata(&session_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
