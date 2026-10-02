#!/usr/bin/env node

import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
	loadWorkflow,
	validateReleaseWorkflow,
	validateRepositoryWorkflows,
	validateUnitWorkflow,
	WorkflowContractError,
} from "../validate_ci_release_workflows.mjs";

const repoRoot = path.resolve(
	path.dirname(fileURLToPath(import.meta.url)),
	"../..",
);
const unitPath = path.join(repoRoot, ".github/workflows/unittest.yaml");
const releasePath = path.join(repoRoot, ".github/workflows/release.yaml");

function expectContractFailure(name, callback, expected) {
	assert.throws(callback, (error) => {
		assert.ok(
			error instanceof WorkflowContractError,
			`${name}: unexpected error type`,
		);
		assert.match(
			error.message,
			expected,
			`${name}: unexpected failure message`,
		);
		return true;
	});
	console.log(`[workflow-contracts][negative] ${name}: PASS`);
}

validateRepositoryWorkflows(repoRoot);
console.log("[workflow-contracts][positive] complete CI/release matrix: PASS");

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["controller-fault-load"].steps = workflow.jobs[
		"controller-fault-load"
	].steps.filter(
		(step) => step.name !== "Run production Argon2 executor contention regression",
	);
	expectContractFailure(
		"production Argon2 contention coverage omitted",
		() => validateUnitWorkflow(workflow),
		/auth_contention/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	delete workflow.jobs["docker-build-smoke"];
	expectContractFailure(
		"existing package break",
		() => validateUnitWorkflow(workflow),
		/missing required job: docker-build-smoke/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	const buildStep = workflow.jobs["controller-docker-smoke"].steps.find(
		(step) => step.name === "Build Controller image",
	);
	assert.ok(buildStep, "controller-docker-smoke: missing image build step");
	buildStep.run = "docker build -t videnoa-controller:ci .";
	expectContractFailure(
		"missing Controller Dockerfile",
		() => validateUnitWorkflow(workflow),
		/Dockerfile.controller/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["controller-rust"].steps = workflow.jobs[
		"controller-rust"
	].steps.filter((step) => step.name !== "Build Controller web assets");
	expectContractFailure(
		"Controller Rust assets omitted",
		() => validateUnitWorkflow(workflow),
		/npm run build/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(releasePath));
	workflow.jobs["version-gate"].steps[1].run = workflow.jobs[
		"version-gate"
	].steps[1].run.replace(
		'"controller": Path("crates/controller/Cargo.toml"),',
		"",
	);
	expectContractFailure(
		"Controller version mismatch gate removed",
		() => validateReleaseWorkflow(workflow),
		/crates\/controller\/Cargo.toml/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(releasePath));
	workflow.jobs["github-release"].steps.at(-1).with.files = workflow.jobs[
		"github-release"
	].steps
		.at(-1)
		.with.files.replace(/^.*videnoa-controller.*linux.*\n/m, "");
	expectContractFailure(
		"missing Controller archive",
		() => validateReleaseWorkflow(workflow),
		/linux-x86_64\.tar\.gz/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(releasePath));
	workflow.jobs["controller-dockerhub-publish"].steps.at(-1).with.tags =
		"controlnet/videnoa-controller:latest";
	expectContractFailure(
		"missing Controller version tag",
		() => validateReleaseWorkflow(workflow),
		/videnoa-controller:\$\{\{/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["controller-package-linux-smoke"].steps[4].run = "true";
	expectContractFailure(
		"forbidden GPU content check removed",
		() => validateUnitWorkflow(workflow),
		/package_controller_test\.sh/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	const archiveStep = workflow.jobs["package-linux64-smoke"].steps.find(
		(step) => step.name === "Create split archive (2000MB volumes)",
	);
	archiveStep.run = archiveStep.run.replace(
		"scripts/package_dist_archive.sh create",
		"7z a -t7z -v2000m",
	);
	expectContractFailure(
		"legacy Linux archive helper bypassed",
		() => validateUnitWorkflow(workflow),
		/scripts\/package_dist_archive\.sh create/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(releasePath));
	const verifyStep = workflow.jobs["package-linux64"].steps.find(
		(step) => step.name === "Validate archive root layout",
	);
	verifyStep.run = "7z l $RUNNER_TEMP/videnoa-linux64-release.7z";
	expectContractFailure(
		"release Linux archive helper bypassed",
		() => validateReleaseWorkflow(workflow),
		/scripts\/package_dist_archive\.sh verify/,
	);
}

for (const [job, text, expected] of [
	["package-linux64-smoke", "2000m 0", /2000m 0/],
	["package-win64-smoke", "-mx=0", /-mx=0/],
	["package-win64-smoke", "7z t $firstVolume", /7z t/],
]) {
	const workflow = structuredClone(loadWorkflow(unitPath));
	const step = workflow.jobs[job].steps.find((step) => step.run?.includes(text));
	assert.ok(step, `${job}: missing expected smoke step`);
	step.run = step.run.replace(text, "");
	expectContractFailure(
		`${job}: archive speed/integrity contract omitted (${text})`,
		() => validateUnitWorkflow(workflow),
		expected,
	);
}

for (const target of ["D:/actions/package-target", "/tmp/package-target", "${{ runner.temp }}/package-target"]) {
	const workflow = structuredClone(loadWorkflow(unitPath));
	const cache = workflow.jobs["package-win64-smoke"].steps.find((step) => step.uses?.startsWith("Swatinem/rust-cache@"));
	cache.with.workspaces = `. -> ${target}`;
	expectContractFailure("absolute cache mapping", () => validateUnitWorkflow(workflow), /cache target must be relative/);
}
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["package-win64-smoke"].steps.find((step) => step.name === "Build package bundle").env.CARGO_TARGET_DIR = "${{ runner.temp }}/different-target";
	expectContractFailure("cache/build directory mismatch", () => validateUnitWorkflow(workflow), /cache and Cargo target directories differ/);
}
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["web-build-check"].steps = workflow.jobs["web-build-check"].steps.filter((step) => !step.uses?.startsWith("actions/upload-artifact@"));
	expectContractFailure("missing verified frontend artifact", () => validateUnitWorkflow(workflow), /upload-artifact/);
}

for (const [label, command] of [["lint", "npm run lint"], ["tests", "npm test"]]) {
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["web-build-check"].steps = workflow.jobs["web-build-check"].steps.filter((step) => !step.run?.startsWith(command));
	expectContractFailure(`Worker web ${label} not run`, () => validateUnitWorkflow(workflow), new RegExp(`web-build-check is missing contract: ${command}`));
}

// Hygiene contracts: timeouts, SHA-pinned actions, and --locked cargo calls.
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	delete workflow.jobs["rust-tests"]["timeout-minutes"];
	expectContractFailure("job without timeout", () => validateUnitWorkflow(workflow), /rust-tests must set timeout-minutes/);
}
{
	const workflow = structuredClone(loadWorkflow(releasePath));
	delete workflow.jobs["package-linux64"]["timeout-minutes"];
	expectContractFailure("release job without timeout", () => validateReleaseWorkflow(workflow), /package-linux64 must set timeout-minutes/);
}
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["web-build-check"].steps[0].uses = "actions/checkout@v5";
	expectContractFailure("action pinned to a mutable tag", () => validateUnitWorkflow(workflow), /must pin actions\/checkout@v5 to a commit SHA/);
}
{
	const workflow = structuredClone(loadWorkflow(releasePath));
	const publish = workflow.jobs["github-release"].steps.at(-1);
	publish.uses = publish.uses.replace(/@[0-9a-f]{40}$/, "@v3");
	expectContractFailure("release action pinned to a mutable tag", () => validateReleaseWorkflow(workflow), /softprops\/action-gh-release@v3 to a commit SHA/);
}
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	const cargoStep = workflow.jobs["rust-tests"].steps.find((step) => step.run?.includes("cargo test --locked -p videnoa-core"));
	assert.ok(cargoStep, "rust-tests: missing Cargo test step");
	cargoStep.run = cargoStep.run.replace("cargo test --locked -p videnoa-core", "cargo test -p videnoa-core");
	expectContractFailure("cargo test without --locked", () => validateUnitWorkflow(workflow), /rust-tests must run cargo with --locked/);
}
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	const clippy = workflow.jobs["workspace-quality"].steps.find((step) => step.run?.includes("cargo clippy --locked --workspace"));
	assert.ok(clippy, "workspace-quality: missing workspace Clippy step");
	clippy.run = "cargo clippy --locked --workspace -- -D warnings";
	expectContractFailure("workspace Clippy skips test targets", () => validateUnitWorkflow(workflow), /workspace-quality.*--all-targets/);
}
{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["workspace-quality"].steps = workflow.jobs["workspace-quality"].steps.filter((step) => !step.uses?.startsWith("EmbarkStudios/cargo-deny-action@"));
	expectContractFailure("advisory check omitted", () => validateUnitWorkflow(workflow), /workspace-quality.*cargo-deny-action/);
}
for (const name of ["dockerhub-publish", "controller-dockerhub-publish"]) {
	const workflow = structuredClone(loadWorkflow(releasePath));
	workflow.jobs[name].needs = ["version-gate", "quality-gate"];
	expectContractFailure(`${name} publishes before packaging finishes`, () => validateReleaseWorkflow(workflow), new RegExp(`${name} must need package-linux64`));
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	const cargoStep = workflow.jobs["rust-tests"].steps.find((step) => step.run?.includes("cargo test --locked -p videnoa-core"));
	assert.ok(cargoStep, "rust-tests: missing Cargo test step");
	delete cargoStep.shell;
	expectContractFailure("Windows pwsh hides earlier cargo failures", () => validateUnitWorkflow(workflow), /rust-tests.*shell: bash/);
}
for (const name of ["package-linux64-smoke", "package-win64-smoke"]) {
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs[name].steps = workflow.jobs[name].steps.filter((step) => !step.run?.includes("scripts/media_tools.ps1"));
	expectContractFailure(`${name} skips the media tools check`, () => validateUnitWorkflow(workflow), new RegExp(`${name}.*media tools`));
}

for (const name of ["package-linux64", "package-win64"]) {
	const workflow = structuredClone(loadWorkflow(releasePath));
	workflow.jobs[name].steps = workflow.jobs[name].steps.filter((step) => !step.run?.includes("scripts/media_tools.ps1"));
	expectContractFailure(`release ${name} skips the media tools check`, () => validateReleaseWorkflow(workflow), new RegExp(`${name}.*media tools`));
}
{
	const workflow = structuredClone(loadWorkflow(releasePath));
	const archive = workflow.jobs["package-win64"].steps.find((step) => step.run?.includes("7z a"));
	assert.ok(archive, "package-win64: missing 7z archive step");
	archive.run = archive.run.replaceAll("$LASTEXITCODE", "$null");
	expectContractFailure("release Windows archive ignores 7z failures", () => validateReleaseWorkflow(workflow), /package-win64.*7z/);
}

console.log(
	"[workflow-contracts] all positive and negative workflow contracts passed",
);
