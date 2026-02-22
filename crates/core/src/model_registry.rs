use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModelType {
    SuperResolution,
    FrameInterpolation,
}

impl std::fmt::Display for ModelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SuperResolution => write!(f, "SuperResolution"),
            Self::FrameInterpolation => write!(f, "FrameInterpolation"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    pub model_type: ModelType,
    pub filename: String,
    pub url: Option<String>,
    pub sha256: Option<String>,
    /// Upscale factor for super-res models (2, 3, or 4). `None` for interpolation.
    pub scale: Option<u32>,
    pub input_names: Vec<String>,
    pub output_names: Vec<String>,
    /// Value range the model expects/produces: `(0.0, 255.0)` for ESRGAN, `(0.0, 1.0)` for CUGAN/RIFE.
    pub normalization_range: (f32, f32),
    /// Spatial dimensions must be multiples of this (4 for ESRGAN, 32 for RIFE).
    pub pad_align: u32,
    pub description: String,
    /// Whether the model uses FP16 (half-precision) inputs/outputs.
    pub is_fp16: bool,
    /// Input format: "standard" (single RGB input), "concatenated" (single 7-ch input for RIFE v4.22+),
    /// or "three_input" (three separate tensors for RIFE v4.6/v4.7).
    pub input_format: String,
}

fn builtin_catalog() -> Vec<ModelEntry> {
    vec![
        ModelEntry {
            name: "RealESRGAN_x4plus_anime_6B".into(),
            model_type: ModelType::SuperResolution,
            filename: "RealESRGAN_x4plus_anime_6B.onnx".into(),
            url: Some("https://huggingface.co/deepghs/imgutils-models/resolve/main/onnx/realesrgan/RealESRGAN_x4plus_anime_6B.onnx".into()),
            sha256: None,
            scale: Some(4),
            input_names: vec!["image.1".into()],
            output_names: vec!["image".into()],
            normalization_range: (0.0, 255.0),
            pad_align: 4,
            description: "RealESRGAN x4 anime-optimized model (6-block variant, 17.9 MB)".into(),
            is_fp16: false,
            input_format: "standard".into(),
        },
        ModelEntry {
            name: "AnimeJaNai_V3_L1_Sharp_HD_x2_FP16".into(),
            model_type: ModelType::SuperResolution,
            filename: "the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx".into(),
            url: None,
            sha256: None,
            scale: Some(2),
            input_names: vec!["input".into()],
            output_names: vec!["output".into()],
            normalization_range: (0.0, 1.0),
            pad_align: 4,
            description: "AnimeJaNai V3 L1 Sharp HD 2x FP16 — Compact architecture, optimized for anime".into(),
            is_fp16: true,
            input_format: "standard".into(),
        },
        ModelEntry {
            name: "RIFE_v4.26".into(),
            model_type: ModelType::FrameInterpolation,
            filename: "rife_v4.26.onnx".into(),
            url: None,
            sha256: None,
            scale: None,
            input_names: vec!["input".into()],
            output_names: vec!["output".into()],
            normalization_range: (0.0, 1.0),
            pad_align: 32,
            description: "RIFE v4.26 frame interpolation — concatenated 7-channel input format".into(),
            is_fp16: false,
            input_format: "concatenated".into(),
        },
    ]
}

pub struct ModelRegistry {
    models_dir: PathBuf,
    entries: Vec<ModelEntry>,
}

