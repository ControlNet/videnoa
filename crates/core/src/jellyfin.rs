use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use url::Url;

/// Basic server information returned by `GET /System/Info`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SystemInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
}

/// Media library from `GET /Library/VirtualFolders`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Library {
    pub name: String,
    pub item_id: String,
    pub collection_type: Option<String>,
    pub locations: Vec<String>,
}

/// Query parameters for `GET /Items`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_item_types: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_term: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recursive: Option<bool>,
}

/// Paginated response from `GET /Items`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemsResponse {
    pub items: Vec<MediaItem>,
    pub total_record_count: u64,
}

/// A Jellyfin media item (movie, episode, series, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaItem {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    #[serde(rename = "Type")]
    pub type_: String,
    pub series_name: Option<String>,
    pub season_name: Option<String>,
    pub index_number: Option<u32>,
    pub overview: Option<String>,
}

/// Authenticated Jellyfin REST API client.
#[derive(Debug)]
pub struct JellyfinClient {
    base_url: Url,
    api_key: String,
    client: reqwest::Client,
}

impl JellyfinClient {
    /// Create a client authenticating via `X-Emby-Token` header.
    pub fn new(base_url: &str, api_key: &str) -> Result<Self> {
        let base_url = Url::parse(base_url).context("invalid Jellyfin base URL")?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(api_key).context("invalid API key characters")?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            base_url,
            api_key: api_key.to_string(),
            client,
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .with_context(|| format!("failed to build URL for path: {path}"))
    }

    /// `GET /System/Info` — verify connectivity.
    pub async fn get_system_info(&self) -> Result<SystemInfo> {
        let url = self.url("/System/Info")?;
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .context("failed to reach Jellyfin server")?;

        if !resp.status().is_success() {
            bail!(
                "Jellyfin /System/Info returned HTTP {}",
                resp.status().as_u16()
            );
        }

        resp.json::<SystemInfo>()
            .await
            .context("failed to parse SystemInfo response")
    }

    /// `GET /Library/VirtualFolders` — list media libraries.
    pub async fn get_libraries(&self) -> Result<Vec<Library>> {
        let url = self.url("/Library/VirtualFolders")?;
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .context("failed to fetch libraries")?;

        if !resp.status().is_success() {
            bail!(
                "Jellyfin /Library/VirtualFolders returned HTTP {}",
                resp.status().as_u16()
            );
        }

        resp.json::<Vec<Library>>()
            .await
            .context("failed to parse libraries response")
    }

    /// `GET /Items` — list media items with filters.
    pub async fn get_items(&self, params: &ItemQuery) -> Result<ItemsResponse> {
        let url = self.url("/Items")?;

        let resp = self
            .client
            .get(url)
            .query(params)
            .send()
            .await
            .context("failed to fetch items")?;

        if !resp.status().is_success() {
            bail!("Jellyfin /Items returned HTTP {}", resp.status().as_u16());
        }

        resp.json::<ItemsResponse>()
            .await
            .context("failed to parse items response")
    }

    /// `GET /Items?Ids={id}` — fetch a single item. Jellyfin wraps the result
    /// in an `ItemsResponse`, so we extract the first element.
    pub async fn get_item(&self, item_id: &str) -> Result<MediaItem> {
        let url = self.url("/Items")?;

        let resp = self
            .client
            .get(url)
            .query(&[("Ids", item_id), ("Fields", "Path,Overview")])
            .send()
            .await
            .with_context(|| format!("failed to fetch item {item_id}"))?;

        if !resp.status().is_success() {
            bail!(
                "Jellyfin /Items?Ids={item_id} returned HTTP {}",
                resp.status().as_u16()
            );
        }

        let items_resp: ItemsResponse =
            resp.json().await.context("failed to parse item response")?;

        items_resp
            .items
            .into_iter()
            .next()
            .with_context(|| format!("item not found: {item_id}"))
    }

    /// Resolve item ID → local filesystem path via `MediaItem.Path`.
    pub async fn get_item_file_path(&self, item_id: &str) -> Result<PathBuf> {
        let item = self.get_item(item_id).await?;
        let path_str = item
            .path
            .with_context(|| format!("item {item_id} has no file path (may be a virtual item)"))?;
        Ok(PathBuf::from(path_str))
    }

