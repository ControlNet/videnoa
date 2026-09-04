import {
	expression,
	requireJob,
	requireNeeds,
	requireText,
	validateGraph,
} from "./common.mjs";

const releaseVersion = `${String.fromCharCode(36)}{RELEASE_VERSION}`;

export function validateReleaseWorkflow(workflow) {
	const jobs = workflow.jobs;
	validateGraph(jobs, "release");
	requireText(requireJob(jobs, "version-gate"), "version-gate", [
		"crates/controller/Cargo.toml",
		"Version mismatch detected across crates",
	]);
	const quality = requireJob(jobs, "quality-gate");
	requireNeeds(quality, "quality-gate", ["version-gate"]);
	requireText(quality, "quality-gate", [
		"./.github/workflows/unittest.yaml",
		'run_packaging_checks":true',
	]);
	const legacy = {
		"package-linux64": [
			`videnoa-linux64-${expression("needs.version-gate.outputs.version")}.7z`,
			"scripts/package_dist_archive.sh create",
			"scripts/package_dist_archive.sh verify",
			"$HOME/.cargo/registry",
			"actions/upload-artifact@v4",
		],
		"package-win64": [
			`videnoa-win64-${expression("needs.version-gate.outputs.version")}.7z`,
			"actions/upload-artifact@v4",
		],
		"dockerhub-publish": [
			`controlnet/videnoa:${expression("needs.version-gate.outputs.version")}`,
			"controlnet/videnoa:latest",
			"DOCKERHUB_USERNAME",
			"DOCKERHUB_TOKEN",
		],
	};
	for (const [name, contracts] of Object.entries(legacy)) {
		const job = requireJob(jobs, name);
		requireNeeds(job, name, ["version-gate", "quality-gate"]);
		requireText(job, name, contracts);
	}
	const linux = requireJob(jobs, "package-controller-linux");
	requireNeeds(linux, "package-controller-linux", [
		"version-gate",
		"quality-gate",
	]);
	requireText(linux, "package-controller-linux", [
		"scripts/package_controller.sh",
		`videnoa-controller-v${expression("needs.version-gate.outputs.version")}-linux-x86_64.tar.gz`,
		"actions/upload-artifact@v4",
	]);
	const windows = requireJob(jobs, "package-controller-windows");
	requireNeeds(windows, "package-controller-windows", [
		"version-gate",
		"quality-gate",
	]);
	requireText(windows, "package-controller-windows", [
		"scripts/package_controller.ps1",
		`videnoa-controller-v${expression("needs.version-gate.outputs.version")}-windows-x86_64.zip`,
		"actions/upload-artifact@v4",
	]);
	const controllerDocker = requireJob(jobs, "controller-dockerhub-publish");
	requireNeeds(controllerDocker, "controller-dockerhub-publish", [
		"version-gate",
		"quality-gate",
	]);
	requireText(controllerDocker, "controller-dockerhub-publish", [
		"Dockerfile.controller",
		`controlnet/videnoa-controller:${expression("needs.version-gate.outputs.version")}`,
		"controlnet/videnoa-controller:latest",
		"DOCKERHUB_USERNAME",
		"DOCKERHUB_TOKEN",
	]);
	const release = requireJob(jobs, "github-release");
	requireNeeds(release, "github-release", [
		"package-linux64",
		"package-win64",
		"dockerhub-publish",
		"package-controller-linux",
		"package-controller-windows",
		"controller-dockerhub-publish",
	]);
	requireText(release, "github-release", [
		`videnoa-linux64-${expression("env.DIST_VERSION")}.7z*`,
		`videnoa-win64-${expression("env.DIST_VERSION")}.7z*`,
		`videnoa-controller-v${expression("env.DIST_VERSION")}-linux-x86_64.tar.gz`,
		`videnoa-controller-v${expression("env.DIST_VERSION")}-windows-x86_64.zip`,
		'fail_on_unmatched_files":true',
	]);
	const verify = requireJob(jobs, "release-verify");
	requireNeeds(verify, "release-verify", [
		"version-gate",
		"github-release",
		"dockerhub-publish",
		"controller-dockerhub-publish",
	]);
	requireText(verify, "release-verify", [
		"expected_linux_part0",
		"expected_win_part0",
		`videnoa-controller-v${releaseVersion}-linux-x86_64.tar.gz`,
		`videnoa-controller-v${releaseVersion}-windows-x86_64.zip`,
		`docker pull controlnet/videnoa:${releaseVersion}`,
		"docker pull controlnet/videnoa:latest",
		`docker pull controlnet/videnoa-controller:${releaseVersion}`,
		"docker pull controlnet/videnoa-controller:latest",
	]);
}
