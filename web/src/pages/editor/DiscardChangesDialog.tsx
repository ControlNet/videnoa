import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";

/** Asks before an action replaces a canvas that has unsaved changes. */
export function DiscardChangesDialog({
	open,
	onCancel,
	onDiscard,
}: {
	open: boolean;
	onCancel: () => void;
	onDiscard: () => void;
}) {
	const { t } = useTranslation("editor");
	return (
		<Dialog
			open={open}
			onOpenChange={(next) => {
				if (!next) onCancel();
			}}
		>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>{t("unsaved.title")}</DialogTitle>
					<DialogDescription>{t("unsaved.description")}</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="outline" onClick={onCancel}>
						{t("unsaved.cancel")}
					</Button>
					<Button variant="destructive" onClick={onDiscard}>
						{t("unsaved.discard")}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
