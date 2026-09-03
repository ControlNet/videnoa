#!/usr/bin/env node

import path from "node:path";
import process from "node:process";
import {
	loadWorkflow,
	WorkflowContractError,
} from "./workflow_contracts/common.mjs";
import { validateReleaseWorkflow } from "./workflow_contracts/release.mjs";
import { validateUnitWorkflow } from "./workflow_contracts/unittest.mjs";

const repoRoot = path.resolve(
	path.dirname(new URL(import.meta.url).pathname),
	"..",
);

export {
	loadWorkflow,
	validateReleaseWorkflow,
	validateUnitWorkflow,
	WorkflowContractError,
};

export function validateRepositoryWorkflows(root = repoRoot) {
	validateUnitWorkflow(
		loadWorkflow(path.join(root, ".github/workflows/unittest.yaml")),
	);
	validateReleaseWorkflow(
		loadWorkflow(path.join(root, ".github/workflows/release.yaml")),
	);
}

if (process.argv[1] === new URL(import.meta.url).pathname) {
	validateRepositoryWorkflows();
	console.log("[workflow-contracts] CI and release workflow contracts passed");
}
