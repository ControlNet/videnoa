//! Removes the staged output files a job left behind when the worker stopped
//! while it was running.
//!
//! `VideoOutput` writes into hidden files beside the final output and renames
//! or deletes them when it finishes (see `reserve_temp_beside_output` in
//! `nodes/video_output.rs`). A worker that is killed, or that exits while an
//! executor is still running, never gets there, and an orphaned FFmpeg may
//! still finalize such a file after the worker is gone. On the next start the
//! worker removes, beside each output of a job that was running, exactly the
//! files with that naming scheme for that output and that job:
//! `.<output stem>.<job id>.<6 letters or digits>.videnoa-<direct|video|mux>[.<ext>]`.
//! The job id keeps another job's files with the same output name, including
//! another worker's, out of reach. A relative output path is taken from the
//! working directory, as the job opened it. An encode kept after a failed mux is
//! renamed to the `kept` role and is never removed. Files staged by releases
//! that did not name the job are left alone.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use super::persistence::{capture_dir_identity, RecordedStagedFile, StagedDirIdentity};
use super::Job;

const STAGED_ROLES: &[&str] = &["direct", "video", "mux"];
/// The random part `tempfile` puts in each name.
const RANDOM_LEN: usize = 6;

pub(super) struct SweepReport {
    pub removed: usize,
    pub completed_records: Vec<(String, PathBuf)>,
}

/// Removes staged files recorded in `jobs.db` (`recorded`) as well as any
/// statically discoverable outputs of `jobs` (legacy fallback). Each recorded
/// `(job_id, path)` is included in [`SweepReport::completed_records`] once its
/// cleanup obligation is satisfied without I/O errors and the target volume
/// identity matches, so an unmounted volume (including an empty readable
/// mountpoint directory) or permission failure retains the obligation for the
/// next startup.
pub(super) fn sweep_interrupted_jobs_with_recorded<'a>(
    jobs: impl IntoIterator<Item = &'a Job>,
    recorded: &[RecordedStagedFile],
) -> SweepReport {
    let mut removed_paths = HashSet::new();
    let mut completed_records = Vec::new();

    for record in recorded {
        let owner = record.job_id.as_str();
        let path = record.path.as_path();
        let expected_dir = record.dir_identity;
        let ok = if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| is_staged_filename_for_owner(name, owner))
        {
            sweep_direct_staged_file(path, expected_dir, &mut removed_paths)
        } else {
            sweep_beside_checked(path, owner, expected_dir, &mut removed_paths)
        };
        if ok {
            completed_records.push((record.job_id.clone(), record.path.clone()));
        }
    }

    let outputs: BTreeSet<(PathBuf, &str)> = jobs
        .into_iter()
        .flat_map(|job| {
            output_paths(job)
                .into_iter()
                .map(move |path| (path, job.id.as_str()))
        })
        .collect();
    for (output, owner) in outputs {
        let _ = sweep_beside_checked(&output, owner, None, &mut removed_paths);
    }

    SweepReport {
        removed: removed_paths.len(),
        completed_records,
    }
}

/// The `output_path` of each `VideoOutput` node: its own param, or the value
/// of the `WorkflowInput` port wired into it (the job param, the node param,
/// then the port's default). Paths computed by other nodes are not known here
/// and are not swept.
fn output_paths(job: &Job) -> Vec<PathBuf> {
    let Ok(workflow) = serde_json::to_value(&job.workflow) else {
        return Vec::new();
    };
    // Jobs open a relative output path against the working directory.
    let Ok(cwd) = std::env::current_dir() else {
        return Vec::new();
    };
    output_paths_in(&workflow, job.params.as_ref(), &cwd)
}

