//! Async validation: Arc<Session> + tokio::task::spawn_blocking pattern.
//! Usage: `cargo run --example spike_async`

use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use ndarray::Array4;
use ort::{
    execution_providers::CUDAExecutionProvider,
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

const MODEL_PATH: &str = "models/RealESRGAN_x4plus_anime_6B.onnx";

/// NCHW dimensions for the dummy input tensor (720p; 1080p OOMs on this GPU due to 4x upscale intermediates)
const INPUT_H: usize = 720;
const INPUT_W: usize = 1280;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== videnoa async CUDA EP spike ===\n");

    println!("Loading model: {MODEL_PATH}");
    let session = Arc::new(Mutex::new(
        Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers([CUDAExecutionProvider::default()
                .build()
                .error_on_failure()])?
            .commit_from_file(MODEL_PATH)
            .context("Failed to load ONNX model")?,
    ));
    println!("Model loaded, Session wrapped in Arc<Mutex> (Send+Sync)");

    let session_clone = Arc::clone(&session);
    let handle = tokio::task::spawn_blocking(move || -> Result<()> {
        let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
        let input_tensor = Tensor::from_array(input_array)?;

        let t = Instant::now();
        let (shape, elapsed) = {
            let mut session = session_clone.lock().unwrap();
            let outputs = session.run(ort::inputs!["image.1" => &input_tensor])?;
            let elapsed = t.elapsed();
            let output_array = outputs["image"].try_extract_array::<f32>()?;
            (output_array.shape().to_vec(), elapsed)
        };

        println!("[spawn_blocking] Input:  [1, 3, {INPUT_H}, {INPUT_W}]");
        println!("[spawn_blocking] Output: {:?}", shape);
        println!("[spawn_blocking] Upscale: {}x", shape[2] / INPUT_H);
        println!("[spawn_blocking] Inference time: {elapsed:.2?}");
        Ok(())
    });

    handle.await??;

    println!("\nRunning 3 sequential inferences from async context...");
    for i in 0..3 {
        let s = Arc::clone(&session);
        let result = tokio::task::spawn_blocking(move || -> Result<std::time::Duration> {
            let input = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
            let tensor = Tensor::from_array(input)?;
            let t = Instant::now();
            {
                let mut session = s.lock().unwrap();
                let _ = session.run(ort::inputs!["image.1" => &tensor])?;
            }
            Ok(t.elapsed())
        })
        .await??;
        println!("  Run {}: {result:.2?}", i + 1);
    }

    println!("\n=== Async spike completed successfully ===");
    Ok(())
}
