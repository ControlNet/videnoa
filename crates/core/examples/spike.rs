//! Validation spike: ort + CUDA EP inference on a single frame.
//! Usage: `cargo run --example spike`

use std::time::Instant;

use anyhow::{Context, Result};
use ndarray::Array4;
use ort::{
    execution_providers::{CUDAExecutionProvider, ExecutionProvider},
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

const MODEL_PATH: &str = "models/RealESRGAN_x4plus_anime_6B.onnx";

/// NCHW dimensions for the dummy input tensor (720p; 1080p OOMs on this GPU due to 4x upscale intermediates)
const INPUT_H: usize = 720;
const INPUT_W: usize = 1280;

fn main() -> Result<()> {
    println!("=== videnoa CUDA EP validation spike ===\n");

    let cuda = CUDAExecutionProvider::default();
    println!("CUDA EP available: {:?}", cuda.is_available());

    println!("Loading model: {MODEL_PATH}");
    let t_load = Instant::now();
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])?
        .commit_from_file(MODEL_PATH)
        .context("Failed to load ONNX model")?;
    let load_elapsed = t_load.elapsed();
    println!("Model loaded in {load_elapsed:.2?}");

    println!("\nModel inputs:");
    for input in session.inputs() {
        println!("  name={}, type={:?}", input.name(), input.dtype());
    }
    println!("Model outputs:");
    for output in session.outputs() {
        println!("  name={}, type={:?}", output.name(), output.dtype());
    }

    println!("\nCreating dummy input: [1, 3, {INPUT_H}, {INPUT_W}] float32");
    let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
    let input_tensor = Tensor::from_array(input_array)?;

    println!("Running warmup inference...");
    let t_warmup = Instant::now();
    let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;
    let warmup_elapsed = t_warmup.elapsed();
    println!("Warmup completed in {warmup_elapsed:.2?}");

    println!("\nRunning timed inference...");
    let t_infer = Instant::now();
    let outputs = session.run(ort::inputs!["image.1" => &input_tensor])?;
    let infer_elapsed = t_infer.elapsed();

    let output = &outputs["image"];
    let output_array = output.try_extract_array::<f32>()?;
    let output_shape = output_array.shape();

    println!("\n=== Results ===");
    println!("Input shape:  [1, 3, {INPUT_H}, {INPUT_W}]");
    println!("Output shape: {:?}", output_shape);
    println!(
        "Upscale factor: {}x (H: {} -> {}, W: {} -> {})",
        output_shape[2] / INPUT_H,
        INPUT_H,
        output_shape[2],
        INPUT_W,
        output_shape[3]
    );
    println!("Model load time:   {load_elapsed:.2?}");
    println!("Warmup inference:   {warmup_elapsed:.2?}");
    println!("Timed inference:    {infer_elapsed:.2?}");

    let min_val = output_array.iter().cloned().reduce(f32::min).unwrap_or(0.0);
    let max_val = output_array.iter().cloned().reduce(f32::max).unwrap_or(0.0);
    println!("Output value range: [{min_val:.4}, {max_val:.4}]");

    println!("\n=== Spike completed successfully ===");
    Ok(())
}