fn output_paths_in(
    workflow: &serde_json::Value,
    params: Option<&HashMap<String, serde_json::Value>>,
    cwd: &Path,
) -> Vec<PathBuf> {
    let empty = Vec::new();
    let nodes = workflow["nodes"].as_array().unwrap_or(&empty);
    let node_types: HashMap<&str, &str> = nodes
        .iter()
        .filter_map(|node| Some((node["id"].as_str()?, node["node_type"].as_str()?)))
        .collect();
    let mut paths = Vec::new();
    for node in nodes {
        if node["node_type"] == "VideoOutput" {
            if let Some(path) = node["params"]["output_path"].as_str() {
                paths.push(PathBuf::from(path));
            }
        }
    }
    for connection in workflow["connections"].as_array().unwrap_or(&empty) {
        let target = connection["to_node"].as_str().unwrap_or_default();
        let source = connection["from_node"].as_str().unwrap_or_default();
        if node_types.get(target) != Some(&"VideoOutput")
            || connection["to_port"] != "output_path"
            || node_types.get(source) != Some(&"WorkflowInput")
        {
            continue;
        }
        let port = connection["from_port"].as_str().unwrap_or_default();
        let from_params = params.and_then(|params| params.get(port));
        let input_node = nodes.iter().find(|node| node["id"] == source);
        let from_node = input_node.and_then(|node| node["params"].get(port));
        let from_default = input_node.and_then(|node| {
            node["params"]["ports"]
                .as_array()?
                .iter()
                .find(|declared| declared["name"] == port)?
                .get("default_value")
        });
        if let Some(path) = from_params
            .or(from_node)
            .or(from_default)
            .and_then(|value| value.as_str())
        {
            paths.push(PathBuf::from(path));
        }
    }
    paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| cwd.join(path))
        .collect()
}

fn parent_dir_or_cwd(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn parent_dir_matches_volume(path: &Path, expected: Option<StagedDirIdentity>) -> bool {
    let dir = parent_dir_or_cwd(path);
    match std::fs::read_dir(dir) {
        Ok(_) => dir_identity_matches(dir, expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            debug!(
                path = %path.display(),
                dir = %dir.display(),
                "Parent directory of staged output is not present; retaining cleanup obligation"
            );
            false
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                dir = %dir.display(),
                %error,
                "Failed to verify parent directory of missing staged output"
            );
            false
        }
    }
}

fn dir_identity_matches(dir: &Path, expected: Option<StagedDirIdentity>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(actual) = capture_dir_identity(dir) else {
        debug!(
            dir = %dir.display(),
            "Unable to inspect target directory identity; retaining cleanup obligation"
        );
        return false;
    };
    if actual != expected {
        debug!(
            dir = %dir.display(),
            expected_dev = expected.dev,
            expected_ino = expected.ino,
            actual_dev = actual.dev,
            actual_ino = actual.ino,
            "Target directory identity changed (e.g. unmounted volume); retaining cleanup obligation"
        );
        return false;
    }
    true
}

fn sweep_direct_staged_file(
    path: &Path,
    expected_dir: Option<StagedDirIdentity>,
    removed_paths: &mut HashSet<PathBuf>,
) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return true;
            }
            match std::fs::remove_file(path) {
                Ok(()) => {
                    info!(path = %path.display(), "Removed a staged output left by an interrupted job");
                    removed_paths.insert(path.to_path_buf());
                    true
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    parent_dir_matches_volume(path, expected_dir)
                }
                Err(error) => {
                    warn!(
                        path = %path.display(),
                        %error,
                        "Failed to remove a staged output left by an interrupted job"
                    );
                    false
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            parent_dir_matches_volume(path, expected_dir)
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                %error,
                "Failed to inspect a staged output left by an interrupted job"
            );
            false
        }
    }
}

fn sweep_beside_checked(
    output: &Path,
    owner: &str,
    expected_dir: Option<StagedDirIdentity>,
    removed_paths: &mut HashSet<PathBuf>,
) -> bool {
    let dir = parent_dir_or_cwd(output);
    let stem = output
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            debug!(
                dir = %dir.display(),
                output = %output.display(),
                "Target directory for staged output sweep is not present; retaining recorded obligation if any"
            );
            return false;
        }
        Err(error) => {
            warn!(
                dir = %dir.display(),
                %error,
                "Failed to scan directory for staged outputs of an interrupted job"
            );
            return false;
        }
    };
    if !dir_identity_matches(dir, expected_dir) {
        return false;
    }
    let mut clean = true;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(
                    dir = %dir.display(),
                    %error,
                    "Failed to read directory entry while sweeping staged outputs"
                );
                clean = false;
                continue;
            }
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_staged_file_of(name, &stem, owner) {
            continue;
        }
        let entry_path = entry.path();
        match entry.file_type() {
            Ok(file_type) if file_type.is_file() => {}
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !parent_dir_matches_volume(&entry_path, expected_dir) {
                    clean = false;
                }
                continue;
            }
            Err(error) => {
                warn!(
                    path = %entry_path.display(),
                    %error,
                    "Failed to inspect file type of a staged output"
                );
                clean = false;
                continue;
            }
        }
        match std::fs::remove_file(&entry_path) {
            Ok(()) => {
                info!(path = %entry_path.display(), "Removed a staged output left by an interrupted job");
                removed_paths.insert(entry_path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !parent_dir_matches_volume(&entry_path, expected_dir) {
                    clean = false;
                }
            }
            Err(error) => {
                warn!(
                    path = %entry_path.display(),
                    %error,
                    "Failed to remove a staged output left by an interrupted job"
                );
                clean = false;
            }
        }
    }
    clean && dir_identity_matches(dir, expected_dir)
}

