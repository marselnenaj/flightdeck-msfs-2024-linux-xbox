//! Only the authenticated loopback service can supply state or accept actions.
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

pub const RESOURCES: &[&str] = &[
    "status",
    "setup",
    "game-update",
    "launcher-update",
    "cloud-saves",
    "fenix",
    "gsx",
    "proton",
    "maintenance",
    "store-check",
    "mods",
    "diagnostics",
    "problem-reports",
];

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    origin: String,
    token: String,
}
// Never expose the CSRF token in an event trace or error.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub path: &'static str,
    pub body: Value,
    pub runtime: String,
    pub confirmation: Option<&'static str>,
}
pub type Snapshot = BTreeMap<&'static str, Value>;
impl Client {
    /// The caller must verify this private service record before constructing
    /// the GUI. Every status response must keep the same session identity.
    pub fn new(port: u16, token: String) -> Result<Self, String> {
        if port == 0
            || !(32..=128).contains(&token.len())
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err("Invalid local service identity".into());
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .build()
                .map_err(|e| e.to_string())?,
            origin: format!("http://127.0.0.1:{port}"),
            token,
        })
    }
    async fn request(
        &self,
        path: &str,
        language: &str,
        body: Option<&Value>,
        runtime: Option<&str>,
    ) -> Result<Value, String> {
        let url = format!("{}/api/{path}", self.origin);
        let mut request = if let Some(body) = body {
            self.http
                .post(url)
                .header("Origin", &self.origin)
                .header("X-Flightdeck-Token", &self.token)
                .json(body)
        } else {
            self.http.get(url)
        };
        if let Some(runtime) = runtime {
            // JSON encoding permits non-ASCII filesystem paths in a header.
            request = request.header("X-Flightdeck-Context", context_header(runtime)?);
        }
        let mut response = request.header("Accept-Language", language).timeout(Duration::from_secs(if path.ends_with("/pick") { 135 } else { 35 })).send().await.map_err(|_| if language == "en" {"The local service is not reachable. Reconnecting automatically."} else {"Der lokale Dienst ist nicht erreichbar. Die Verbindung wird automatisch erneut versucht."}.to_string())?;
        let success = response.status().is_success();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Incomplete local response".to_string())?
        {
            if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                return Err("Local response exceeds the size limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let mut result: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid local response".to_string())?;
        if !success || result["ok"] == false {
            return Err(result["error"]
                .as_str()
                .or(result["message"].as_str())
                .unwrap_or("Local operation failed")
                .chars()
                .take(3000)
                .collect());
        }
        if path == "status"
            && (result["app"]["name"] != "Flightdeck"
                || result["csrf_token"] != self.token
                || !result["runtime"].is_object()
                || !result["game"].is_object())
        {
            return Err("Local service identity changed. Reopen Flightdeck.".into());
        }
        if path == "status"
            && let Some(object) = result.as_object_mut()
        {
            object.remove("csrf_token");
        }
        Ok(result)
    }
    pub async fn snapshot(
        &self,
        language: &'static str,
        details: bool,
    ) -> Result<Snapshot, String> {
        self.poll(
            language,
            RESOURCES[1..]
                .iter()
                .copied()
                .filter(|key| details || !["mods", "diagnostics", "problem-reports"].contains(key))
                .collect(),
        )
        .await
    }
    pub async fn poll(
        &self,
        language: &'static str,
        keys: Vec<&'static str>,
    ) -> Result<Snapshot, String> {
        let status = self.request("status", language, None, None).await?;
        let context = status["runtime"]["path"].as_str().unwrap_or("").to_string();
        let mut result = Snapshot::from([("status", status)]);
        let mut tasks = tokio::task::JoinSet::new();
        for key in keys {
            if !RESOURCES.contains(&key) {
                return Err("Unknown local resource".into());
            }
            let client = self.clone();
            tasks.spawn(async move { (key, client.request(key, language, None, None).await) });
        }
        while let Some(value) = tasks.join_next().await {
            let (key, value) = value.map_err(|_| "Local request interrupted".to_string())?;
            match value {
                Ok(value) => {
                    result.insert(key, value);
                }
                Err(error) => {
                    result.insert(key, serde_json::json!({"_error":error}));
                }
            }
        }
        // A second window may have selected an edition while requests ran.
        let final_status = self.request("status", language, None, None).await?;
        if final_status["runtime"]["path"].as_str().unwrap_or("") != context {
            return Err(if language == "en" {
                "Installation changed; refreshing status."
            } else {
                "Die Installation wurde gewechselt; der Status wird neu geladen."
            }
            .into());
        }
        result.insert("status", final_status);
        Ok(result)
    }
    pub async fn post(&self, request: &Request, language: &str) -> Result<Value, String> {
        self.request(
            request.path,
            language,
            Some(&request.body),
            Some(&request.runtime),
        )
        .await
    }
    pub async fn discover(&self, path: &'static str, language: &str) -> Result<Value, String> {
        self.request(path, language, None, None).await
    }
}

fn context_header(runtime: &str) -> Result<String, String> {
    let json = serde_json::to_string(runtime).map_err(|e| e.to_string())?;
    let mut ascii = String::new();
    for ch in json.chars() {
        if ch.is_ascii() {
            ascii.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                ascii.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    Ok(ascii)
}
#[cfg(test)]
mod tests {
    #[test]
    fn context_is_ascii_and_roundtrips_unicode_spaces_and_quotes() {
        let root = "/synthetic/Über Wien/✈/\"quoted\"";
        let header = super::context_header(root).expect("header");
        assert!(header.is_ascii());
        assert_eq!(serde_json::from_str::<String>(&header).expect("JSON"), root);
    }
}
