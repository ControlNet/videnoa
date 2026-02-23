//! TensorRT EP validation spike: TRT vs CUDA EP comparison + engine caching.
//!
//! Tests:
//! 1. TRT EP availability check
//! 2. TRT EP with engine cache (first run = compile, second run = cache hit)
//! 3. CUDA EP vs TRT EP inference time comparison
//!
//! Usage: `cargo run --example spike_trt`
//!
//! Runtime env vars:
//!   ORT_DYLIB_PATH=$PWD/ort-dist/lib/libonnxruntime.so
//!   LD_LIBRARY_PATH=$PWD/ort-dist/lib:$CONDA_PREFIX/lib:$LD_LIBRARY_PATH

use std::time::Instant;

use anyhow::{Context, Result};
use ndarray::Array4;
use ort::{
    execution_providers::{CUDAExecutionProvider, ExecutionProvider, TensorRTExecutionProvider},
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

const MODEL_PATH: &str = "models/RealESRGAN_x4plus_anime_6B.onnx";
const TRT_CACHE_DIR: &str = "trt_cache";

/// Small input to avoid OOM during TRT engine compilation (TRT needs extra workspace memory).
const INPUT_H: usize = 160;
const INPUT_W: usize = 240;

const NUM_BENCHMARK_RUNS: usize = 5;

fn main() -> Result<()> {
    videnoa_core::runtime::setup_runtime_libs();

    println!("=== videnoa TensorRT EP validation spike ===\n");

    match std::env::var("ORT_DYLIB_PATH") {
        Ok(path) => println!("ORT_DYLIB_PATH: {path}"),
        Err(_) => println!("ORT_DYLIB_PATH: <unset>"),
    }

    // --- Step 0: Check EP availability ---
    let cuda = CUDAExecutionProvider::default();
    let trt = TensorRTExecutionProvider::default();
    println!("CUDA EP available: {:?}", cuda.is_available());
    println!("TRT  EP available: {:?}", trt.is_available());

    println!("\n--- CUDA EP Baseline ---");
    let cuda_times = run_benchmark_with_cuda()?;
    println!(
        "CUDA EP: load={:.2?}, warmup={:.2?}, avg_infer={:.2?} (over {} runs)",
        cuda_times.load, cuda_times.warmup, cuda_times.avg_inference, NUM_BENCHMARK_RUNS
    );

    // Probe TRT EP: is_available() checks compile-time support, but runtime needs libnvinfer
    let trt_runtime_ok = probe_trt_runtime();
    println!(
        "\nTRT runtime probe: {}",
        if trt_runtime_ok {
            "OK"
        } else {
            "FAILED (libnvinfer missing)"
        }
    );

    if trt_runtime_ok {
        std::fs::create_dir_all(TRT_CACHE_DIR)?;

        println!("\n--- TensorRT EP: First run (engine compilation) ---");
        let trt_first = run_trt_inference(true)?;
        println!(
            "TRT first run: load={:.2?}, warmup={:.2?}, infer={:.2?}",
            trt_first.load, trt_first.warmup, trt_first.avg_inference
        );

        println!("\n--- TensorRT EP: Second run (cache hit) ---");
        let trt_cached = run_trt_inference(false)?;
        println!(
            "TRT cached run: load={:.2?}, warmup={:.2?}, infer={:.2?}",
            trt_cached.load, trt_cached.warmup, trt_cached.avg_inference
        );

        println!("\n--- Performance Comparison ---");
        println!(
            "  CUDA EP avg inference:        {:.2?}",
            cuda_times.avg_inference
        );
        println!(
            "  TRT EP avg inference (cached): {:.2?}",
            trt_cached.avg_inference
        );
        let speedup =
            cuda_times.avg_inference.as_secs_f64() / trt_cached.avg_inference.as_secs_f64();
        println!("  TRT speedup: {:.2}x", speedup);
        println!(
            "  TRT engine compile time: {:.2?} (first load)",
            trt_first.load
        );
        println!(
            "  TRT cached load time:    {:.2?} (subsequent loads)",
            trt_cached.load
        );

        let cache_files: Vec<_> = std::fs::read_dir(TRT_CACHE_DIR)?
            .filter_map(|e| e.ok())
            .collect();
        println!(
            "\nTRT cache dir ({TRT_CACHE_DIR}/): {} file(s)",
            cache_files.len()
        );
        for entry in &cache_files {
            let meta = entry.metadata()?;
            println!(
                "  {} ({:.1} MB)",
                entry.file_name().to_string_lossy(),
                meta.len() as f64 / 1_048_576.0
            );
        }
    } else {
        println!("\n--- TensorRT EP NOT available at runtime ---");
        println!("is_available()=true means ORT was compiled with TRT support,");
        println!("but libnvinfer.so.10 (TensorRT runtime) is not installed.");
        println!("\nTo enable TRT EP:");
        println!("  1. Install TensorRT 10.x matching CUDA version");
        println!("     conda install -c nvidia tensorrt");
        println!("  2. Ensure libnvinfer.so is in LD_LIBRARY_PATH");
        println!("  3. Re-run this spike");
        println!("\nTRT EP config (ready to use once libnvinfer is available):");
        println!("  TensorRTExecutionProvider::default()");
        println!("    .with_engine_cache(true)");
        println!("    .with_engine_cache_path(\"{TRT_CACHE_DIR}\")");
        println!("    .with_fp16(true)");
        println!("    .with_device_id(0)");

        println!("\n--- TRT→CUDA fallback (graceful degradation) ---");
        let mut session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers([
                TensorRTExecutionProvider::default()
                    .with_engine_cache(true)
                    .with_engine_cache_path(TRT_CACHE_DIR)
                    .with_fp16(true)
                    .build(),
                CUDAExecutionProvider::default().build(),
            ])?
            .commit_from_file(MODEL_PATH)
            .context("Failed to load model with TRT+CUDA fallback")?;
        println!("Session loaded (TRT unavailable, CUDA EP active as fallback)");

        let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
        let input_tensor = Tensor::from_array(input_array)?;
        let t = Instant::now();
        let outputs = session.run(ort::inputs!["image.1" => &input_tensor])?;
        let elapsed = t.elapsed();
        let output = outputs["image"].try_extract_array::<f32>()?;
        println!(
            "Inference OK: output={:?}, time={elapsed:.2?}",
            output.shape()
        );
    }

    println!("\n=== TRT EP spike completed ===");
    Ok(())
}

fn probe_trt_runtime() -> bool {
    match Session::builder()
        .and_then(|b| {
            b.with_execution_providers([TensorRTExecutionProvider::default()
                .build()
                .error_on_failure()])
        })
        .and_then(|b| b.commit_from_file(MODEL_PATH))
    {
        Ok(_) => true,
        Err(e) => {
            println!("TRT probe error: {e}");
            false
        }
    }
}

struct BenchmarkResult {
    load: std::time::Duration,
    warmup: std::time::Duration,
    avg_inference: std::time::Duration,
}

fn run_benchmark_with_cuda() -> Result<BenchmarkResult> {
    let t_load = Instant::now();
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])?
        .commit_from_file(MODEL_PATH)
        .context("Failed to load model with CUDA EP")?;
    let load = t_load.elapsed();

    let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
    let input_tensor = Tensor::from_array(input_array)?;

    // Warmup
    let t_warmup = Instant::now();
    let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;
    let warmup = t_warmup.elapsed();

    // Benchmark runs
    let mut total = std::time::Duration::ZERO;
    for _ in 0..NUM_BENCHMARK_RUNS {
        let t = Instant::now();
        let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;
        total += t.elapsed();
    }
    let avg_inference = total / NUM_BENCHMARK_RUNS as u32;

    Ok(BenchmarkResult {
        load,
        warmup,
        avg_inference,
    })
}