impl ModelRegistry {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            entries: Vec::new(),
        }
    }

    pub fn with_builtin_models(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            entries: builtin_catalog(),
        }
    }

    pub fn discover(&mut self) -> Result<()> {
        let dir = &self.models_dir;
        if !dir.exists() {
            return Ok(());
        }

        let read_dir = fs::read_dir(dir)
            .with_context(|| format!("Failed to read models directory: {}", dir.display()))?;

        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();

            let is_onnx = path
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("onnx"))
                .unwrap_or(false);

            if !is_onnx {
                continue;
            }

            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            if self.entries.iter().any(|e| e.filename == filename) {
                continue;
            }

            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&filename)
                .to_string();

            info!(filename = %filename, "Discovered unknown ONNX model");

            let lower = filename.to_lowercase();
            let is_fp16 = lower.contains("fp16");
            let input_format = if lower.contains("rife") {
                "concatenated".to_string()
            } else {
                "standard".to_string()
            };

            self.entries.push(ModelEntry {
                name,
                model_type: ModelType::SuperResolution,
                filename,
                url: None,
                sha256: None,
                scale: None,
                input_names: Vec::new(),
                output_names: Vec::new(),
                normalization_range: (0.0, 1.0),
                pad_align: 4,
                description: "Discovered model (metadata unknown)".into(),
                is_fp16,
                input_format,
            });
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn list(&self) -> &[ModelEntry] {
        &self.entries
    }

    pub fn list_by_type(&self, model_type: ModelType) -> Vec<&ModelEntry> {
        self.entries
            .iter()
            .filter(|e| e.model_type == model_type)
            .collect()
    }

    pub fn is_downloaded(&self, name: &str) -> bool {
        self.get(name)
            .map(|e| self.models_dir.join(&e.filename).is_file())
            .unwrap_or(false)
    }

    pub fn model_path(&self, name: &str) -> Option<PathBuf> {
        self.get(name).map(|e| self.models_dir.join(&e.filename))
    }

    pub fn download(&self, name: &str) -> Result<PathBuf> {
        let entry = self
            .get(name)
            .with_context(|| format!("Unknown model: {name}"))?;

        let url = entry
            .url
            .as_deref()
            .with_context(|| format!("No download URL for model: {name}"))?;

        fs::create_dir_all(&self.models_dir).with_context(|| {
            format!(
                "Failed to create models directory: {}",
                self.models_dir.display()
            )
        })?;

        let final_path = self.models_dir.join(&entry.filename);
        let tmp_path = self.models_dir.join(format!("{}.part", entry.filename));

        info!(model = %name, url = %url, "Downloading model");

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30 * 60))
            .build()
            .context("Failed to build HTTP client for model download")?;

        let mut response = client
            .get(url)
            .send()
            .with_context(|| format!("Failed to start download for model {name}"))?;

        if !response.status().is_success() {
            let _ = fs::remove_file(&tmp_path);
            bail!(
                "Download request for model {name} returned HTTP {}",
                response.status().as_u16()
            );
        }

        let mut tmp_file = fs::File::create(&tmp_path)
            .with_context(|| format!("Failed to create temp file: {}", tmp_path.display()))?;

        if let Err(err) = response
            .copy_to(&mut tmp_file)
            .with_context(|| format!("Failed while downloading model {name} from {url}"))
        {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }

        if let Err(err) = tmp_file
            .sync_all()
            .with_context(|| format!("Failed to flush temp file: {}", tmp_path.display()))
        {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }

        if let Some(expected_hash) = &entry.sha256 {
            info!(model = %name, "Verifying SHA256 hash");
            let actual_hash = sha256_file(&tmp_path)?;
            if actual_hash != *expected_hash {
                let _ = fs::remove_file(&tmp_path);
                bail!("SHA256 mismatch for {name}: expected {expected_hash}, got {actual_hash}");
            }
            info!(model = %name, "Hash verified OK");
        } else {
            warn!(model = %name, "No SHA256 hash configured — skipping verification");
        }

        fs::rename(&tmp_path, &final_path).with_context(|| {
            format!(
                "Failed to move {} → {}",
                tmp_path.display(),
                final_path.display()
            )
        })?;

        info!(model = %name, path = %final_path.display(), "Download complete");
        Ok(final_path)
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.entries).context("Failed to serialize model catalog")
    }

    pub fn load_json(&mut self, json: &str) -> Result<()> {
        let loaded: Vec<ModelEntry> =
            serde_json::from_str(json).context("Failed to parse model catalog JSON")?;
        for entry in loaded {
            if !self.entries.iter().any(|e| e.name == entry.name) {
                self.entries.push(entry);
            }
        }
        Ok(())
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.write_all(&buf[..n])?;
    }
    let hash = hasher.finalize();
    Ok(format!("{hash:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_builtin_catalog_count() {
        let catalog = builtin_catalog();
        assert_eq!(catalog.len(), 3, "Expected 3 built-in models");
    }

    #[test]
    fn test_builtin_catalog_model_types() {
        let catalog = builtin_catalog();
        let sr_count = catalog
            .iter()
            .filter(|e| e.model_type == ModelType::SuperResolution)
            .count();
        let fi_count = catalog
            .iter()
            .filter(|e| e.model_type == ModelType::FrameInterpolation)
            .count();
        assert_eq!(sr_count, 2);
        assert_eq!(fi_count, 1);
    }

    #[test]
    fn test_with_builtin_models() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        assert_eq!(reg.list().len(), 3);
    }

    #[test]
    fn test_new_empty() {
        let reg = ModelRegistry::new(test_models_dir());
        assert!(reg.list().is_empty());
    }

    #[test]
    fn test_get_existing() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());

        let esrgan = reg.get("RealESRGAN_x4plus_anime_6B").unwrap();
        assert_eq!(esrgan.scale, Some(4));
        assert_eq!(esrgan.pad_align, 4);
        assert_eq!(esrgan.normalization_range, (0.0, 255.0));
        assert_eq!(esrgan.input_names, vec!["image.1"]);
        assert_eq!(esrgan.output_names, vec!["image"]);
        assert!(!esrgan.is_fp16);

        let animejanai = reg.get("AnimeJaNai_V3_L1_Sharp_HD_x2_FP16").unwrap();
        assert_eq!(animejanai.scale, Some(2));
        assert!(animejanai.is_fp16);
        assert_eq!(animejanai.normalization_range, (0.0, 1.0));

        let rife = reg.get("RIFE_v4.26").unwrap();
        assert_eq!(rife.model_type, ModelType::FrameInterpolation);
        assert_eq!(rife.input_format, "concatenated");
        assert_eq!(rife.pad_align, 32);
    }

    #[test]
    fn test_get_missing() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        assert!(reg.get("NonExistentModel").is_none());
    }

    #[test]
    fn test_list_by_type_super_res() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        let sr = reg.list_by_type(ModelType::SuperResolution);
        assert_eq!(sr.len(), 2);
        assert!(sr
            .iter()
            .all(|e| e.model_type == ModelType::SuperResolution));
    }

    #[test]
    fn test_list_by_type_interpolation() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        let fi = reg.list_by_type(ModelType::FrameInterpolation);
        assert_eq!(fi.len(), 1);
        assert!(fi
            .iter()
            .all(|e| e.model_type == ModelType::FrameInterpolation));
    }

    #[test]
    fn test_model_path() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        let path = reg.model_path("RealESRGAN_x4plus_anime_6B");
        assert_eq!(
            path,
            Some(test_models_dir().join("RealESRGAN_x4plus_anime_6B.onnx"))
        );
    }

    #[test]
    fn test_model_path_missing() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        assert!(reg.model_path("FakeModel").is_none());
    }

    #[test]
    fn test_is_downloaded_false() {
        let dir = tempdir();
        let reg = ModelRegistry::with_builtin_models(dir.clone());
        assert!(!reg.is_downloaded("RealESRGAN_x4plus_anime_6B"));
        cleanup(&dir);
    }

    #[test]
    fn test_is_downloaded_true() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("RealESRGAN_x4plus_anime_6B.onnx"),
            b"fake model data",
        )
        .unwrap();
        let reg = ModelRegistry::with_builtin_models(dir.clone());
        assert!(reg.is_downloaded("RealESRGAN_x4plus_anime_6B"));
        cleanup(&dir);
    }

    #[test]
    fn test_discover_empty_dir() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        let mut reg = ModelRegistry::with_builtin_models(dir.clone());
        reg.discover().unwrap();
        assert_eq!(reg.list().len(), 3);
        cleanup(&dir);
    }

    #[test]
    fn test_discover_known_model() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("RealESRGAN_x4plus_anime_6B.onnx"), b"data").unwrap();
        let mut reg = ModelRegistry::with_builtin_models(dir.clone());
        reg.discover().unwrap();
        assert_eq!(reg.list().len(), 3);
        cleanup(&dir);
    }

    #[test]
    fn test_discover_unknown_model() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("MyCustomModel.onnx"), b"data").unwrap();
        let mut reg = ModelRegistry::with_builtin_models(dir.clone());
        reg.discover().unwrap();
        assert_eq!(reg.list().len(), 4);
        let custom = reg.get("MyCustomModel");
        assert!(custom.is_some());
        assert_eq!(custom.unwrap().filename, "MyCustomModel.onnx");
        cleanup(&dir);
    }

    #[test]
    fn test_discover_nonexistent_dir() {
        let dir = std::env::temp_dir().join("videnoa_test_nonexistent_dir_xyz");
        let mut reg = ModelRegistry::with_builtin_models(dir);
        reg.discover().unwrap();
        assert_eq!(reg.list().len(), 3);
    }

    #[test]
    fn test_discover_ignores_non_onnx() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("readme.txt"), b"hello").unwrap();
        fs::write(dir.join("weights.bin"), b"data").unwrap();
        let mut reg = ModelRegistry::new(dir.clone());
        reg.discover().unwrap();
        assert!(reg.list().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_sha256_file() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("testfile.bin");
        fs::write(&path, b"hello world").unwrap();
        let hash = sha256_file(&path).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        cleanup(&dir);
    }

    #[test]
    fn test_json_roundtrip() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        let json = reg.to_json().unwrap();

        let mut reg2 = ModelRegistry::new(test_models_dir());
        reg2.load_json(&json).unwrap();
        assert_eq!(reg2.list().len(), 3);

        let entry = reg2.get("RealESRGAN_x4plus_anime_6B").unwrap();
        assert_eq!(entry.scale, Some(4));
        assert_eq!(entry.normalization_range, (0.0, 255.0));
    }

    #[test]
    fn test_load_json_no_duplicates() {
        let mut reg = ModelRegistry::with_builtin_models(test_models_dir());
        let json = reg.to_json().unwrap();
        reg.load_json(&json).unwrap();
        assert_eq!(reg.list().len(), 3);
    }

    #[test]
    fn test_download_no_url() {
        let dir = tempdir();
        let reg = ModelRegistry::with_builtin_models(dir.clone());
        let result = reg.download("RIFE_v4.26");
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            err.to_string().contains("No download URL"),
            "Expected 'No download URL' error, got: {err}"
        );
        cleanup(&dir);
    }

    #[test]
    fn test_download_unknown_model() {
        let dir = tempdir();
        let reg = ModelRegistry::with_builtin_models(dir.clone());
        let result = reg.download("NonExistentModel");
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            err.to_string().contains("Unknown model"),
            "Expected 'Unknown model' error, got: {err}"
        );
        cleanup(&dir);
    }

    #[test]
    fn test_model_type_display() {
        assert_eq!(ModelType::SuperResolution.to_string(), "SuperResolution");
        assert_eq!(
            ModelType::FrameInterpolation.to_string(),
            "FrameInterpolation"
        );
    }

    #[test]
    fn test_rife_model_metadata() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());
        let rife = reg.get("RIFE_v4.26").unwrap();
        assert_eq!(rife.model_type, ModelType::FrameInterpolation);
        assert_eq!(rife.pad_align, 32);
        assert_eq!(rife.input_names, vec!["input"]);
        assert_eq!(rife.output_names, vec!["output"]);
        assert_eq!(rife.input_format, "concatenated");
        assert!(rife.scale.is_none());
    }

    #[test]
    fn test_esrgan_vs_animejanai_normalization() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());

        let esrgan = reg.get("RealESRGAN_x4plus_anime_6B").unwrap();
        assert_eq!(esrgan.normalization_range, (0.0, 255.0));
        assert!(!esrgan.is_fp16);

        let animejanai = reg.get("AnimeJaNai_V3_L1_Sharp_HD_x2_FP16").unwrap();
        assert_eq!(animejanai.normalization_range, (0.0, 1.0));
        assert!(animejanai.is_fp16);
    }

    #[test]
    #[ignore]
    fn test_download_real() {
        let dir = tempdir();
        let reg = ModelRegistry::with_builtin_models(dir.clone());
        let path = reg.download("RealESRGAN_x4plus_anime_6B").unwrap();
        assert!(path.is_file());
        let meta = fs::metadata(&path).unwrap();
        assert!(meta.len() > 1_000_000, "Downloaded file is too small");
        cleanup(&dir);
    }

    #[test]
    fn test_builtin_fp16_and_format() {
        let reg = ModelRegistry::with_builtin_models(test_models_dir());

        let esrgan = reg.get("RealESRGAN_x4plus_anime_6B").unwrap();
        assert!(!esrgan.is_fp16);
        assert_eq!(esrgan.input_format, "standard");

        let animejanai = reg.get("AnimeJaNai_V3_L1_Sharp_HD_x2_FP16").unwrap();
        assert!(animejanai.is_fp16);
        assert_eq!(animejanai.input_format, "standard");

        let rife = reg.get("RIFE_v4.26").unwrap();
        assert!(!rife.is_fp16);
        assert_eq!(rife.input_format, "concatenated");
    }

    #[test]
    fn test_discover_fp16_model() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AnimeJaNai_fp16_x2.onnx"), b"data").unwrap();
        let mut reg = ModelRegistry::new(dir.clone());
        reg.discover().unwrap();
        let model = reg.get("AnimeJaNai_fp16_x2").unwrap();
        assert!(model.is_fp16);
        assert_eq!(model.input_format, "standard");
        cleanup(&dir);
    }

    #[test]
    fn test_discover_rife_model_format() {
        let dir = tempdir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("rife_v4.26_fp16.onnx"), b"data").unwrap();
        let mut reg = ModelRegistry::new(dir.clone());
        reg.discover().unwrap();
        let model = reg.get("rife_v4.26_fp16").unwrap();
        assert!(model.is_fp16);
        assert_eq!(model.input_format, "concatenated");
        cleanup(&dir);
    }

    fn tempdir() -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("videnoa_test_{id}"));
        dir
    }

    fn test_models_dir() -> PathBuf {
        std::env::temp_dir().join("models")
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }
}
