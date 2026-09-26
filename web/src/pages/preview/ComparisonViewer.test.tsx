import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { extractFrames, processFrame } from "@/api/client";
import { useUIStore } from "@/stores/ui-store";
import { ComparisonViewer } from "./ComparisonViewer";

// These API responses are synthetic fixtures for request ownership tests.
vi.mock("@/api/client", () => ({
	extractFrames: vi.fn(),
	processFrame: vi.fn(),
}));

function deferred<T>() {
	let resolve!: (value: T) => void;
	const promise = new Promise<T>((done) => { resolve = done; });
	return { promise, resolve };
}

beforeEach(() => {
	vi.resetAllMocks();
	useUIStore.setState({ activeModal: "preview" });
	vi.mocked(extractFrames).mockResolvedValue({
		preview_id: "test-preview",
		frames: [
			{ index: 0, url: "/test/frame-0.png" },
			{ index: 1, url: "/test/frame-1.png" },
		],
	});
});

async function openFrames() {
	render(<ComparisonViewer />);
	fireEvent.change(screen.getByPlaceholderText("Enter video file path..."), {
		target: { value: "/test/video.mkv" },
	});
	fireEvent.click(screen.getByRole("button", { name: "Extract Frames" }));
	await screen.findByRole("button", { name: "Process Frame" });
}

it.each(["thumbnail", "keyboard"])("ignores a late result after %s frame selection", async (navigation) => {
	const pending = deferred<Awaited<ReturnType<typeof processFrame>>>();
	vi.mocked(processFrame).mockReturnValueOnce(pending.promise);
	await openFrames();
	fireEvent.click(screen.getByRole("button", { name: "Process Frame" }));
	if (navigation === "thumbnail") {
		fireEvent.click(screen.getByAltText("Frame 1"));
	} else {
		fireEvent.keyDown(window, { key: "ArrowRight" });
	}
	await act(async () => pending.resolve({ processed_url: "/test/stale.png" }));
	expect(screen.queryByAltText("After")).not.toBeInTheDocument();
	expect(screen.getByAltText("Before")).toHaveAttribute("src", "/test/frame-1.png");
	expect(screen.getByRole("button", { name: "Process Frame" })).toBeEnabled();
});

it("clears an earlier result when a new processing request fails", async () => {
	vi.mocked(processFrame)
		.mockResolvedValueOnce({ processed_url: "/test/processed.png" })
		.mockRejectedValueOnce(new Error("Unsupported preview node"));
	await openFrames();
	fireEvent.click(screen.getByRole("button", { name: "Process Frame" }));
	await screen.findByAltText("After");
	fireEvent.click(screen.getByRole("button", { name: "Process Frame" }));
	await screen.findByText("Unsupported preview node");
	expect(screen.queryByAltText("After")).not.toBeInTheDocument();
});

it("ignores a pending result after closing and reopening the preview", async () => {
	const pending = deferred<Awaited<ReturnType<typeof processFrame>>>();
	vi.mocked(processFrame).mockReturnValueOnce(pending.promise);
	await openFrames();
	fireEvent.click(screen.getByRole("button", { name: "Process Frame" }));
	act(() => useUIStore.setState({ activeModal: null }));
	await act(async () => pending.resolve({ processed_url: "/test/stale.png" }));
	act(() => useUIStore.setState({ activeModal: "preview" }));
	await waitFor(() => expect(screen.getByRole("button", { name: "Process Frame" })).toBeEnabled());
	expect(screen.queryByAltText("After")).not.toBeInTheDocument();
});