fn run_trt_inference(is_first_run: bool) -> Result<BenchmarkResult> {
    std::fs::create_dir_all(TRT_CACHE_DIR)?;

    if is_first_run {
        // Clean cache for first run test
        for entry in std::fs::read_dir(TRT_CACHE_DIR)? {
            let entry = entry?;
            std::fs::remove_file(entry.path()).ok();
        }
    }

    let t_load = Instant::now();
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_execution_providers([TensorRTExecutionProvider::default()
            .with_engine_cache(true)
            .with_engine_cache_path(TRT_CACHE_DIR)
            .with_fp16(true)
            .with_device_id(0)
            .build()
            .error_on_failure()])?
        .commit_from_file(MODEL_PATH)
        .context("Failed to load model with TRT EP")?;
    let load = t_load.elapsed();

    let input_array = Array4::<f32>::from_elem((1, 3, INPUT_H, INPUT_W), 0.5);
    let input_tensor = Tensor::from_array(input_array)?;

    // Warmup
    let t_warmup = Instant::now();
    let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;
    let warmup = t_warmup.elapsed();

    // Benchmark runs
    let mut total = std::time::Duration::ZERO;
    for _ in 0..NUM_BENCHMARK_RUNS {
        let t = Instant::now();
        let _ = session.run(ort::inputs!["image.1" => &input_tensor])?;
        total += t.elapsed();
    }
    let avg_inference = total / NUM_BENCHMARK_RUNS as u32;

    Ok(BenchmarkResult {
        load,
        warmup,
        avg_inference,
    })
}
