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
	delete workflow.jobs["docker-build-smoke"];
	expectContractFailure(
		"existing package break",
		() => validateUnitWorkflow(workflow),
		/missing required job: docker-build-smoke/,
	);
}

{
	const workflow = structuredClone(loadWorkflow(unitPath));
	workflow.jobs["controller-docker-smoke"].steps[1].run =
		"docker build -t videnoa-controller:ci .";
	expectContractFailure(
		"missing Controller Dockerfile",
		() => validateUnitWorkflow(workflow),
		/Dockerfile.controller/,
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

console.log(
	"[workflow-contracts] all positive and negative workflow contracts passed",
);
