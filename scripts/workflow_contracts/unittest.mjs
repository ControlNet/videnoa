import {
	requireJob,
	requireNeeds,
	requireText,
	validateGraph,
} from "./common.mjs";

export function validateUnitWorkflow(workflow) {
	const jobs = workflow.jobs;
	validateGraph(jobs, "unittest");
	const legacy = {
		"rust-tests": [
			"cargo test -p videnoa-core --lib --tests",
			"cargo test -p videnoa-app --lib --tests",
		],
		"web-build-check": [
			'working-directory":"web',
			"npm ci --no-fund",
			"npm run build",
		],
		"package-linux64-smoke": [
			"scripts/package_dist.sh",
			"scripts/check_linux_package_compat.sh",
			"scripts/package_dist_archive.sh create",
			"scripts/package_dist_archive.sh verify",
			"scripts/tests/package_dist_archive_test.sh",
			"$HOME/.cargo/registry",
			"videnoa-linux64-smoke.7z",
		],
		"package-win64-smoke": [
			"scripts/package_dist.ps1",
			"videnoa-win64-smoke.7z",
		],
		"docker-build-smoke": [
			"docker build -t videnoa-ci-smoke .",
			"videnoa --help",
		],
	};
	for (const [name, contracts] of Object.entries(legacy))
		requireText(requireJob(jobs, name), name, contracts);
	for (const name of [
		"package-linux64-smoke",
		"package-win64-smoke",
		"docker-build-smoke",
	]) {
		requireNeeds(jobs[name], name, ["rust-tests", "web-build-check"]);
	}
	requireText(requireJob(jobs, "workflow-contracts"), "workflow-contracts", [
		"npm ci --no-fund",
		"validate_ci_release_workflows.test.mjs",
	]);
	requireText(requireJob(jobs, "controller-rust"), "controller-rust", [
		"cargo fmt --all -- --check",
		"cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings",
		"cargo test -p videnoa-controller --all-targets",
	]);
	const faultLoad = requireJob(jobs, "controller-fault-load");
	requireNeeds(faultLoad, "controller-fault-load", ["controller-rust"]);
	requireText(faultLoad, "controller-fault-load", [
		"--test task20",
		"--test task21",
		"--test task21_concurrency",
		"--test task21_filesystem",
		"--test task21_resources",
		"--test task21_security",
	]);
	requireText(requireJob(jobs, "controller-web"), "controller-web", [
		"npm ci --no-fund",
		"npm run lint",
		"npm test -- --run",
		"npm run build",
		"playwright install --with-deps chromium",
		"npm run test:e2e",
	]);
	const linux = requireJob(jobs, "controller-package-linux-smoke");
	requireNeeds(linux, "controller-package-linux-smoke", [
		"controller-rust",
		"controller-web",
	]);
	requireText(linux, "controller-package-linux-smoke", [
		"scripts/package_controller.sh",
		"package_controller_test.sh",
		"controller_archive_root_files_test.sh",
	]);
	const windows = requireJob(jobs, "controller-package-windows-smoke");
	requireNeeds(windows, "controller-package-windows-smoke", [
		"controller-rust",
		"controller-web",
	]);
	requireText(windows, "controller-package-windows-smoke", [
		"scripts/package_controller.ps1",
		`videnoa-controller-v${String.fromCharCode(36)}{version}-windows-x86_64.zip`,
	]);
	const image = requireJob(jobs, "controller-docker-smoke");
	requireNeeds(image, "controller-docker-smoke", [
		"controller-rust",
		"controller-web",
	]);
	requireText(image, "controller-docker-smoke", [
		"docker build -f Dockerfile.controller",
		"scripts/check_controller_container.sh videnoa-controller:ci --all",
	]);
}