    /// `POST /Library/Refresh` — trigger a library rescan.
    pub async fn trigger_library_scan(&self) -> Result<()> {
        let url = self.url("/Library/Refresh")?;
        let resp = self
            .client
            .post(url)
            .send()
            .await
            .context("failed to trigger library scan")?;

        if !resp.status().is_success() {
            bail!(
                "Jellyfin /Library/Refresh returned HTTP {}",
                resp.status().as_u16()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation_valid_url() {
        let client = JellyfinClient::new("http://localhost:8096", "test-api-key");
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.base_url().as_str(), "http://localhost:8096/");
        assert_eq!(client.api_key(), "test-api-key");
    }

    #[test]
    fn test_client_creation_invalid_url() {
        let client = JellyfinClient::new("not a url", "key");
        assert!(client.is_err());
        let err = client.unwrap_err().to_string();
        assert!(err.contains("invalid Jellyfin base URL"), "got: {err}");
    }

    #[test]
    fn test_client_url_construction() {
        let client = JellyfinClient::new("http://myserver:8096", "key").unwrap();
        let url = client.url("/System/Info").unwrap();
        assert_eq!(url.as_str(), "http://myserver:8096/System/Info");

        let url = client.url("/Library/VirtualFolders").unwrap();
        assert_eq!(url.as_str(), "http://myserver:8096/Library/VirtualFolders");
    }

    #[test]
    fn test_client_url_with_trailing_slash() {
        let client = JellyfinClient::new("http://myserver:8096/", "key").unwrap();
        let url = client.url("/Items").unwrap();
        assert_eq!(url.as_str(), "http://myserver:8096/Items");
    }

    #[test]
    fn test_item_query_serialization_full() {
        let query = ItemQuery {
            parent_id: Some("abc123".to_string()),
            include_item_types: Some("Movie,Episode".to_string()),
            search_term: Some("naruto".to_string()),
            limit: Some(20),
            start_index: Some(0),
            fields: Some("Path,Overview".to_string()),
            recursive: Some(true),
        };

        let qs = serde_qs_manual(&query);
        assert!(qs.contains("ParentId=abc123"), "got: {qs}");
        assert!(qs.contains("IncludeItemTypes=Movie"), "got: {qs}");
        assert!(qs.contains("Limit=20"), "got: {qs}");
        assert!(qs.contains("StartIndex=0"), "got: {qs}");
        assert!(qs.contains("Recursive=true"), "got: {qs}");
    }

    #[test]
    fn test_item_query_serialization_empty() {
        let query = ItemQuery::default();
        let json = serde_json::to_value(&query).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.is_empty(), "expected empty object, got: {json}");
    }

    #[test]
    fn test_deserialize_system_info() {
        let json = r#"{
            "ServerName": "My Jellyfin",
            "Version": "10.9.0",
            "Id": "abc123def456"
        }"#;

        let info: SystemInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.server_name, "My Jellyfin");
        assert_eq!(info.version, "10.9.0");
        assert_eq!(info.id, "abc123def456");
    }

    #[test]
    fn test_deserialize_library() {
        let json = r#"{
            "Name": "Anime",
            "ItemId": "f5e4d3c2b1a0",
            "CollectionType": "tvshows",
            "Locations": ["/media/anime", "/media/anime2"]
        }"#;

        let lib: Library = serde_json::from_str(json).unwrap();
        assert_eq!(lib.name, "Anime");
        assert_eq!(lib.item_id, "f5e4d3c2b1a0");
        assert_eq!(lib.collection_type.as_deref(), Some("tvshows"));
        assert_eq!(lib.locations, vec!["/media/anime", "/media/anime2"]);
    }

    #[test]
    fn test_deserialize_library_no_collection_type() {
        let json = r#"{
            "Name": "Mixed",
            "ItemId": "aaa111",
            "Locations": ["/media/mixed"]
        }"#;

        let lib: Library = serde_json::from_str(json).unwrap();
        assert_eq!(lib.name, "Mixed");
        assert!(lib.collection_type.is_none());
    }

    #[test]
    fn test_deserialize_items_response() {
        let json = r#"{
            "Items": [
                {
                    "Id": "item001",
                    "Name": "Episode 01 - Pilot",
                    "Path": "/media/anime/show/S01E01.mkv",
                    "Type": "Episode",
                    "SeriesName": "My Anime",
                    "SeasonName": "Season 1",
                    "IndexNumber": 1,
                    "Overview": "The adventure begins."
                },
                {
                    "Id": "item002",
                    "Name": "Episode 02 - Journey",
                    "Path": "/media/anime/show/S01E02.mkv",
                    "Type": "Episode",
                    "SeriesName": "My Anime",
                    "SeasonName": "Season 1",
                    "IndexNumber": 2
                }
            ],
            "TotalRecordCount": 24
        }"#;

        let resp: ItemsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total_record_count, 24);
        assert_eq!(resp.items.len(), 2);

        let ep1 = &resp.items[0];
        assert_eq!(ep1.id, "item001");
        assert_eq!(ep1.name, "Episode 01 - Pilot");
        assert_eq!(ep1.path.as_deref(), Some("/media/anime/show/S01E01.mkv"));
        assert_eq!(ep1.type_, "Episode");
        assert_eq!(ep1.series_name.as_deref(), Some("My Anime"));
        assert_eq!(ep1.season_name.as_deref(), Some("Season 1"));
        assert_eq!(ep1.index_number, Some(1));
        assert_eq!(ep1.overview.as_deref(), Some("The adventure begins."));

        let ep2 = &resp.items[1];
        assert_eq!(ep2.index_number, Some(2));
        assert!(ep2.overview.is_none());
    }

    #[test]
    fn test_deserialize_media_item_movie() {
        let json = r#"{
            "Id": "mov001",
            "Name": "Spirited Away",
            "Path": "/media/movies/Spirited Away (2001)/Spirited Away.mkv",
            "Type": "Movie",
            "Overview": "A young girl becomes trapped in a strange world of spirits."
        }"#;

        let item: MediaItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, "mov001");
        assert_eq!(item.type_, "Movie");
        assert!(item.series_name.is_none());
        assert!(item.season_name.is_none());
        assert!(item.index_number.is_none());
        assert!(item.path.is_some());
    }

    #[test]
    fn test_deserialize_media_item_no_path() {
        // Series/Season items don't have a file path
        let json = r#"{
            "Id": "ser001",
            "Name": "My Anime",
            "Type": "Series"
        }"#;

        let item: MediaItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.type_, "Series");
        assert!(item.path.is_none());
    }

    #[test]
    fn test_deserialize_items_response_empty() {
        let json = r#"{"Items": [], "TotalRecordCount": 0}"#;
        let resp: ItemsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.items.len(), 0);
        assert_eq!(resp.total_record_count, 0);
    }

    fn serde_qs_manual(query: &ItemQuery) -> String {
        let val = serde_json::to_value(query).unwrap();
        let obj = val.as_object().unwrap();
        obj.iter()
            .map(|(k, v)| {
                let v_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{k}={v_str}")
            })
            .collect::<Vec<_>>()
            .join("&")
    }
}
