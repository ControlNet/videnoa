use std::env;
#[cfg(windows)]
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use tracing::{info, warn};

#[cfg(unix)]
const ORT_LIB_NAME: &str = "libonnxruntime.so";
#[cfg(windows)]
const ORT_LIB_NAME: &str = "onnxruntime.dll";

/// Search directories relative to the current executable for runtime libraries.
///
/// Probes these locations in order:
///   1. `<exe_dir>/` (Windows only)
///   2. `<exe_dir>/lib/`
///   3. `<exe_dir>/../lib/`
///   4. `<cwd>/lib/`
///   5. `/usr/local/lib/` (Unix only)
///   6. `/usr/lib/` (Unix only)
fn candidate_lib_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = env::current_exe().and_then(|p| p.canonicalize()) {
        if let Some(exe_dir) = exe.parent() {
            #[cfg(windows)]
            {
                dirs.push(exe_dir.to_path_buf());
            }
            dirs.push(exe_dir.join("lib"));
            if let Some(parent) = exe_dir.parent() {
                dirs.push(parent.join("lib"));
            }
        }
    }
    if let Ok(cwd) = env::current_dir() {
        let cwd_lib = cwd.join("lib");
        if !dirs.contains(&cwd_lib) {
            dirs.push(cwd_lib);
        }
    }
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/usr/local/lib"));
        dirs.push(PathBuf::from("/usr/lib"));
    }
    dirs
}

fn candidate_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = env::current_exe().and_then(|p| p.canonicalize()) {
        if let Some(exe_dir) = exe.parent() {
            dirs.push(exe_dir.to_path_buf());
            dirs.push(exe_dir.join("bin"));
            if let Some(parent) = exe_dir.parent() {
                dirs.push(parent.join("bin"));
            }
        }
    }

    if let Ok(cwd) = env::current_dir() {
        if !dirs.contains(&cwd) {
            dirs.push(cwd.clone());
        }
        let cwd_bin = cwd.join("bin");
        if !dirs.contains(&cwd_bin) {
            dirs.push(cwd_bin);
        }
    }

    dirs
}

#[cfg(unix)]
fn candidate_binary_names(binary: &str) -> Vec<String> {
    vec![binary.to_string()]
}

#[cfg(windows)]
fn candidate_binary_names(binary: &str) -> Vec<String> {
    if Path::new(binary).components().count() > 1 {
        return vec![binary.to_string()];
    }

    let lower = binary.to_ascii_lowercase();
    if lower.ends_with(".exe") || lower.ends_with(".cmd") || lower.ends_with(".bat") {
        return vec![binary.to_string()];
    }

    vec![
        format!("{binary}.exe"),
        format!("{binary}.cmd"),
        format!("{binary}.bat"),
        binary.to_string(),
    ]
}

fn find_binary_in_dirs(binary: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let names = candidate_binary_names(binary);
    for dir in dirs {
        for name in &names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn command_for(binary: &str) -> ProcessCommand {
    if let Some(path) = find_binary_in_dirs(binary, &candidate_bin_dirs()) {
        return ProcessCommand::new(path);
    }
    ProcessCommand::new(binary)
}

fn find_ort_dylib_in_dirs(dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(ORT_LIB_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn normalize_windows_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn build_path_with_prepended_dirs(current: Option<OsString>, dirs: &[PathBuf]) -> OsString {
    use std::collections::HashSet;

    let fallback = current.clone().unwrap_or_default();
    let mut merged = Vec::new();
    let mut seen = HashSet::<String>::new();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let key = normalize_windows_path_key(dir);
        if seen.insert(key) {
            merged.push(dir.clone());
        }
    }

    if let Some(path) = current {
        for dir in env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let key = normalize_windows_path_key(&dir);
            if seen.insert(key) {
                merged.push(dir);
            }
        }
    }

    env::join_paths(merged).unwrap_or(fallback)
}

#[cfg(windows)]
fn prepend_candidate_dirs_to_path(dirs: &[PathBuf]) {
    let merged = build_path_with_prepended_dirs(env::var_os("PATH"), dirs);
    env::set_var("PATH", merged);
}

/// Return a load-priority tier for known GPU runtime libs, or `None` for
/// anything we should NOT preload (ORT providers, unrelated system libs).
///
/// ORT providers are NOT preloaded because they depend on symbols exported
/// by `libonnxruntime.so`, which is loaded later by the ORT crate itself.
///
///   0 — CUDA runtime (libcudart, libcublas, libcublasLt, libcufft, libcurand)
///   1 — cuDNN (libcudnn*)
///   2 — TensorRT (libnvinfer*, libnvonnxparser*)
#[cfg(unix)]
fn load_priority(name: &str) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    if name.starts_with("libcudart")
        || name.starts_with("libcublaslt")
        || name.starts_with("libcublas")
        || name.starts_with("libcufft")
        || name.starts_with("libcurand")
    {
        Some(0)
    } else if name.starts_with("libcudnn") {
        Some(1)
    } else if name.starts_with("libnvinfer") || name.starts_with("libnvonnxparser") {
        Some(2)
    } else {
        None
    }
}

#[cfg(windows)]
fn load_priority(name: &str) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    if name.starts_with("cudart64_")
        || name.starts_with("cublas64_")
        || name.starts_with("cublaslt64_")
    {
        Some(0)
    } else if name.starts_with("cudnn64_") {
        Some(1)
    } else if name.starts_with("nvinfer") || name.starts_with("nvonnxparser") {
        Some(2)
    } else {
        None
    }
}