/// Whether `name` is `.<any stem>.<owner>.<random>.videnoa-<role>[.<ext>]`.
fn is_staged_filename_for_owner(name: &str, owner: &str) -> bool {
    if owner.is_empty() {
        return false;
    }
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    let Some((before_marker, role_and_ext)) = rest.rsplit_once(".videnoa-") else {
        return false;
    };
    let (role, extension) = match role_and_ext.split_once('.') {
        Some((role, extension)) => (role, Some(extension)),
        None => (role_and_ext, None),
    };
    if !STAGED_ROLES.contains(&role)
        || !extension.is_none_or(|ext| !ext.is_empty() && !ext.contains('.'))
    {
        return false;
    }
    let Some(random_start) = before_marker.len().checked_sub(RANDOM_LEN) else {
        return false;
    };
    let Some((before_random, random)) = before_marker.split_at_checked(random_start) else {
        return false;
    };
    if !random.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return false;
    }
    let Some(before_dot) = before_random.strip_suffix('.') else {
        return false;
    };
    let Some(before_owner) = before_dot.strip_suffix(owner) else {
        return false;
    };
    before_owner.ends_with('.')
}

/// Whether `name` is `.<stem>.<owner>.<random>.videnoa-<role>[.<ext>]`.
fn is_staged_file_of(name: &str, stem: &str, owner: &str) -> bool {
    let Some(rest) = name
        .strip_prefix('.')
        .and_then(|rest| rest.strip_prefix(stem))
        .and_then(|rest| rest.strip_prefix('.'))
        .and_then(|rest| rest.strip_prefix(owner))
        .and_then(|rest| rest.strip_prefix('.'))
    else {
        return false;
    };
    let Some((random, rest)) = rest.split_at_checked(RANDOM_LEN) else {
        return false;
    };
    if !random.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return false;
    }
    let Some(rest) = rest.strip_prefix(".videnoa-") else {
        return false;
    };
    let (role, extension) = match rest.split_once('.') {
        Some((role, extension)) => (role, Some(extension)),
        None => (rest, None),
    };
    STAGED_ROLES.contains(&role)
        && extension.is_none_or(|ext| !ext.is_empty() && !ext.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn output_paths_cover_relative_paths_and_input_defaults() {
        let workflow = serde_json::json!({
            "nodes": [
                {"id": "literal", "node_type": "VideoOutput", "params": {"output_path": "out/a.mkv"}},
                {"id": "absolute", "node_type": "VideoOutput", "params": {"output_path": "/media/b.mkv"}},
                {"id": "wired", "node_type": "VideoOutput", "params": {}},
                {"id": "input", "node_type": "WorkflowInput", "params": {"ports": [
                    {"name": "target", "port_type": "Path", "default_value": "c.mkv"}
                ]}}
            ],
            "connections": [
                {"from_node": "input", "from_port": "target", "to_node": "wired",
                 "to_port": "output_path", "port_type": "Path"}
            ]
        });
        let cwd = Path::new("/srv/worker");

        let paths = output_paths_in(&workflow, None, cwd);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/srv/worker/out/a.mkv"),
                PathBuf::from("/media/b.mkv"),
                PathBuf::from("/srv/worker/c.mkv"),
            ]
        );

        let params = HashMap::from([("target".to_string(), serde_json::json!("/media/d.mkv"))]);
        let paths = output_paths_in(&workflow, Some(&params), cwd);
        assert_eq!(paths[2], PathBuf::from("/media/d.mkv"));
    }

    #[test]
    fn only_this_jobs_staged_names_for_this_output_match() {
        for name in [
            ".Episode 01.job-1.a1B2c3.videnoa-video.mkv",
            ".Episode 01.job-1.Zz9900.videnoa-mux.mkv",
            ".Episode 01.job-1.abcdef.videnoa-direct.mp4",
            ".Episode 01.job-1.abcdef.videnoa-direct",
        ] {
            assert!(is_staged_file_of(name, "Episode 01", "job-1"), "{name}");
        }
        for name in [
            "Episode 01.mkv",
            ".Episode 02.job-1.a1B2c3.videnoa-video.mkv",
            ".Episode 01.job-2.a1B2c3.videnoa-video.mkv",
            ".Episode 01.a1B2c3.videnoa-video.mkv",
            ".Episode 01.job-1.a1B2c3.videnoa-kept.mkv",
            ".Episode 01.job-1.a1B2c.videnoa-video.mkv",
            ".Episode 01.job-1.a1B2c3d.videnoa-video.mkv",
            ".Episode 01.job-1.a1-2c3.videnoa-video.mkv",
            ".Episode 01.job-1.a1B2c3.videnoa-audio.mkv",
            ".Episode 01.job-1.a1B2c3.videnoa-video.mkv.part",
            ".Episode 01.job-1.a1B2c3.videnoa-video.",
            "Episode 01.job-1.a1B2c3.videnoa-video.mkv",
        ] {
            assert!(!is_staged_file_of(name, "Episode 01", "job-1"), "{name}");
        }
    }

    #[test]
    fn owner_filename_check_handles_dotted_and_empty_stems_without_touching_kept() {
        for name in [
            ".Episode 01.job-1.a1B2c3.videnoa-video.mkv",
            ".Show.S01E01.1080p.job-1.Zz9900.videnoa-mux.mkv",
            "..job-1.abcdef.videnoa-direct",
        ] {
            assert!(is_staged_filename_for_owner(name, "job-1"), "{name}");
        }
        for name in [
            "Episode 01.mkv",
            ".Episode 01.job-2.a1B2c3.videnoa-video.mkv",
            ".Episode 01.job-1.a1B2c3.videnoa-kept.mkv",
            ".Episode 01.a1B2c3.videnoa-video.mkv",
            "Episode 01.job-1.a1B2c3.videnoa-video.mkv",
        ] {
            assert!(!is_staged_filename_for_owner(name, "job-1"), "{name}");
        }
    }

    #[test]
    fn multibyte_utf8_names_never_panic_and_valid_unicode_stems_still_sweep() {
        // Every case below places `before_marker.len() - RANDOM_LEN` inside a
        // 2-byte, 3-byte, or 4-byte UTF-8 scalar so an unchecked `split_at`
        // would panic.
        for name in [
            // 2-byte UTF-8 (2-byte scalar): offsets +1 inside 0..2 and +1 inside 1..3
            ".éabcde.videnoa-video.mkv",
            ".aéabcde.videnoa-direct.mp4",
            // 3-byte UTF-8 CJK: offsets +1 and +2 inside 3-byte scalars
            ".第1话.videnoa-video.mkv",
            ".中文名ab.videnoa-mux.mkv",
            ".中文名.videnoa-video",
            // 4-byte UTF-8 emoji: offsets +1, +2, +3 inside 0..4
            ".🎬abc.videnoa-video.mkv",
            ".🎬🎬.videnoa-mux.mkv",
            ".🎬abcde.videnoa-direct.mp4",
        ] {
            assert!(!is_staged_filename_for_owner(name, "job-1"), "{name}");
        }

        // Multi-byte UTF-8 in the 6-byte random slot of `is_staged_file_of`
        // (byte index 6 falls inside the second 3-byte CJK scalar at bytes 5..8).
        assert!(!is_staged_file_of(
            ".番剧第01话.job-1.ab中文.videnoa-video.mkv",
            "番剧第01话",
            "job-1"
        ));

        // Valid Unicode stems with an owner and 6-char ASCII alphanumeric random suffix.
        for (stem, name) in [
            ("番剧第01话", ".番剧第01话.job-1.a1B2c3.videnoa-video.mkv"),
            (
                "Épisode 01 🎬",
                ".Épisode 01 🎬.job-1.Zz9900.videnoa-mux.mkv",
            ),
        ] {
            assert!(is_staged_filename_for_owner(name, "job-1"), "{name}");
            assert!(is_staged_file_of(name, stem, "job-1"), "{name}");
        }

        let dir = tempfile::TempDir::new().unwrap();
        let hidden_unicode_target = dir.path().join(".第1话.videnoa-video.mkv");
        let hidden_emoji_target = dir.path().join(".🎬🎬.videnoa-mux.mkv");
        let unicode_output = dir.path().join("番剧第01话.mkv");
        let unicode_staged_direct = dir
            .path()
            .join(".番剧第01话.job-1.a1B2c3.videnoa-video.mkv");
        let unicode_staged_beside = dir.path().join(".番剧第01话.job-1.Zz9900.videnoa-mux.mkv");

        for path in [
            &hidden_unicode_target,
            &hidden_emoji_target,
            &unicode_staged_direct,
            &unicode_staged_beside,
        ] {
            std::fs::write(path, b"fixture").unwrap();
        }

        let recorded = vec![
            RecordedStagedFile {
                job_id: "job-1".to_string(),
                path: hidden_unicode_target.clone(),
                dir_identity: super::super::persistence::capture_parent_dir_identity(
                    &hidden_unicode_target,
                ),
            },
            RecordedStagedFile {
                job_id: "job-1".to_string(),
                path: hidden_emoji_target.clone(),
                dir_identity: super::super::persistence::capture_parent_dir_identity(
                    &hidden_emoji_target,
                ),
            },
            RecordedStagedFile {
                job_id: "job-1".to_string(),
                path: unicode_output.clone(),
                dir_identity: super::super::persistence::capture_parent_dir_identity(
                    &unicode_output,
                ),
            },
            RecordedStagedFile {
                job_id: "job-1".to_string(),
                path: unicode_staged_direct.clone(),
                dir_identity: super::super::persistence::capture_parent_dir_identity(
                    &unicode_staged_direct,
                ),
            },
        ];
        let expected_completed: Vec<(String, PathBuf)> = recorded
            .iter()
            .map(|r| (r.job_id.clone(), r.path.clone()))
            .collect();
        let report = sweep_interrupted_jobs_with_recorded(std::iter::empty(), &recorded);

        assert!(
            hidden_unicode_target.exists(),
            "non-staged hidden Unicode target must not be deleted"
        );
        assert!(
            hidden_emoji_target.exists(),
            "non-staged hidden emoji target must not be deleted"
        );
        assert!(!unicode_staged_direct.exists());
        assert!(!unicode_staged_beside.exists());
        assert_eq!(report.removed, 2);
        assert_eq!(report.completed_records, expected_completed);
    }

    #[test]
    fn missing_parent_directory_retains_recorded_obligations_until_volume_is_online() {
        let root = tempfile::TempDir::new().unwrap();
        let offline_volume = root.path().join("mnt-nas").join("anime-season-01");
        let target_output = offline_volume.join("Episode 01.mkv");
        let direct_staged = offline_volume.join(".Episode 01.job-42.a1B2c3.videnoa-video.mkv");
        let beside_staged = offline_volume.join(".Episode 01.job-42.Zz9900.videnoa-mux.mkv");
        let kept_encode = offline_volume.join(".Episode 01.job-42.K3pt00.videnoa-kept.mkv");

        let recorded = vec![
            RecordedStagedFile {
                job_id: "job-42".to_string(),
                path: direct_staged.clone(),
                dir_identity: None,
            },
            RecordedStagedFile {
                job_id: "job-42".to_string(),
                path: target_output.clone(),
                dir_identity: None,
            },
        ];
        let expected_completed: Vec<(String, PathBuf)> = recorded
            .iter()
            .map(|r| (r.job_id.clone(), r.path.clone()))
            .collect();

        // While the volume directory does not exist (`NotFound`), neither the
        // direct staged path nor the target output path may be marked completed.
        let offline_report = sweep_interrupted_jobs_with_recorded(std::iter::empty(), &recorded);
        assert_eq!(offline_report.removed, 0);
        assert!(
            offline_report.completed_records.is_empty(),
            "expected obligations to be retained while parent dir is missing, got {:?}",
            offline_report.completed_records
        );

        // Once the volume comes online, both retained obligations sweep the
        // leftover staged files (preserving `kept`) and enter `completed_records`.
        std::fs::create_dir_all(&offline_volume).unwrap();
        for path in [&direct_staged, &beside_staged, &kept_encode] {
            std::fs::write(path, b"staged bytes").unwrap();
        }

        let online_report = sweep_interrupted_jobs_with_recorded(std::iter::empty(), &recorded);
        assert!(!direct_staged.exists());
        assert!(!beside_staged.exists());
        assert!(kept_encode.exists());
        assert_eq!(online_report.removed, 2);
        assert_eq!(online_report.completed_records, expected_completed);

        // If a recorded staged file is already absent inside an existing
        // accessible directory, its obligation is still completed cleanly.
        let already_absent_report =
            sweep_interrupted_jobs_with_recorded(std::iter::empty(), &recorded);
        assert_eq!(already_absent_report.removed, 0);
        assert_eq!(already_absent_report.completed_records, expected_completed);
    }

    #[cfg(unix)]
    #[test]
    fn unmounted_readable_mountpoint_retains_recorded_obligations_until_original_volume_returns() {
        let root = tempfile::TempDir::new().unwrap();
        let mountpoint = root.path().join("media");
        let detached_volume = root.path().join("media-detached");
        std::fs::create_dir_all(&mountpoint).unwrap();

        // Given: staged output placed directly at the mountpoint root, with its
        // mounted directory identity captured when the job recorded the paths.
        let target_output = mountpoint.join("output.mkv");
        let direct_staged = mountpoint.join(".output.job-99.a1B2c3.videnoa-video.mkv");
        let beside_staged = mountpoint.join(".output.job-99.Zz9900.videnoa-mux.mkv");
        std::fs::write(&direct_staged, b"video bytes").unwrap();
        std::fs::write(&beside_staged, b"mux bytes").unwrap();
        let mounted_identity = capture_dir_identity(&mountpoint).expect("unix dir identity");

        let recorded = vec![
            RecordedStagedFile {
                job_id: "job-99".to_string(),
                path: direct_staged.clone(),
                dir_identity: Some(mounted_identity),
            },
            RecordedStagedFile {
                job_id: "job-99".to_string(),
                path: target_output.clone(),
                dir_identity: Some(mounted_identity),
            },
        ];

        // Simulate unmounting: move the mounted volume aside and create an empty,
        // readable underlying mountpoint directory at the same path.
        std::fs::rename(&mountpoint, &detached_volume).unwrap();
        std::fs::create_dir_all(&mountpoint).unwrap();
        assert_ne!(capture_dir_identity(&mountpoint), Some(mounted_identity));

        // When: startup sweep runs against the readable empty mountpoint.
        let unmounted_report = sweep_interrupted_jobs_with_recorded(std::iter::empty(), &recorded);

        // Then: nothing was removed and NEITHER database obligation is completed.
        assert_eq!(unmounted_report.removed, 0);
        assert!(
            unmounted_report.completed_records.is_empty(),
            "readable empty mountpoint must not clear recorded obligations: {:?}",
            unmounted_report.completed_records
        );
        assert!(detached_volume
            .join(".output.job-99.a1B2c3.videnoa-video.mkv")
            .exists());

        // When: the original volume is mounted back at `mountpoint` and startup sweeps again.
        std::fs::remove_dir(&mountpoint).unwrap();
        std::fs::rename(&detached_volume, &mountpoint).unwrap();
        assert_eq!(capture_dir_identity(&mountpoint), Some(mounted_identity));

        let remounted_report = sweep_interrupted_jobs_with_recorded(std::iter::empty(), &recorded);
        assert_eq!(remounted_report.removed, 2);
        assert!(!direct_staged.exists());
        assert!(!beside_staged.exists());
        assert_eq!(
            remounted_report.completed_records,
            vec![
                ("job-99".to_string(), direct_staged),
                ("job-99".to_string(), target_output),
            ]
        );
    }
}
