import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, getConfig, updateConfig } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import type { AppConfig } from "@/types";
import { SettingsPage } from "../SettingsPage";

vi.mock("@/api/client", async (importOriginal) => ({
	...(await importOriginal<typeof import("@/api/client")>()),
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
		vi.mocked(getConfig).mockResolvedValue({ config: makeConfig(), etag: '"v1"' });
		vi.mocked(updateConfig).mockResolvedValue({
			config: makeConfig({ performance: { profiling_enabled: true } }),
			etag: '"v2"',
		});

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
			'"v1"',
		);
	});

	it("keeps iroh relays when the iroh toggle changes", async () => {
		const iroh = { enabled: true, relay_urls: ["https://relay.example.test"] };
		vi.mocked(getConfig).mockResolvedValue({ config: makeConfig({ iroh }), etag: '"v1"' });
		vi.mocked(updateConfig).mockResolvedValue({ config: makeConfig({ iroh: { ...iroh, enabled: false } }), etag: '"v2"' });

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
			'"v1"',
		);
	});

	it("edits iroh relays one per line, opts into public relays, and drops both when cleared", async () => {
		const iroh = { enabled: true, relay_urls: ["https://relay.example.test"] };
		vi.mocked(getConfig).mockResolvedValue({ config: makeConfig({ iroh }), etag: '"v1"' });
		vi.mocked(updateConfig).mockImplementation(async (config) => ({ config, etag: '"v2"' }));

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

		vi.mocked(getConfig).mockResolvedValue({ config: legacyConfig, etag: '"v1"' });
		vi.mocked(updateConfig).mockResolvedValue({ config: makeConfig(), etag: '"v2"' });

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

	// Settings are one document: a save based on an old version (e.g. after
	// the desktop language switch wrote it) must not revert the newer one.
	it("reloads and says so when the settings changed elsewhere", async () => {
		vi.mocked(getConfig)
			.mockResolvedValueOnce({ config: makeConfig(), etag: '"v1"' })
			.mockResolvedValueOnce({
				config: makeConfig({ locale: "zh-CN", performance: { profiling_enabled: true } }),
				etag: '"v2"',
			});
		vi.mocked(updateConfig).mockRejectedValue(
			new ApiError(412, "changed", { error: "changed", code: "config_changed" }),
		);

		render(
			<MemoryRouter>
				<SettingsPage />
			</MemoryRouter>,
		);
		const models = await screen.findByLabelText(/^Models/);
		fireEvent.change(models, { target: { value: "other-models" } });
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(getConfig).toHaveBeenCalledTimes(2);
		});
		expect(updateConfig).toHaveBeenCalledWith(
			expect.objectContaining({
				paths: expect.objectContaining({ models_dir: "other-models" }),
			}),
			'"v1"',
		);
		expect(
			await screen.findByText(
				"Settings were changed elsewhere and have been reloaded. Apply your changes again.",
			),
		).toBeInTheDocument();
		expect(screen.getByLabelText(/^Models/)).toHaveValue("models");
		expect(screen.getByLabelText("Profiling telemetry")).toBeChecked();
	});
});

describe("SettingsPage server section", () => {
	afterEach(() => {
		delete (globalThis as { __TAURI__?: unknown }).__TAURI__;
	});

	function renderPage() {
		render(
			<MemoryRouter>
				<SettingsPage />
			</MemoryRouter>,
		);
	}

	it("shows the configured host and port in the browser", async () => {
		vi.mocked(getConfig).mockResolvedValue({ config: makeConfig(), etag: '"v1"' });

		renderPage();

		const server = await screen.findByRole("region", { name: "Server" });
		await waitFor(() => {
			expect(within(server).getByText("3000")).toBeInTheDocument();
		});
		expect(within(server).getByText("0.0.0.0")).toBeInTheDocument();
	});

	// The desktop app binds 127.0.0.1 on a port chosen at start and ignores
	// server.host/server.port, so showing them would be misleading.
	it("marks host and port as not applicable in the desktop app and keeps them on save", async () => {
		(globalThis as { __TAURI__?: unknown }).__TAURI__ = { window: { appWindow: {} } };
		vi.mocked(getConfig).mockResolvedValue({ config: makeConfig(), etag: '"v1"' });
		vi.mocked(updateConfig).mockImplementation(async (config) => ({ config, etag: '"v2"' }));

		renderPage();

		const models = await screen.findByLabelText(/^Models/);
		const server = screen.getByRole("region", { name: "Server" });
		expect(within(server).getByText(/Not used by the desktop app/)).toBeInTheDocument();
		expect(within(server).queryByText("3000")).not.toBeInTheDocument();
		expect(within(server).queryByText("0.0.0.0")).not.toBeInTheDocument();

		fireEvent.change(models, { target: { value: "other-models" } });
		fireEvent.click(screen.getByRole("button", { name: "Save" }));
		await waitFor(() => {
			expect(updateConfig).toHaveBeenCalledTimes(1);
		});
		expect(updateConfig).toHaveBeenCalledWith(
			expect.objectContaining({ server: { port: 3000, host: "0.0.0.0" } }),
			'"v1"',
		);
	});
});
