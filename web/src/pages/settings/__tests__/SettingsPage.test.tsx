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

	it("keeps iroh relays when the iroh toggle changes", async () => {
		const iroh = { enabled: true, relay_urls: ["https://relay.example.test"] };
		vi.mocked(getConfig).mockResolvedValue(makeConfig({ iroh }));
		vi.mocked(updateConfig).mockResolvedValue(makeConfig({ iroh: { ...iroh, enabled: false } }));

		render(
			<MemoryRouter>
				<SettingsPage />
			</MemoryRouter>,
		);
		await waitFor(() => {
			expect(getConfig).toHaveBeenCalledTimes(1);
		});

		fireEvent.click(screen.getByLabelText("Enable iroh"));
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(updateConfig).toHaveBeenCalledTimes(1);
		});
		expect(updateConfig).toHaveBeenCalledWith(
			expect.objectContaining({
				iroh: { enabled: false, relay_urls: ["https://relay.example.test"] },
			}),
		);
	});

	it("edits iroh relays one per line, opts into public relays, and drops both when cleared", async () => {
		const iroh = { enabled: true, relay_urls: ["https://relay.example.test"] };
		vi.mocked(getConfig).mockResolvedValue(makeConfig({ iroh }));
		vi.mocked(updateConfig).mockImplementation(async (config) => config);

		render(
			<MemoryRouter>
				<SettingsPage />
			</MemoryRouter>,
		);
		const relays = await screen.findByLabelText(/^Relay URLs/);
		expect(relays).toHaveValue("https://relay.example.test");

		// Blank lines and surrounding spaces are not part of the saved list.
		fireEvent.change(relays, {
			target: { value: " https://relay.example.test\n\nhttp://10.0.0.5:3340 \n" },
		});
		expect(relays).toHaveValue(" https://relay.example.test\n\nhttp://10.0.0.5:3340 \n");
		const usePublic = screen.getByLabelText(/Also use the public iroh relays/);
		expect(usePublic).not.toBeChecked();
		fireEvent.click(usePublic);
		fireEvent.click(screen.getByRole("button", { name: "Save" }));
		await waitFor(() => {
			expect(updateConfig).toHaveBeenCalledTimes(1);
		});
		expect(vi.mocked(updateConfig).mock.calls[0]?.[0].iroh).toEqual({
			enabled: true,
			relay_urls: ["https://relay.example.test", "http://10.0.0.5:3340"],
			use_public_relays: true,
		});
		await waitFor(() => {
			expect(relays).toHaveValue("https://relay.example.test\nhttp://10.0.0.5:3340");
		});

		// An empty list returns to the public relays, as omitted keys.
		fireEvent.change(relays, { target: { value: "" } });
		expect(usePublic).toBeDisabled();
		fireEvent.click(screen.getByRole("button", { name: "Save" }));
		await waitFor(() => {
			expect(updateConfig).toHaveBeenCalledTimes(2);
		});
		expect(vi.mocked(updateConfig).mock.calls[1]?.[0].iroh).toEqual({ enabled: true });
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
