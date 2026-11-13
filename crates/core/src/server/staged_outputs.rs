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

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use super::Job;

const STAGED_ROLES: &[&str] = &["direct", "video", "mux"];
/// The random part `tempfile` puts in each name.
const RANDOM_LEN: usize = 6;

/// Removes staged files of the given jobs' outputs; returns how many.
pub(super) fn sweep_interrupted_jobs<'a>(jobs: impl IntoIterator<Item = &'a Job>) -> usize {
    let outputs: BTreeSet<(PathBuf, &str)> = jobs
        .into_iter()
        .flat_map(|job| {
            output_paths(job)
                .into_iter()
                .map(move |path| (path, job.id.as_str()))
        })
        .collect();
    let mut removed = 0;
    for (output, owner) in outputs {
        removed += sweep_beside(&output, owner);
    }
    removed
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

fn sweep_beside(output: &Path, owner: &str) -> usize {
    let (Some(dir), Some(stem)) = (output.parent(), output.file_stem()) else {
        return 0;
    };
    let Some(stem) = stem.to_str() else {
        return 0;
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // A folder that is gone or unreachable has nothing to clean now.
        Err(_) => return 0,
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_staged_file_of(name, stem, owner) || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => {
                info!(path = %entry.path().display(), "Removed a staged output left by an interrupted job");
                removed += 1;
            }
            Err(error) => warn!(
                path = %entry.path().display(),
                %error,
                "Failed to remove a staged output left by an interrupted job"
            ),
        }
    }
    removed
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
}
