import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { i18n, initializeI18n } from "@/i18n";
import { ErrorBoundary, ErrorFallback } from "../ErrorBoundary";

function ThrowingComponent(): never {
	throw new Error("Test error");
}

describe("ErrorBoundary", () => {
	beforeEach(async () => {
		initializeI18n();
		await i18n.changeLanguage("en");
	});

	it("renders children when no error", () => {
		render(
			<ErrorBoundary>
				<p>Hello</p>
			</ErrorBoundary>,
		);
		expect(screen.getByText("Hello")).toBeInTheDocument();
	});

	it("shows fallback when child throws", () => {
		const spy = vi.spyOn(console, "error").mockImplementation(() => {});
		render(
			<ErrorBoundary>
				<ThrowingComponent />
			</ErrorBoundary>,
		);
		expect(screen.getByText("Something went wrong")).toBeInTheDocument();
		expect(screen.getByText("Test error")).toBeInTheDocument();
		spy.mockRestore();
	});
});

describe("ErrorFallback", () => {
	beforeEach(async () => {
		initializeI18n();
		await i18n.changeLanguage("en");
	});

	it("shows the error message", () => {
		const error = new Error("Something broke");
		render(<ErrorFallback error={error} resetError={vi.fn()} />);
		expect(screen.getByText("Something broke")).toBeInTheDocument();
		expect(screen.getByText("Something went wrong")).toBeInTheDocument();
	});

	it("toggles stack trace on Show details click", () => {
		const error = new Error("Boom");
		error.stack = "Error: Boom\n    at test.tsx:1:1";
		render(<ErrorFallback error={error} resetError={vi.fn()} />);

		expect(screen.queryByText(/Error: Boom/)).not.toBeInTheDocument();

		fireEvent.click(screen.getByText("Show details"));

		expect(screen.getByText(/Error: Boom/)).toBeInTheDocument();
		expect(screen.getByText("Hide details")).toBeInTheDocument();
	});

	it("calls resetError on Try Again click", () => {
		const resetError = vi.fn();
		const error = new Error("fail");
		render(<ErrorFallback error={error} resetError={resetError} />);

		fireEvent.click(screen.getByText("Try Again"));

		expect(resetError).toHaveBeenCalledOnce();
	});

	it("renders localized labels in zh-CN", async () => {
		await i18n.changeLanguage("zh-CN");
		const error = new Error("Something broke");

		render(<ErrorFallback error={error} resetError={vi.fn()} />);

		expect(screen.getByText("出现了错误")).toBeInTheDocument();
		expect(screen.getByText("显示详情")).toBeInTheDocument();
		expect(screen.getByText("重试")).toBeInTheDocument();
	});
});
