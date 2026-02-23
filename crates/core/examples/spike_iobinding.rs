//! IoBinding spike: bind GPU input → inference → GPU output, compare with session.run().
//! Usage: `cargo run --example spike_iobinding`

use std::time::Instant;

use anyhow::{Context, Result};
use ndarray::Array4;
use ort::{
    execution_providers::CUDAExecutionProvider,
    memory::{AllocationDevice, Allocator, AllocatorType, MemoryInfo, MemoryType},
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

const MODEL_PATH: &str = "models/RealESRGAN_x4plus_anime_6B.onnx";

const INPUT_H: usize = 160;
const INPUT_W: usize = 240;
const OUTPUT_H: usize = INPUT_H * 4;
const OUTPUT_W: usize = INPUT_W * 4;

const NUM_RUNS: usize = 10;

fn main() -> Result<()> {
    println!("=== videnoa IoBinding validation spike ===\n");

    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])?
        .commit_from_file(MODEL_PATH)
        .context("Failed to load ONNX model")?;

    println!("Model loaded: {MODEL_PATH}");
    println!("Input: [1, 3, {INPUT_H}, {INPUT_W}], Output: [1, 3, {OUTPUT_H}, {OUTPUT_W}]\n");

    let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
    let input_tensor = Tensor::from_array(input_array.clone())?;

    // --- Baseline: session.run() ---
    println!("--- Baseline: session.run() ---");
    let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;

    let mut run_total = std::time::Duration::ZERO;
    for _ in 0..NUM_RUNS {
        let t = Instant::now();
        let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;
        run_total += t.elapsed();
    }
    let run_avg = run_total / NUM_RUNS as u32;
    println!("session.run() avg: {run_avg:.2?} (over {NUM_RUNS} runs)");

    // --- IoBinding: bind_output_to_device (dynamic output shape) ---
    println!("\n--- IoBinding: bind_output_to_device ---");
    match test_iobinding_dynamic(&mut session, &input_tensor) {
        Ok(avg) => println!("IoBinding (dynamic output) avg: {avg:.2?} (over {NUM_RUNS} runs)"),
        Err(e) => println!("IoBinding (dynamic output) FAILED: {e}"),
    }

    // --- IoBinding: pre-allocated output tensor ---
    println!("\n--- IoBinding: pre-allocated output ---");
    match test_iobinding_preallocated(&mut session, &input_tensor) {
        Ok(avg) => println!("IoBinding (pre-alloc output) avg: {avg:.2?} (over {NUM_RUNS} runs)"),
        Err(e) => println!("IoBinding (pre-alloc output) FAILED: {e}"),
    }

    // --- IoBinding: CUDA_PINNED memory for input ---
    println!("\n--- IoBinding: CUDA_PINNED input memory ---");
    match test_iobinding_pinned_input(&mut session) {
        Ok(avg) => println!("IoBinding (pinned input) avg: {avg:.2?} (over {NUM_RUNS} runs)"),
        Err(e) => println!("IoBinding (pinned input) FAILED: {e}"),
    }

    // --- Summary ---
    println!("\n--- Summary ---");
    println!("session.run() avg:  {run_avg:.2?}");
    println!("(IoBinding results above for comparison)");

    println!("\n=== IoBinding spike completed ===");
    Ok(())
}

fn test_iobinding_dynamic(
    session: &mut Session,
    input_tensor: &Tensor<f32>,
) -> Result<std::time::Duration> {
    let mut binding = session.create_binding()?;
    binding.bind_input("image.1", input_tensor)?;
    binding.bind_output_to_device("image", &session.allocator().memory_info())?;

    session.run_binding(&binding)?;

    let mut total = std::time::Duration::ZERO;
    for _ in 0..NUM_RUNS {
        binding.bind_input("image.1", input_tensor)?;
        let t = Instant::now();
        let _outputs = session.run_binding(&binding)?;
        total += t.elapsed();
    }

    Ok(total / NUM_RUNS as u32)
}

fn test_iobinding_preallocated(
    session: &mut Session,
    input_tensor: &Tensor<f32>,
) -> Result<std::time::Duration> {
    let output_mem = MemoryInfo::new(
        AllocationDevice::CUDA_PINNED,
        0,
        AllocatorType::Device,
        MemoryType::CPUOutput,
    )?;
    let output_allocator = Allocator::new(session, output_mem)?;
    let output_tensor = Tensor::<f32>::new(&output_allocator, [1, 3, OUTPUT_H, OUTPUT_W])?;

    let mut binding = session.create_binding()?;
    binding.bind_input("image.1", input_tensor)?;
    binding.bind_output("image", output_tensor)?;

    session.run_binding(&binding)?;

    let mut total = std::time::Duration::ZERO;
    for _ in 0..NUM_RUNS {
        binding.bind_input("image.1", input_tensor)?;
        let t = Instant::now();
        let _outputs = session.run_binding(&binding)?;
        total += t.elapsed();
    }

    Ok(total / NUM_RUNS as u32)
}

fn test_iobinding_pinned_input(session: &mut Session) -> Result<std::time::Duration> {
    // CUDA_PINNED tensors cannot use extract_array_mut() in ort v2.0-rc.11
    // (Error: "Cannot extract from value on device CudaPinned, which is not CPU accessible")
    // Workaround: use regular CPU tensor with bind_input (copies to device automatically)
    // and pre-allocated CUDA_PINNED output tensor
    let output_mem = MemoryInfo::new(
        AllocationDevice::CUDA_PINNED,
        0,
        AllocatorType::Device,
        MemoryType::CPUOutput,
    )?;
    let output_allocator = Allocator::new(session, output_mem)?;
    let output_tensor = Tensor::<f32>::new(&output_allocator, [1, 3, OUTPUT_H, OUTPUT_W])?;

    let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
    let input_tensor = Tensor::from_array(input_array)?;

    let mut binding = session.create_binding()?;
    binding.bind_input("image.1", &input_tensor)?;
    binding.bind_output("image", output_tensor)?;

    session.run_binding(&binding)?;

    let mut total = std::time::Duration::ZERO;
    for _ in 0..NUM_RUNS {
        binding.bind_input("image.1", &input_tensor)?;
        let t = Instant::now();
        let _outputs = session.run_binding(&binding)?;
        total += t.elapsed();
    }

    Ok(total / NUM_RUNS as u32)
}
