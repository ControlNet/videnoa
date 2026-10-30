import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { i18n, initializeI18n } from "@/i18n";
import { WorkflowPathPicker } from "../WorkflowPathPicker";

vi.mock("@/api/client", () => ({
	listPresets: vi.fn(),
	listWorkflows: vi.fn(),
}));

import { listPresets, listWorkflows } from "@/api/client";

function openPicker(label: string) {
	const trigger = screen.getByRole("combobox", { name: label });
	trigger.focus();
	fireEvent.keyDown(trigger, { key: "ArrowDown", code: "ArrowDown" });
}

describe("WorkflowPathPicker", () => {
	beforeEach(async () => {
		initializeI18n();
		await i18n.changeLanguage("en");
		vi.mocked(listPresets).mockReset();
		vi.mocked(listWorkflows).mockReset();
		vi.mocked(listPresets).mockResolvedValue([]);
		vi.mocked(listWorkflows).mockResolvedValue([]);
	});

	it("shows localized placeholders and empty state in en", async () => {
		render(<WorkflowPathPicker value="" onChange={vi.fn()} />);

		expect(screen.getByText("Loading...")).toBeInTheDocument();

		await waitFor(() => {
			expect(screen.getByText("Select workflow")).toBeInTheDocument();
		});
		expect(
			screen.getByRole("combobox", { name: "Select workflow path" }),
		).toBeInTheDocument();

		openPicker("Select workflow path");
		expect(
			await screen.findByText("No workflows with interfaces"),
		).toBeInTheDocument();
	});

	it("shows localized placeholders and empty state in zh-CN", async () => {
		await i18n.changeLanguage("zh-CN");

		render(<WorkflowPathPicker value="" onChange={vi.fn()} />);

		expect(screen.getByText("加载中...")).toBeInTheDocument();

		await waitFor(() => {
			expect(screen.getByText("选择工作流")).toBeInTheDocument();
		});
		expect(
			screen.getByRole("combobox", { name: "选择工作流路径" }),
		).toBeInTheDocument();

		openPicker("选择工作流路径");
		expect(await screen.findByText("没有可用接口的工作流")).toBeInTheDocument();
	});

	it("an empty list that loaded is not an error", async () => {
		render(<WorkflowPathPicker value="" onChange={vi.fn()} />);

		await waitFor(() => {
			expect(screen.getByText("Select workflow")).toBeInTheDocument();
		});
		expect(screen.queryByRole("alert")).not.toBeInTheDocument();
	});

	// A failed list must not read as "no workflows with interfaces".
	it("says the list failed to load and retries it", async () => {
		vi.mocked(listWorkflows)
			.mockRejectedValueOnce(new Error("HTTP 500"))
			.mockResolvedValueOnce([
				{
					filename: "upscale.json",
					name: "Upscale",
					description: "",
					workflow: { nodes: [], connections: [] },
					has_interface: true,
				},
			]);

		render(<WorkflowPathPicker value="" onChange={vi.fn()} />);

		const alert = await screen.findByRole("alert");
		expect(alert).toHaveTextContent("Could not load the workflow list");
		openPicker("Select workflow path");
		expect(screen.queryByText("No workflows with interfaces")).not.toBeInTheDocument();
		fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

		fireEvent.click(within(alert).getByRole("button", { name: "Retry" }));

		await waitFor(() => {
			expect(screen.queryByRole("alert")).not.toBeInTheDocument();
		});
		expect(listWorkflows).toHaveBeenCalledTimes(2);
		openPicker("Select workflow path");
		expect(await screen.findByRole("option", { name: "Upscale" })).toBeInTheDocument();
	});
});
