//! Run without libtest so malloc is configured before any test harness threads exist.

fn main() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    linux::run();
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    println!("glibc allocator policy test does not apply to this platform");
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux {

    use std::sync::{Arc, Barrier};
    use videnoa_core::runtime::configure_host_memory;

    fn arena_count() -> usize {
        // SAFETY: open_memstream owns the buffer until fclose publishes its final
        // address and length. Copy the XML before freeing that C-allocated buffer.
        unsafe {
            let mut buffer = std::ptr::null_mut();
            let mut len = 0;
            let stream = libc::open_memstream(&mut buffer, &mut len);
            assert!(!stream.is_null());
            assert_eq!(libc::malloc_info(0, stream), 0);
            assert_eq!(libc::fclose(stream), 0);
            let xml = String::from_utf8_lossy(std::slice::from_raw_parts(buffer.cast::<u8>(), len));
            let count = xml.matches("<heap nr=").count();
            libc::free(buffer.cast());
            count
        }
    }

    pub(super) fn run() {
        const CHILD_MODE: &str = "VIDENOA_ALLOCATOR_POLICY_TEST_MODE";
        let Ok(mode) = std::env::var(CHILD_MODE) else {
            for mode in ["default", "environment", "tunable"] {
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .env(CHILD_MODE, mode)
                    .env_remove("MALLOC_ARENA_MAX")
                    .env_remove("GLIBC_TUNABLES");
                match mode {
                    "environment" => {
                        command.env("MALLOC_ARENA_MAX", "2");
                    }
                    "tunable" => {
                        command.env("GLIBC_TUNABLES", "glibc.malloc.arena_max=3");
                    }
                    _ => {}
                }
                let output = command
                    .output()
                    .expect("run isolated allocator policy test");
                assert!(
                    output.status.success(),
                    "{mode}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                println!("allocator policy {mode}: passed");
            }
            return;
        };

        // SAFETY: this subprocess has no test harness and has not started any threads.
        unsafe { configure_host_memory() };
        let start = Arc::new(Barrier::new(33));
        let allocated = Arc::new(Barrier::new(33));
        let release = Arc::new(Barrier::new(33));
        let threads: Vec<_> = (0..32)
            .map(|_| {
                let (start, allocated, release) =
                    (start.clone(), allocated.clone(), release.clone());
                std::thread::spawn(move || {
                    start.wait();
                    let buffers: Vec<_> = (0..256).map(|_| vec![0x5a_u8; 1024]).collect();
                    std::hint::black_box(&buffers);
                    allocated.wait();
                    release.wait();
                    std::hint::black_box(&buffers);
                })
            })
            .collect();
        start.wait();
        allocated.wait();
        let count = arena_count();
        release.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        let limit = match mode.as_str() {
            "default" => 8,
            "environment" => 2,
            "tunable" => 3,
            _ => panic!("unexpected test mode"),
        };
        assert!(
            count > 0 && count <= limit,
            "expected at most {limit} arenas, got {count}"
        );
    }
}
