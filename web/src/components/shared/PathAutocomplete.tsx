import { FileText, FolderOpen, Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { FsEntry } from "@/api/client";
import { browseDirectory } from "@/api/client";
import { Input } from "@/components/ui/input";
import {
	Popover,
	PopoverAnchor,
	PopoverContent,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

interface PathAutocompleteProps {
	value: string;
	onChange: (path: string) => void;
	className?: string;
}

export function PathAutocomplete({
	value,
	onChange,
	className,
}: PathAutocompleteProps) {
	const { t } = useTranslation("common");
	const [open, setOpen] = useState(false);
	const [suggestions, setSuggestions] = useState<FsEntry[]>([]);
	const [loading, setLoading] = useState(false);
	const debounceRef = useRef<ReturnType<typeof setTimeout>>(null);
	const fetchIdRef = useRef(0);

	const fetchSuggestions = useCallback((input: string) => {
		if (debounceRef.current) clearTimeout(debounceRef.current);
		debounceRef.current = setTimeout(() => {
			const id = ++fetchIdRef.current;

			// Split input into directory part + filename prefix for partial matching.
			// e.g. "/home/user/Do" → dir="/home/user/", namePrefix="do"
			const lastSlash = input.lastIndexOf("/");
			const dir = lastSlash >= 0 ? input.slice(0, lastSlash + 1) : input;
			const namePrefix =
				lastSlash >= 0 ? input.slice(lastSlash + 1).toLowerCase() : "";

			setLoading(true);
			browseDirectory(dir)
				.then((entries) => {
					if (id !== fetchIdRef.current) return;
					const filtered = namePrefix
						? entries.filter((e) => e.name.toLowerCase().startsWith(namePrefix))
						: entries;
					setSuggestions(filtered);
					setLoading(false);
					setOpen(true);
				})
				.catch(() => {
					if (id !== fetchIdRef.current) return;
					setSuggestions([]);
					setLoading(false);
				});
		}, 300);
	}, []);

	useEffect(() => {
		return () => {
			if (debounceRef.current) clearTimeout(debounceRef.current);
		};
	}, []);

	const handleInputChange = useCallback(
		(e: React.ChangeEvent<HTMLInputElement>) => {
			const v = e.target.value;
			onChange(v);
			fetchSuggestions(v);
		},
		[onChange, fetchSuggestions],
	);

	const handleSelect = useCallback(
		(entry: FsEntry) => {
			if (entry.is_dir) {
				const dirPath = entry.path.endsWith("/")
					? entry.path
					: `${entry.path}/`;
				onChange(dirPath);
				fetchSuggestions(dirPath);
			} else {
				onChange(entry.path);
				setOpen(false);
			}
		},
		[onChange, fetchSuggestions],
	);

	const handleFocus = useCallback(() => {
		fetchSuggestions(value);
	}, [fetchSuggestions, value]);

	const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
		if (e.key === "Escape") setOpen(false);
	}, []);

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverAnchor asChild>
				<Input
					type="text"
					value={value}
					onChange={handleInputChange}
					onFocus={handleFocus}
					onKeyDown={handleKeyDown}
					className={cn("bg-background/50 border-border/50 px-1.5", className)}
					placeholder={t("pathAutocomplete.placeholder")}
				/>
			</PopoverAnchor>
			<PopoverContent
				className="w-[var(--radix-popover-trigger-width)] p-0"
				align="start"
				sideOffset={2}
				onOpenAutoFocus={(e) => {
					e.preventDefault();
				}}
			>
				<div className="max-h-[180px] overflow-y-auto">
					{loading && suggestions.length === 0 && (
						<div className="flex items-center justify-center py-3">
							<Loader2 className="size-3.5 animate-spin text-muted-foreground" />
							<span className="sr-only">{t("pathAutocomplete.loading")}</span>
						</div>
					)}
					{!loading && suggestions.length === 0 && (
						<div className="px-2 py-2 text-xs text-muted-foreground">
							{t("pathAutocomplete.empty")}
						</div>
					)}
					{suggestions.map((entry) => (
						<button
							key={entry.path}
							type="button"
							className="flex w-full items-center gap-1.5 px-2 py-1 text-left text-xs hover:bg-accent/50 transition-colors cursor-pointer"
							onMouseDown={(e) => {
								e.preventDefault();
							}}
							onClick={() => {
								handleSelect(entry);
							}}
						>
							{entry.is_dir ? (
								<FolderOpen className="size-3 shrink-0 text-amber-500" />
							) : (
								<FileText className="size-3 shrink-0 text-muted-foreground" />
							)}
							<span className="truncate">{entry.name}</span>
						</button>
					))}
				</div>
			</PopoverContent>
		</Popover>
	);
}
