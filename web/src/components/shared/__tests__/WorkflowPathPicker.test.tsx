import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
});
