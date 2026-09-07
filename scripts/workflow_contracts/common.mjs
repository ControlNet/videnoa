import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

const repoRoot = path.resolve(
	path.dirname(new URL(import.meta.url).pathname),
	"../..",
);
const require = createRequire(
	path.join(repoRoot, "controller-web/package.json"),
);
const yaml = require("js-yaml");

export class WorkflowContractError extends Error {}

export const expression = (value) => `${String.fromCharCode(36)}{{ ${value} }}`;

function fail(message) {
	throw new WorkflowContractError(message);
}

export function requireValue(condition, message) {
	if (!condition) fail(message);
}

function asList(value) {
	if (value === undefined) return [];
	return Array.isArray(value) ? value : [value];
}

export function requireJob(jobs, name) {
	const job = jobs[name];
	requireValue(job !== undefined, `missing required job: ${name}`);
	return job;
}

export function requireNeeds(job, jobName, expected) {
	const actual = new Set(asList(job.needs));
	for (const dependency of expected) {
		requireValue(actual.has(dependency), `${jobName} must need ${dependency}`);
	}
}

export function requireText(job, jobName, expected) {
	const text = JSON.stringify(job);
	for (const value of expected) {
		requireValue(
			text.includes(value),
			`${jobName} is missing contract: ${value}`,
		);
	}
}

export function validateGraph(jobs, workflowName) {
	for (const [jobName, job] of Object.entries(jobs)) {
		for (const dependency of asList(job.needs)) {
			requireValue(
				jobs[dependency] !== undefined,
				`${workflowName}.${jobName} needs missing job ${dependency}`,
			);
		}
	}
	const visiting = new Set();
	const visited = new Set();
	function visit(jobName) {
		if (visiting.has(jobName))
			fail(`${workflowName} contains a dependency cycle at ${jobName}`);
		if (visited.has(jobName)) return;
		visiting.add(jobName);
		for (const dependency of asList(jobs[jobName].needs)) visit(dependency);
		visiting.delete(jobName);
		visited.add(jobName);
	}
	for (const jobName of Object.keys(jobs)) visit(jobName);
}

export function loadWorkflow(filePath) {
	const parsed = yaml.load(fs.readFileSync(filePath, "utf8"));
	requireValue(
		parsed && typeof parsed === "object",
		`${filePath} must contain a YAML mapping`,
	);
	requireValue(
		parsed.jobs && typeof parsed.jobs === "object",
		`${filePath} must define jobs`,
	);
	return parsed;
}
