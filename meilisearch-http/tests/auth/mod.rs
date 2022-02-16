mod api_keys;
mod authorization;
mod payload;
mod tenant_token;

use crate::common::Server;
use actix_web::http::StatusCode;

use serde_json::{json, Value};

impl Server {
    pub fn use_api_key(&mut self, api_key: impl AsRef<str>) {
        self.service.api_key = Some(api_key.as_ref().to_string());
    }

    pub async fn add_api_key(&self, content: Value) -> (Value, StatusCode) {
        let url = "/keys";
        self.service.post(url, content).await
    }

    pub async fn get_api_key(&self, key: impl AsRef<str>) -> (Value, StatusCode) {
        let url = format!("/keys/{}", key.as_ref());
        self.service.get(url).await
    }

    pub async fn patch_api_key(&self, key: impl AsRef<str>, content: Value) -> (Value, StatusCode) {
        let url = format!("/keys/{}", key.as_ref());
        self.service.patch(url, content).await
    }

    pub async fn list_api_keys(&self) -> (Value, StatusCode) {
        let url = "/keys";
        self.service.get(url).await
    }

    pub async fn delete_api_key(&self, key: impl AsRef<str>) -> (Value, StatusCode) {
        let url = format!("/keys/{}", key.as_ref());
        self.service.delete(url).await
    }

    pub async fn dummy_request(
        &self,
        method: impl AsRef<str>,
        url: impl AsRef<str>,
    ) -> (Value, StatusCode) {
        match method.as_ref() {
            "POST" => self.service.post(url, json!({})).await,
            "PUT" => self.service.put(url, json!({})).await,
            "PATCH" => self.service.patch(url, json!({})).await,
            "GET" => self.service.get(url).await,
            "DELETE" => self.service.delete(url).await,
            _ => unreachable!(),
        }
    }
}