/// Pre-load `.so`/`.dll` files via RTLD_GLOBAL or LoadLibrary so that
/// subsequent library loads by ORT find them in the process address space.
///
/// glibc caches `LD_LIBRARY_PATH` at startup, so `env::set_var` after
/// `main()` begins has no effect on dlopen search paths. This bypasses
/// that by loading each lib with its absolute path directly.
///
/// Libraries are loaded in dependency order (CUDA → cuDNN → TRT) to
/// ensure transitive dependencies are already in the process address
/// space when higher-level libs load.
///
/// Deduplicates by file name — the first directory in `dirs` that contains
/// a given `.so` wins, so caller ordering matters.
fn preload_libs_from_dirs(dirs: &[PathBuf]) {
    use std::collections::HashSet;

    let mut seen_names: HashSet<String> = HashSet::new();
    let mut libs: Vec<(u8, String, PathBuf)> = Vec::new();

    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            if !is_candidate_lib(&name, &path) {
                continue;
            }

            if !seen_names.insert(name.clone()) {
                continue;
            }

            let priority = match load_priority(&name) {
                Some(p) => p,
                None => continue,
            };
            libs.push((priority, name, path));
        }
    }

    // Sort by (priority, name) so dependencies load before dependents.
    libs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    for (_, _, lib_path) in &libs {
        unsafe { load_library(lib_path) };
    }
}

#[cfg(unix)]
fn is_candidate_lib(name: &str, path: &Path) -> bool {
    name.contains(".so") && !path.is_symlink()
}

#[cfg(windows)]
fn is_candidate_lib(_name: &str, path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
}

#[cfg(unix)]
unsafe fn load_library(path: &Path) {
    if let Ok(lib) =
        libloading::os::unix::Library::open(Some(path), libc::RTLD_LAZY | libc::RTLD_GLOBAL)
    {
        std::mem::forget(lib);
    }
}

#[cfg(windows)]
unsafe fn load_library(path: &Path) {
    if let Ok(lib) = libloading::Library::new(path) {
        std::mem::forget(lib);
    }
}

/// Auto-detect and configure runtime library paths before ORT initialization.
///
/// Call this at the very start of `main()`, before any ORT or tracing init.
pub fn setup_runtime_libs() {
    let dirs = candidate_lib_dirs();

    if env::var_os("ORT_DYLIB_PATH").is_none() {
        if let Some(path) = find_ort_dylib_in_dirs(&dirs) {
            env::set_var("ORT_DYLIB_PATH", &path);
        }

        #[cfg(windows)]
        prepend_candidate_dirs_to_path(&dirs);
    }

    preload_libs_from_dirs(&dirs);
}

