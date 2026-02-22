import { render, screen } from "@testing-library/react";
import { useTranslation } from "react-i18next";
import { beforeEach, describe, expect, it } from "vitest";
import { i18n, initializeI18n } from "@/i18n";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "../dialog";

function FallbackProbe() {
	const { t } = useTranslation("common");
	return <span>{t("qa.fallback.missingInZh")}</span>;
}

describe("Dialog", () => {
	beforeEach(async () => {
		initializeI18n();
		await i18n.changeLanguage("en");
	});

	it("uses localized close sr-only label in en", () => {
		render(
			<Dialog open>
				<DialogContent>
					<DialogTitle>Dialog title</DialogTitle>
					<DialogDescription>Dialog description</DialogDescription>
				</DialogContent>
			</Dialog>,
		);

		expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
	});

	it("uses localized close sr-only label in zh-CN", async () => {
		await i18n.changeLanguage("zh-CN");

		render(
			<Dialog open>
				<DialogContent>
					<DialogTitle>Dialog title</DialogTitle>
					<DialogDescription>Dialog description</DialogDescription>
				</DialogContent>
			</Dialog>,
		);

		expect(screen.getByRole("button", { name: "关闭" })).toBeInTheDocument();
	});

	it("falls back to en close label when zh-CN key is missing", async () => {
		const fallbackLabel = i18n.getFixedT("en", "common")("dialog.close");
		const fallbackText = `${fallbackLabel} fallback sentinel`;

		i18n.addResource("en", "common", "qa.fallback.missingInZh", fallbackText);
		await i18n.changeLanguage("zh-CN");

		render(<FallbackProbe />);

		expect(screen.getByText(fallbackText)).toBeInTheDocument();
	});
});
