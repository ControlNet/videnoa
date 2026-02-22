import {
	act,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FsEntry } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import { PathAutocomplete } from "../PathAutocomplete";

vi.mock("@/api/client", () => ({
	browseDirectory: vi.fn(),
}));

import { browseDirectory } from "@/api/client";

const MOCK_ENTRIES: FsEntry[] = [
	{ name: "Documents", is_dir: true, path: "/home/Documents" },
	{ name: "photo.jpg", is_dir: false, path: "/home/photo.jpg" },
];

beforeEach(async () => {
	initializeI18n();
	await i18n.changeLanguage("en");
	vi.mocked(browseDirectory).mockReset();
	vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
	vi.useRealTimers();
});

describe("PathAutocomplete", () => {
	it("renders input with placeholder", () => {
		render(<PathAutocomplete value="" onChange={vi.fn()} />);
		expect(screen.getByPlaceholderText("path/to/file")).toBeInTheDocument();
	});

	it("calls onChange when user types", () => {
		const onChange = vi.fn();
		render(<PathAutocomplete value="" onChange={onChange} />);
		const input = screen.getByPlaceholderText("path/to/file");
		fireEvent.change(input, { target: { value: "/home/" } });
		expect(onChange).toHaveBeenCalledWith("/home/");
	});

	it("shows suggestions from browseDirectory", async () => {
		vi.mocked(browseDirectory).mockResolvedValue(MOCK_ENTRIES);

		render(<PathAutocomplete value="/home/" onChange={vi.fn()} />);
		const input = screen.getByPlaceholderText("path/to/file");

		await act(async () => {
			fireEvent.focus(input);
			await vi.advanceTimersByTimeAsync(300);
		});

		await waitFor(() => {
			expect(screen.getByText("Documents")).toBeInTheDocument();
		});
	});

	it("shows folder and file icons", async () => {
		vi.mocked(browseDirectory).mockResolvedValue(MOCK_ENTRIES);

		render(<PathAutocomplete value="/home/" onChange={vi.fn()} />);
		const input = screen.getByPlaceholderText("path/to/file");

		await act(async () => {
			fireEvent.focus(input);
			await vi.advanceTimersByTimeAsync(300);
		});

		await waitFor(() => {
			expect(screen.getByText("Documents")).toBeInTheDocument();
		});

		const buttons = screen.getAllByRole("button");
		const folderButton = buttons.find((b) => b.textContent === "Documents");
		const fileButton = buttons.find((b) => b.textContent === "photo.jpg");

		expect(folderButton).toBeDefined();
		expect(fileButton).toBeDefined();

		if (!folderButton || !fileButton) {
			throw new Error(
				"Expected folder and file suggestion buttons to be present",
			);
		}

		const folderSvg = folderButton.querySelector("svg");
		expect(folderSvg).toBeTruthy();

		if (!folderSvg) {
			throw new Error("Expected folder icon SVG to be present");
		}
		expect(folderSvg.classList.toString()).toContain("text-amber-500");

		const fileSvg = fileButton.querySelector("svg");
		expect(fileSvg).toBeTruthy();

		if (!fileSvg) {
			throw new Error("Expected file icon SVG to be present");
		}
		expect(fileSvg.classList.toString()).toContain("text-muted-foreground");
	});

	it("calls onChange with directory path on folder click", async () => {
		const onChange = vi.fn();
		vi.mocked(browseDirectory).mockResolvedValue(MOCK_ENTRIES);

		render(<PathAutocomplete value="/home/" onChange={onChange} />);
		const input = screen.getByPlaceholderText("path/to/file");

		await act(async () => {
			fireEvent.focus(input);
			await vi.advanceTimersByTimeAsync(300);
		});

		await waitFor(() => {
			expect(screen.getByText("Documents")).toBeInTheDocument();
		});

		onChange.mockClear();
		fireEvent.click(screen.getByText("Documents"));

		expect(onChange).toHaveBeenCalledWith("/home/Documents/");
	});

	it("shows localized empty text in zh-CN", async () => {
		await i18n.changeLanguage("zh-CN");
		vi.mocked(browseDirectory)
			.mockResolvedValueOnce(MOCK_ENTRIES)
			.mockResolvedValueOnce([]);

		render(<PathAutocomplete value="/home/" onChange={vi.fn()} />);
		const input = screen.getByPlaceholderText("path/to/file");

		await act(async () => {
			fireEvent.focus(input);
			await vi.advanceTimersByTimeAsync(300);
		});

		await waitFor(() => {
			expect(screen.getByText("Documents")).toBeInTheDocument();
		});

		await act(async () => {
			fireEvent.change(input, { target: { value: "/home/not-found" } });
			await vi.advanceTimersByTimeAsync(300);
		});

		await waitFor(() => {
			expect(screen.getByText("未找到条目")).toBeInTheDocument();
		});
	});
});
