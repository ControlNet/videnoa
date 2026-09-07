//! Allocation-only stage fixtures verify job teardown without loading a GPU model.

use super::*;

fn rss_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/smaps_rollup")
        .expect("read resident memory")
        .lines()
        .find_map(|line| line.strip_prefix("Rss:"))
        .expect("resident memory field")
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap()
        * 1024
}

#[test]
fn video_context_returns_freed_pages_on_success_error_and_unwind() {
    const CHILD_MODE: &str = "VIDENOA_CONTEXT_MEMORY_TEST_MODE";
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        // Allocator policy and RSS assertions must not affect parallel unit tests.
        for mode in ["success", "error", "panic"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "nodes::compile_context::memory_tests::video_context_returns_freed_pages_on_success_error_and_unwind",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_MODE, mode)
                .env("MALLOC_MMAP_MAX_", "0")
                .env("MALLOC_TRIM_THRESHOLD_", "1073741824")
                .env("MALLOC_ARENA_MAX", "1")
                .output()
                .expect("run isolated allocator regression");
            assert!(
                output.status.success(),
                "{mode}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };

    let mut sentinels = Vec::new();
    let mut before_drop = 0;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        let context = VideoCompileContext::default();
        let mut buffers = Vec::new();
        for _ in 0..96 {
            buffers.push(vec![0x5a_u8; 1024 * 1024]);
            // Live allocations between freed blocks prevent automatic top trimming.
            sentinels.push(vec![0xa5_u8; 4096]);
        }
        std::hint::black_box(&buffers);
        let stage = move |_: &Frame, _: &Frame, _: bool, _: &ExecutionContext| {
            std::hint::black_box(&buffers);
            Ok(Vec::new())
        };
        context
            .accumulated_stages
            .borrow_mut()
            .push(PipelineStage::Interpolator(Box::new(stage)));
        before_drop = rss_bytes();
        match mode.as_str() {
            "error" => bail!("intentional compilation failure"),
            "panic" => panic!("intentional compilation unwind"),
            "success" => Ok(()),
            _ => panic!("unexpected test mode"),
        }
    }));
    match mode.as_str() {
        "success" => assert!(result.unwrap().is_ok()),
        "error" => assert!(result.unwrap().is_err()),
        "panic" => assert!(result.is_err()),
        _ => unreachable!(),
    }
    let after_drop = rss_bytes();
    assert!(
        before_drop.saturating_sub(after_drop) > 64 * 1024 * 1024,
        "job teardown retained freed pages: before={before_drop}, after={after_drop}"
    );
    assert!(sentinels.iter().flatten().all(|byte| *byte == 0xa5));
}