/// Log which runtime libraries were resolved, for diagnostics.
/// Call after tracing is initialized.
pub fn log_runtime_lib_status() {
    if let Ok(ort) = env::var("ORT_DYLIB_PATH") {
        let exists = Path::new(&ort).is_file();
        if exists {
            info!("ORT library: {ort}");
        } else {
            warn!("ORT_DYLIB_PATH set to {ort} but file not found");
        }
    } else {
        warn!("ORT_DYLIB_PATH not set — ORT will try default search paths");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn candidate_lib_dirs_contains_system_paths() {
        let dirs = candidate_lib_dirs();
        assert!(dirs.contains(&PathBuf::from("/usr/local/lib")));
        assert!(dirs.contains(&PathBuf::from("/usr/lib")));
    }

    #[test]
    fn candidate_lib_dirs_includes_cwd_lib() {
        let dirs = candidate_lib_dirs();
        if let Ok(cwd) = env::current_dir() {
            assert!(dirs.contains(&cwd.join("lib")));
        }
    }

    #[test]
    fn find_ort_dylib_in_dirs_does_not_panic() {
        let dirs = candidate_lib_dirs();
        let _ = find_ort_dylib_in_dirs(&dirs);
    }

    #[test]
    fn candidate_bin_dirs_includes_cwd_bin() {
        let dirs = candidate_bin_dirs();
        if let Ok(cwd) = env::current_dir() {
            assert!(dirs.contains(&cwd.join("bin")));
        }
    }

    #[test]
    fn find_binary_in_dirs_prefers_first_match() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).expect("first dir should be created");
        std::fs::create_dir_all(&second).expect("second dir should be created");

        #[cfg(unix)]
        let binary_name = "ffprobe";
        #[cfg(windows)]
        let binary_name = "ffprobe.exe";

        std::fs::write(first.join(binary_name), b"first").expect("first binary should exist");
        std::fs::write(second.join(binary_name), b"second").expect("second binary should exist");

        let resolved = find_binary_in_dirs("ffprobe", &[first.clone(), second.clone()])
            .expect("binary should be resolved");
        assert_eq!(resolved, first.join(binary_name));
    }

    #[cfg(unix)]
    #[test]
    fn load_priority_orders_cuda_before_cudnn_before_trt() {
        assert!(load_priority("libcudart.so.12") < load_priority("libcudnn.so.9"));
        assert!(load_priority("libcublas.so.12") < load_priority("libcudnn_ops.so.9"));
        assert!(load_priority("libcudnn.so.9") < load_priority("libnvinfer.so.10"));
    }

    #[cfg(windows)]
    #[test]
    fn load_priority_orders_cuda_before_cudnn_before_trt() {
        assert!(load_priority("cudart64_12.dll") < load_priority("cudnn64_9.dll"));
        assert!(load_priority("cublas64_12.dll") < load_priority("cudnn64_9.dll"));
        assert!(load_priority("cudnn64_9.dll") < load_priority("nvinfer.dll"));
    }

    #[cfg(unix)]
    #[test]
    fn load_priority_excludes_ort_and_unknown_libs() {
        assert_eq!(load_priority("libonnxruntime.so.1.23.2"), None);
        assert_eq!(load_priority("libonnxruntime_providers_cuda.so"), None);
        assert_eq!(load_priority("libsomething_else.so"), None);
    }

    #[cfg(windows)]
    #[test]
    fn load_priority_excludes_ort_and_unknown_libs() {
        assert_eq!(load_priority("onnxruntime.dll"), None);
        assert_eq!(load_priority("onnxruntime_providers_cuda.dll"), None);
        assert_eq!(load_priority("something_else.dll"), None);
    }

    #[cfg(windows)]
    #[test]
    fn build_path_with_prepended_dirs_prefers_candidate_dirs() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let preferred_lib = temp.path().join("preferred-lib");
        let preferred_bin = temp.path().join("preferred-bin");
        let existing = temp.path().join("existing");

        std::fs::create_dir_all(&preferred_lib).expect("preferred lib dir should be created");
        std::fs::create_dir_all(&preferred_bin).expect("preferred bin dir should be created");
        std::fs::create_dir_all(&existing).expect("existing dir should be created");

        let current =
            env::join_paths([existing.clone(), preferred_lib.clone()]).expect("path should join");
        let merged = build_path_with_prepended_dirs(
            Some(current),
            &[preferred_lib.clone(), preferred_bin.clone()],
        );
        let dirs: Vec<PathBuf> = env::split_paths(&merged).collect();

        assert_eq!(dirs.first(), Some(&preferred_lib));
        assert_eq!(dirs.get(1), Some(&preferred_bin));
        assert_eq!(dirs.get(2), Some(&existing));
        assert_eq!(
            dirs.iter().filter(|dir| *dir == &preferred_lib).count(),
            1,
            "preferred lib should not be duplicated"
        );
    }
}
