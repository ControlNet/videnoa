use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::{Response, StatusCode};
use serde_json::{json, Value};

use super::domain::{
    CreateJobResponse, FileStatResponse, HttpResult, JobResponse, PresetResponse, UploadResponse,
    WorkflowEntry, WorkflowInterface,
};

#[derive(Clone)]
pub struct MockClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    eligibility: BTreeMap<String, bool>,
}

impl Catalog {
    pub fn contains_eligible(&self, name: &str) -> bool {
        self.eligibility.get(name) == Some(&true)
    }

    pub fn contains_incompatible(&self, name: &str) -> bool {
        self.eligibility.get(name) == Some(&false)
    }
}

impl MockClient {
    pub fn new(base_url: &str) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(0)
            .build()?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            http,
        })
    }

    pub async fn health(&self) -> Result<Health, reqwest::Error> {
        self.health_raw().await?.error_for_status()?.json().await
    }

    pub async fn health_raw(&self) -> Result<Response, reqwest::Error> {
        self.http
            .get(format!("{}/api/health", self.base_url))
            .send()
            .await
    }

    pub async fn catalog(&self) -> Result<Catalog, reqwest::Error> {
        let workflows: Vec<WorkflowEntry> = self
            .http
            .get(format!("{}/api/workflows", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let presets: Vec<PresetResponse> = self
            .http
            .get(format!("{}/api/presets", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let names = workflows
            .into_iter()
            .map(|workflow| workflow.filename)
            .chain(presets.into_iter().map(|preset| preset.id));
        let mut eligibility = BTreeMap::new();
        for name in names {
            let interface: WorkflowInterface = self
                .http
                .get(format!("{}/api/workflows/{name}/interface", self.base_url))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            eligibility.insert(name, is_eligible(&interface));
        }
        Ok(Catalog { eligibility })
    }

    pub async fn upload(&self, path: &str, bytes: &[u8]) -> Result<UploadResponse, reqwest::Error> {
        self.http
            .put(format!("{}/api/files/{path}", self.base_url))
            .header("content-type", "application/octet-stream")
            .body(bytes.to_vec())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    pub async fn run(
        &self,
        workflow_name: &str,
        key: &str,
        params: Value,
    ) -> Result<HttpResult<CreateJobResponse>, reqwest::Error> {
        let response = self.run_raw(workflow_name, key, params).await?;
        let status = response.status();
        let body = response.error_for_status()?.json().await?;
        Ok(HttpResult { status, body })
    }

    pub async fn run_raw(
        &self,
        workflow_name: &str,
        key: &str,
        params: Value,
    ) -> Result<Response, reqwest::Error> {
        self.http
            .post(format!("{}/api/run", self.base_url))
            .header("idempotency-key", key)
            .json(&json!({"workflow_name": workflow_name, "params": params}))
            .send()
            .await
    }

    pub async fn job(&self, id: &str) -> Result<JobResponse, reqwest::Error> {
        self.job_raw(id).await?.error_for_status()?.json().await
    }

    pub async fn job_raw(&self, id: &str) -> Result<Response, reqwest::Error> {
        self.http
            .get(format!("{}/api/jobs/{id}", self.base_url))
            .send()
            .await
    }

    pub async fn download(&self, path: &str) -> Result<Vec<u8>, reqwest::Error> {
        Ok(self
            .http
            .get(format!("{}/api/files/{path}", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }

    pub async fn stat(&self, path: &str) -> Result<FileStatResponse, reqwest::Error> {
        self.http
            .get(format!("{}/api/files/{path}/stat", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    pub async fn delete(&self, path: &str) -> Result<StatusCode, reqwest::Error> {
        Ok(self
            .http
            .delete(format!("{}/api/files/{path}", self.base_url))
            .send()
            .await?
            .status())
    }
}

#[derive(serde::Deserialize)]
pub struct Health {
    pub status: String,
}

fn is_eligible(interface: &WorkflowInterface) -> bool {
    ["input", "output"].into_iter().all(|name| {
        interface
            .inputs
            .iter()
            .any(|port| port.name == name && port.port_type == "Path")
    })
}
