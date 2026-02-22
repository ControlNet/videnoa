import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getConfig, updateConfig } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import type { AppConfig } from "@/types";
import { SettingsPage } from "../SettingsPage";

vi.mock("@/api/client", () => ({
	getConfig: vi.fn(),
	updateConfig: vi.fn(),
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

function makeConfig(overrides: Partial<AppConfig> = {}): AppConfig {
	return {
		paths: {
			models_dir: "models",
			trt_cache_dir: "trt_cache",
			presets_dir: "presets",
			workflows_dir: "data/workflows",
		},
		server: {
			port: 3000,
			host: "0.0.0.0",
		},
		locale: "en",
		performance: {
			profiling_enabled: false,
		},
		...overrides,
	};
}

beforeEach(async () => {
	vi.clearAllMocks();
	initializeI18n();
	await i18n.changeLanguage("en");
});

describe("SettingsPage profiling toggle", () => {
	it("renders profiling toggle and saves profiling_enabled changes", async () => {
		vi.mocked(getConfig).mockResolvedValue(makeConfig());
		vi.mocked(updateConfig).mockResolvedValue(
			makeConfig({ performance: { profiling_enabled: true } }),
		);

		render(
			<MemoryRouter>
				<SettingsPage />
			</MemoryRouter>,
		);

		await waitFor(() => {
			expect(getConfig).toHaveBeenCalledTimes(1);
		});

		const profilingToggle = screen.getByLabelText("Profiling telemetry");
		expect(profilingToggle).not.toBeChecked();

		fireEvent.click(profilingToggle);
		expect(profilingToggle).toBeChecked();

		const saveButton = screen.getByRole("button", { name: "Save" });
		expect(saveButton).not.toBeDisabled();
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(updateConfig).toHaveBeenCalledTimes(1);
		});

		expect(updateConfig).toHaveBeenCalledWith(
			expect.objectContaining({
				performance: { profiling_enabled: true },
			}),
		);
	});

	it("normalizes legacy config responses that omit performance section", async () => {
		const legacyConfig = {
			paths: {
				models_dir: "models",
				trt_cache_dir: "trt_cache",
				presets_dir: "presets",
				workflows_dir: "data/workflows",
			},
			server: {
				port: 3000,
				host: "0.0.0.0",
			},
			locale: "en",
		} as unknown as AppConfig;

		vi.mocked(getConfig).mockResolvedValue(legacyConfig);
		vi.mocked(updateConfig).mockResolvedValue(makeConfig());

		render(
			<MemoryRouter>
				<SettingsPage />
			</MemoryRouter>,
		);

		await waitFor(() => {
			expect(screen.getByLabelText("Profiling telemetry")).toBeInTheDocument();
		});

		expect(screen.getByLabelText("Profiling telemetry")).not.toBeChecked();
		expect(screen.queryByText("Something went wrong")).not.toBeInTheDocument();
	});
});
