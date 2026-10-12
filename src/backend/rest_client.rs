//! Supabase REST (PostgREST) client — replaces direct Postgres pool usage.
//!
//! All data operations go through the Supabase REST API at
//! `https://<project_ref>.supabase.co/rest/v1/`. Each request is authenticated
//! with `apikey` + `Authorization: Bearer <jwt>` headers, and RLS enforces
//! row-level access control server-side.
//!
//! The client returns typed JSON rows deserialized via `serde_json::Value`,
//! keeping the conversion layer thin so `commands.rs` mirrors its previous
//! shape with minimal friction.

use crate::error::{AppError, Result};
use serde_json::Value;

/// A single row from PostgREST, accessible by column name.
pub type Row = serde_json::Map<String, Value>;

/// Supabase REST (PostgREST) client.
#[derive(Clone)]
pub struct SupabaseRestClient {
    client: reqwest::Client,
    base_url: String,
    anon_key: String,
}

impl SupabaseRestClient {
    pub fn new(supabase_url: &str, supabase_anon_key: &str) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            base_url: format!("{}/rest/v1", supabase_url.trim_end_matches('/')),
            anon_key: supabase_anon_key.to_string(),
        }
    }

    /// Build the common request headers: apikey, Authorization Bearer, and
    /// Prefer: return=minimal by default (caller can override via `prefer`).
    fn auth_headers(&self, token: &str, prefer: Option<&str>) -> Vec<(&str, String)> {
        let mut headers = vec![
            ("apikey", self.anon_key.clone()),
            ("Authorization", format!("Bearer {}", token)),
        ];
        if let Some(p) = prefer {
            headers.push(("Prefer", p.to_string()));
        } else {
            headers.push(("Prefer", "return=minimal".to_string()));
        }
        headers
    }

    /// GET /rest/v1/<table>?<query params> — returns matched rows.
    pub async fn select(
        &self,
        token: &str,
        table: &str,
        query: &str, // e.g. "select=*" or "select=col1,col2&discord_id=eq.123"
    ) -> Result<Vec<Row>> {
        let url = format!("{}/{}?{}", self.base_url, table, query);
        let headers = self.auth_headers(token, None);

        let req = self.build_request(&url, headers);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::SupabaseFunction(format!(
                "REST select on {} failed: {}", table, text
            )));
        }

        let value: Value = resp.json().await?;
        match value {
            Value::Array(arr) => Ok(arr
                .into_iter()
                .filter_map(|v| v.as_object().cloned())
                .collect()),
            Value::Object(obj) => Ok(vec![obj]),
            _ => Ok(Vec::new()),
        }
    }

    /// POST /rest/v1/<table> — insert one or more rows.
    /// If `prefer` is "return=representation", the response body contains
    /// the inserted rows.
    pub async fn insert(
        &self,
        token: &str,
        table: &str,
        rows: &[Value],
        prefer: Option<&str>,
    ) -> Result<Vec<Row>> {
        let url = format!("{}/{}", self.base_url, table);
        let headers = self.auth_headers(token, prefer);

        let body = if rows.len() == 1 {
            rows[0].clone()
        } else {
            Value::Array(rows.to_vec())
        };

        let req = self.build_request(&url, headers).json(&body);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::SupabaseFunction(format!(
                "REST insert on {} failed: {}", table, text
            )));
        }

        self.parse_response_rows(resp, prefer).await
    }

    /// PATCH /rest/v1/<table> — update rows matching `query` params.
    pub async fn update(
        &self,
        token: &str,
        table: &str,
        query: &str,
        row: &Value,
        prefer: Option<&str>,
    ) -> Result<Vec<Row>> {
        let url = format!("{}/{}?{}", self.base_url, table, query);
        let headers = self.auth_headers(token, prefer);

        let req = self.build_request_with_method(&url, reqwest::Method::PATCH, headers).json(row);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::SupabaseFunction(format!(
                "REST update on {} failed: {}", table, text
            )));
        }

        self.parse_response_rows(resp, prefer).await
    }

    /// DELETE /rest/v1/<table>?<query> — delete matching rows.
    pub async fn delete(
        &self,
        token: &str,
        table: &str,
        query: &str,
    ) -> Result<()> {
        let url = format!("{}/{}?{}", self.base_url, table, query);
        let headers = self.auth_headers(token, None);

        let req = self.build_request_with_method(&url, reqwest::Method::DELETE, headers);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::SupabaseFunction(format!(
                "REST delete on {} failed: {}", table, text
            )));
        }

        Ok(())
    }

    /// RPC /rest/rpc/<function> — call a stored procedure.
    pub async fn rpc(
        &self,
        token: &str,
        function: &str,
        args: &Value,
        prefer: Option<&str>,
    ) -> Result<Vec<Row>> {
        let url = format!("{}/rpc/{}", self.base_url, function);
        let headers = self.auth_headers(token, prefer);

        let req = self.build_request(&url, headers).json(args);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::SupabaseFunction(format!(
                "RPC {} failed: {}", function, text
            )));
        }

        self.parse_response_rows(resp, prefer).await
    }

    // ── helpers ──────────────────────────────────────────────

    fn build_request(&self, url: &str, headers: Vec<(&str, String)>) -> reqwest::RequestBuilder {
        self.build_request_with_method(url, reqwest::Method::GET, headers)
    }

    fn build_request_with_method(
        &self,
        url: &str,
        method: reqwest::Method,
        headers: Vec<(&str, String)>,
    ) -> reqwest::RequestBuilder {
        let mut req = self.client.request(method, url);
        for (key, val) in headers {
            req = req.header(key, val);
        }
        req
    }

    async fn parse_response_rows(
        &self,
        resp: reqwest::Response,
        prefer: Option<&str>,
    ) -> Result<Vec<Row>> {
        let want_representation = prefer
            .map(|p| p.contains("return=representation"))
            .unwrap_or(false);

        if !want_representation {
            // return=minimal → empty or minimal body
            return Ok(Vec::new());
        }

        let text = resp.text().await.unwrap_or_default();
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let value: Value = serde_json::from_str(&text).map_err(|e| {
            AppError::SupabaseFunction(format!("Failed to parse REST response: {}", e))
        })?;

        match value {
            Value::Array(arr) => Ok(arr
                .into_iter()
                .filter_map(|v| v.as_object().cloned())
                .collect()),
            Value::Object(obj) => Ok(vec![obj]),
            _ => Ok(Vec::new()),
        }
    }

    // ── query-param helpers ─────────────────────────────────

    /// Build a PostgREST `in=()` filter for an array of values.
    pub fn in_filter(column: &str, values: &[i64]) -> String {
        let inner = values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!("{}:in.({})", column, inner)
    }

    /// Build a PostgREST `select=` expression with embedded resource joins.
    pub fn select_expr(columns: &str) -> String {
        format!("select={}", columns)
    }
}

/// Extract a typed value from a PostgREST row by column name.
pub fn get_column(row: &Row, name: &str) -> Value {
    row.get(name).cloned().unwrap_or(Value::Null)
}

/// Extract a typed value from a stored `Value` (e.g. from HashMap<Value>).
pub fn get_column_from_value(val: &Value, name: &str) -> Value {
    match val {
        Value::Object(map) => map.get(name).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}
