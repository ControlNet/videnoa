use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use url::Url;

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct JellyfinVideoNode;

impl JellyfinVideoNode {
    pub fn new(_params: &HashMap<String, serde_json::Value>) -> Result<Self> {
        Ok(Self)
    }

    fn required_str(inputs: &HashMap<String, PortData>, name: &str) -> Result<String> {
        let value = match inputs.get(name) {
            Some(PortData::Str(s)) => s.trim(),
            _ => bail!("missing or invalid '{name}' input (expected Str)"),
        };

        if value.is_empty() {
            bail!("'{name}' must not be empty");
        }

        Ok(value.to_string())
    }

    fn build_download_url(base_url: &str, item_id: &str, api_key: &str) -> Result<Url> {
        let mut url = Url::parse(base_url).context("invalid Jellyfin base URL")?;

        {
            let mut path_segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid Jellyfin base URL"))?;
            path_segments.clear();
            path_segments.push("Items");
            path_segments.push(item_id);
            path_segments.push("Download");
        }

        url.query_pairs_mut().clear().append_pair("ApiKey", api_key);

        Ok(url)
    }
}

impl Node for JellyfinVideoNode {
    fn node_type(&self) -> &str {
        "jellyfin_video"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "jellyfin_url".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "api_key".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "item_id".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "video_url".to_string(),
            port_type: PortType::Str,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let jellyfin_url = Self::required_str(inputs, "jellyfin_url")?;
        let api_key = Self::required_str(inputs, "api_key")?;
        let item_id = Self::required_str(inputs, "item_id")?;

        let video_url = Self::build_download_url(&jellyfin_url, &item_id, &api_key)?;

        let mut outputs = HashMap::new();
        outputs.insert(
            "video_url".to_string(),
            PortData::Str(video_url.to_string()),
        );
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_inputs() -> HashMap<String, PortData> {
        let mut inputs = HashMap::new();
        inputs.insert(
            "jellyfin_url".to_string(),
            PortData::Str("http://localhost:8096".to_string()),
        );
        inputs.insert("api_key".to_string(), PortData::Str("test-key".to_string()));
        inputs.insert("item_id".to_string(), PortData::Str("abc123".to_string()));
        inputs
    }

    #[test]
    fn test_node_ports() {
        let node = JellyfinVideoNode;

        assert_eq!(node.node_type(), "jellyfin_video");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].name, "jellyfin_url");
        assert_eq!(inputs[0].port_type, PortType::Str);
        assert!(inputs[0].required);
        assert_eq!(inputs[1].name, "api_key");
        assert_eq!(inputs[2].name, "item_id");

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "video_url");
        assert_eq!(outputs[0].port_type, PortType::Str);
        assert!(outputs[0].required);
    }

    #[test]
    fn test_execute_generates_download_url() {
        let mut node = JellyfinVideoNode;
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "jellyfin_url".to_string(),
            PortData::Str("http://localhost:8096/library?foo=bar".to_string()),
        );
        inputs.insert("api_key".to_string(), PortData::Str("k&=1".to_string()));
        inputs.insert(
            "item_id".to_string(),
            PortData::Str("episode 01/α".to_string()),
        );

        let outputs = node.execute(&inputs, &ctx).unwrap();
        let actual = match outputs.get("video_url") {
            Some(PortData::Str(url)) => url,
            _ => panic!("expected PortData::Str for video_url"),
        };

        assert_eq!(
            actual,
            "http://localhost:8096/Items/episode%2001%2F%CE%B1/Download?ApiKey=k%26%3D1"
        );
    }

    #[test]
    fn test_execute_rejects_missing_or_empty_inputs() {
        let mut node = JellyfinVideoNode;
        let ctx = ExecutionContext::default();

        let err = node
            .execute(&HashMap::new(), &ctx)
            .err()
            .expect("missing jellyfin_url should fail");
        assert!(err.to_string().contains("jellyfin_url"));

        let mut missing_api_key = HashMap::new();
        missing_api_key.insert(
            "jellyfin_url".to_string(),
            PortData::Str("http://localhost:8096".to_string()),
        );
        let err = node
            .execute(&missing_api_key, &ctx)
            .err()
            .expect("missing api_key should fail");
        assert!(err.to_string().contains("api_key"));

        let mut empty_item_id = valid_inputs();
        empty_item_id.insert("item_id".to_string(), PortData::Str("   ".to_string()));
        let err = node
            .execute(&empty_item_id, &ctx)
            .err()
            .expect("empty item_id should fail");
        assert!(err.to_string().contains("item_id"));
    }

    #[test]
    fn test_execute_rejects_invalid_base_url() {
        let mut node = JellyfinVideoNode;
        let ctx = ExecutionContext::default();
        let mut inputs = valid_inputs();
        inputs.insert(
            "jellyfin_url".to_string(),
            PortData::Str("not a url".to_string()),
        );

        let err = node
            .execute(&inputs, &ctx)
            .err()
            .expect("invalid base URL should fail");
        let msg = err.to_string();
        assert!(msg.contains("invalid Jellyfin base URL"), "got: {msg}");
    }

    #[test]
    fn test_build_download_url_rejects_non_base_url() {
        let err = JellyfinVideoNode::build_download_url("mailto:test@example.com", "id", "key")
            .err()
            .expect("non-http(s) base URL should fail");
        assert!(err.to_string().contains("invalid Jellyfin base URL"));
    }
}
