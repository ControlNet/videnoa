import { Layers, LayoutGrid, List, Search, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ModelEntry, ModelType } from "@/api/client";
import { listModels } from "@/api/client";
import { PageContainer } from "@/components/layout/PageContainer";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { getErrorMessage } from "@/lib/presentation-error";
import { cn } from "@/lib/utils";
import { ModelDetail } from "./ModelDetail";

// ─── Constants ───────────────────────────────────────────────────────────────

type TypeFilter = "All" | ModelType;

const TYPE_FILTERS: {
	labelKey: string;
	value: TypeFilter;
	icon: typeof Sparkles;
}[] = [
	{ labelKey: "filters.type.all", value: "All", icon: Layers },
	{
		labelKey: "filters.type.superResolution",
		value: "SuperResolution",
		icon: Sparkles,
	},
	{
		labelKey: "filters.type.frameInterpolation",
		value: "FrameInterpolation",
		icon: Layers,
	},
];

const FALLBACK_MODEL_LOAD_ERROR = "Failed to load models";

/*
 * A model's accent is the accent of the node that runs it, so the two screens
 * agree on what "super resolution" looks like. Mirrors `accent_color` in
 * crates/core/src/descriptor.rs.
 */
const TYPE_ACCENT: Record<ModelType, string> = {
	SuperResolution: "#F97316",
	FrameInterpolation: "#06B6D4",
};

function typeToneClass(type: ModelType): string {
	return type === "SuperResolution" ? "tone-superres" : "tone-interpolation";
}

function typeBadgeLabelKey(type: ModelType): string {
	return type === "SuperResolution"
		? "typeBadge.superResolution"
		: "typeBadge.frameInterpolation";
}

// ─── Segmented control ───────────────────────────────────────────────────────

interface SegmentedOption<Value extends string> {
	value: Value;
	label?: string;
	ariaLabel?: string;
	icon?: typeof Sparkles;
}

/*
 * One control height for the whole filter row, and a selected state you can
 * actually see: the accent wash that already marks the active nav item.
 */
function Segmented<Value extends string>({
	value,
	options,
	onChange,
	groupLabel,
	compact = false,
}: {
	value: Value;
	options: SegmentedOption<Value>[];
	onChange: (next: Value) => void;
	groupLabel: string;
	compact?: boolean;
}) {
	return (
		<div
			role="group"
			aria-label={groupLabel}
			className="flex h-9 shrink-0 items-center gap-0.5 rounded-lg border border-border bg-secondary/45 p-[3px]"
		>
			{options.map((option) => {
				const OptionIcon = option.icon;
				const selected = option.value === value;
				return (
					<button
						key={option.value}
						type="button"
						aria-pressed={selected}
						aria-label={option.ariaLabel}
						onClick={() => {
							onChange(option.value);
						}}
						className={cn(
							"inline-flex h-7 items-center justify-center gap-1.5 rounded-md text-xs transition-colors",
							compact ? "px-2.5" : "px-3",
							selected
								? "bg-primary/15 font-semibold text-primary"
								: "font-medium text-muted-foreground hover:text-foreground",
						)}
					>
						{OptionIcon && <OptionIcon className="size-3.5" />}
						{option.label}
					</button>
				);
			})}
		</div>
	);
}

// ─── Skeleton loaders ────────────────────────────────────────────────────────

function SkeletonCard() {
	return (
		<Card className="overflow-hidden">
			<div className="h-[3px] bg-muted" />
			<CardContent className="space-y-3 p-4">
				<div className="h-5 w-3/4 rounded bg-muted animate-pulse" />
				<div className="flex gap-1.5">
					<div className="h-5 w-24 rounded bg-muted animate-pulse" />
					<div className="h-5 w-10 rounded bg-muted animate-pulse" />
				</div>
				<div className="h-10 w-full rounded bg-muted animate-pulse" />
				<div className="h-8 w-2/3 rounded bg-muted animate-pulse" />
			</CardContent>
		</Card>
	);
}

function SkeletonRow() {
	return (
		<div className="flex items-center gap-4 rounded-lg border border-border bg-card px-4 py-3">
			<div className="h-4 w-40 rounded bg-muted animate-pulse" />
			<div className="h-5 w-24 rounded bg-muted animate-pulse" />
			<div className="h-5 w-10 rounded bg-muted animate-pulse" />
			<div className="h-5 w-12 rounded bg-muted animate-pulse" />
			<div className="h-4 w-24 rounded bg-muted animate-pulse" />
			<div className="h-4 w-36 rounded bg-muted animate-pulse" />
			<div className="h-4 flex-1 rounded bg-muted animate-pulse" />
		</div>
	);
}

// ─── Model card (grid view) ──────────────────────────────────────────────────

function SpecBadge({ children }: { children: React.ReactNode }) {
	return (
		<Badge variant="outline" className="tone-badge tone-spec font-mono font-medium">
			{children}
		</Badge>
	);
}

function ModelCard({
	model,
	onClick,
}: {
	model: ModelEntry;
	onClick: () => void;
}) {
	const { t } = useTranslation(["models", "common"]);
	const [lo, hi] = model.normalization_range;

	return (
		<Card
			className="flex cursor-pointer flex-col overflow-hidden transition-colors hover:border-primary/45"
			onClick={onClick}
		>
			{/* The type, readable across the grid without reading a word of it. */}
			<div
				className="h-[3px] shrink-0"
				style={{ backgroundColor: TYPE_ACCENT[model.model_type] }}
			/>
			<CardContent className="flex flex-1 flex-col gap-3 p-4">
				<CardTitle className="truncate font-mono text-sm font-semibold tracking-[-0.01em]">
					{model.name}
				</CardTitle>

				<div className="flex flex-wrap items-center gap-1.5">
					<Badge
						variant="outline"
						className={cn("tone-badge", typeToneClass(model.model_type))}
					>
						{t(typeBadgeLabelKey(model.model_type))}
					</Badge>
					{model.scale != null && <SpecBadge>{model.scale}x</SpecBadge>}
					{model.is_fp16 && <SpecBadge>FP16</SpecBadge>}
				</div>

				{model.description ? (
					<TooltipProvider delayDuration={300}>
						<Tooltip>
							<TooltipTrigger asChild>
								<p className="line-clamp-2 h-10 cursor-default text-[13px] leading-5 text-foreground/70">
									{model.description}
								</p>
							</TooltipTrigger>
							<TooltipContent
								side="bottom"
								className="max-w-xs border bg-popover text-xs text-popover-foreground"
							>
								{model.description}
							</TooltipContent>
						</Tooltip>
					</TooltipProvider>
				) : (
					<p className="h-10 text-[13px] leading-5 text-muted-foreground italic">
						{t("card.noDescription")}
					</p>
				)}

				<Separator />

				<dl className="grid grid-cols-2 gap-3">
					<div>
						<dt className="text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
							{t("card.rangeLabel")}
						</dt>
						<dd className="mt-0.5 font-mono text-xs">
							{lo}–{hi}
						</dd>
					</div>
					<div className="min-w-0">
						<dt className="text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
							{t("card.inputLabel")}
						</dt>
						<dd className="mt-0.5 truncate font-mono text-xs">
							{model.input_format}
						</dd>
					</div>
				</dl>

				<p className="mt-auto truncate font-mono text-[11px] text-muted-foreground/80">
					{model.filename}
				</p>
			</CardContent>
		</Card>
	);
}

// ─── Model row (list view) ───────────────────────────────────────────────────

function ModelRow({
	model,
	onClick,
}: {
	model: ModelEntry;
	onClick: () => void;
}) {
	const { t } = useTranslation(["models", "common"]);
	return (
		<button
			type="button"
			className="flex w-full cursor-pointer items-center gap-4 rounded-lg border border-border bg-card px-4 py-3 text-left transition-colors hover:border-primary/45"
			onClick={onClick}
		>
			<span
				className="h-6 w-[3px] shrink-0 rounded-full"
				style={{ backgroundColor: TYPE_ACCENT[model.model_type] }}
			/>
			<span className="w-48 shrink-0 truncate font-mono text-[13px] font-semibold">
				{model.name}
			</span>
			<span className="w-32 shrink-0">
				<Badge
					variant="outline"
					className={cn("tone-badge", typeToneClass(model.model_type))}
				>
					{t(typeBadgeLabelKey(model.model_type))}
				</Badge>
			</span>
			<span className="w-12 shrink-0 text-center font-mono text-xs text-muted-foreground">
				{model.scale != null ? `${model.scale}x` : t("common:notAvailable")}
			</span>
			<span className="flex w-14 shrink-0 justify-center">
				{model.is_fp16 ? (
					<SpecBadge>FP16</SpecBadge>
				) : (
					<span className="text-xs text-muted-foreground">
						{t("common:notAvailable")}
					</span>
				)}
			</span>
			<span className="w-24 shrink-0 truncate font-mono text-xs text-muted-foreground">
				{model.input_format}
			</span>
			<span className="w-44 shrink-0 truncate font-mono text-xs text-muted-foreground">
				{model.filename}
			</span>
			<span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
				{model.description || t("common:notAvailable")}
			</span>
		</button>
	);
}

// ─── Empty and error states ──────────────────────────────────────────────────

function EmptyState() {
	const { t } = useTranslation("models");
	return (
		<div className="flex flex-col items-center justify-center py-20 text-center">
			<Search className="mb-3.5 size-8 text-muted-foreground/45" />
			<p className="text-sm text-muted-foreground">{t("empty.noModels")}</p>
		</div>
	);
}

function ErrorState({ message }: { message: string }) {
	return (
		<div className="flex flex-col items-center justify-center py-20 text-center">
			<p className="text-sm text-destructive">{message}</p>
		</div>
	);
}

// ─── Main page ───────────────────────────────────────────────────────────────

export function ModelsPage() {
	const { t } = useTranslation(["models", "common"]);
	const [models, setModels] = useState<ModelEntry[]>([]);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [searchQuery, setSearchQuery] = useState("");
	const [typeFilter, setTypeFilter] = useState<TypeFilter>("All");
	const [view, setView] = useState<"grid" | "list">("grid");
	const [selectedModel, setSelectedModel] = useState<ModelEntry | null>(null);
	const [detailOpen, setDetailOpen] = useState(false);

	useEffect(() => {
		let cancelled = false;
		listModels()
			.then((data) => {
				if (!cancelled) {
					setModels(data);
					setError(null);
				}
			})
			.catch((err: unknown) => {
				if (!cancelled) {
					setError(getErrorMessage(err, FALLBACK_MODEL_LOAD_ERROR));
				}
			})
			.finally(() => {
				if (!cancelled) setLoading(false);
			});
		return () => {
			cancelled = true;
		};
	}, []);

	const filtered = useMemo(() => {
		const q = searchQuery.toLowerCase();
		return models.filter((m) => {
			if (typeFilter !== "All" && m.model_type !== typeFilter) return false;
			if (
				q &&
				!m.name.toLowerCase().includes(q) &&
				!m.description.toLowerCase().includes(q)
			)
				return false;
			return true;
		});
	}, [models, searchQuery, typeFilter]);

	const subtitle = loading
		? t("common:loading")
		: error
			? t("page.subtitle.error")
			: t("page.subtitle.count", { count: filtered.length });

	const errorMessage =
		error === FALLBACK_MODEL_LOAD_ERROR ? t("errors.loadFailed") : error;

	return (
		<PageContainer>
			<div className="mb-6">
				<h2 className="text-lg font-semibold tracking-tight">
					{t("page.title")}
				</h2>
				<p className="mt-1 text-sm text-muted-foreground">{subtitle}</p>
			</div>

			{/* One row, one control height, two groups. */}
			<div className="mb-5 flex flex-wrap items-center gap-3">
				<div className="relative w-80 shrink-0">
					<Search className="pointer-events-none absolute left-3 top-1/2 size-[15px] -translate-y-1/2 text-muted-foreground" />
					<Input
						placeholder={t("search.placeholder")}
						value={searchQuery}
						onChange={(e) => {
							setSearchQuery(e.target.value);
						}}
						className="pl-[34px]"
					/>
				</div>

				<Segmented
					groupLabel={t("filters.groupLabel")}
					value={typeFilter}
					onChange={setTypeFilter}
					options={TYPE_FILTERS.map((f) => ({
						value: f.value,
						label: t(f.labelKey),
						icon: f.icon,
					}))}
				/>

				<div className="ml-auto">
					<Segmented
						compact
						groupLabel={t("view.groupLabel")}
						value={view}
						onChange={setView}
						options={[
							{ value: "grid", ariaLabel: t("view.grid"), icon: LayoutGrid },
							{ value: "list", ariaLabel: t("view.list"), icon: List },
						]}
					/>
				</div>
			</div>

			{loading ? (
				view === "grid" ? (
					<div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
						{Array.from({ length: 6 }, (_, i) => (
							<SkeletonCard key={`skel-card-${String(i)}`} />
						))}
					</div>
				) : (
					<div className="flex flex-col gap-2">
						{Array.from({ length: 6 }, (_, i) => (
							<SkeletonRow key={`skel-row-${String(i)}`} />
						))}
					</div>
				)
			) : error ? (
				<ErrorState message={errorMessage ?? t("errors.loadFailed")} />
			) : filtered.length === 0 ? (
				<EmptyState />
			) : view === "grid" ? (
				<div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
					{filtered.map((m) => (
						<ModelCard
							key={m.name}
							model={m}
							onClick={() => {
								setSelectedModel(m);
								setDetailOpen(true);
							}}
						/>
					))}
				</div>
			) : (
				<div className="flex flex-col gap-2">
					<div className="flex items-center gap-4 px-4 py-2 text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
						<span className="w-[3px] shrink-0" />
						<span className="w-48 shrink-0">{t("listHeaders.name")}</span>
						<span className="w-32 shrink-0">{t("listHeaders.type")}</span>
						<span className="w-12 shrink-0 text-center">
							{t("listHeaders.scale")}
						</span>
						<span className="w-14 shrink-0 text-center">FP16</span>
						<span className="w-24 shrink-0">{t("listHeaders.input")}</span>
						<span className="w-44 shrink-0">{t("listHeaders.filename")}</span>
						<span className="min-w-0 flex-1">
							{t("listHeaders.description")}
						</span>
					</div>
					{filtered.map((m) => (
						<ModelRow
							key={m.name}
							model={m}
							onClick={() => {
								setSelectedModel(m);
								setDetailOpen(true);
							}}
						/>
					))}
				</div>
			)}
			<ModelDetail
				model={selectedModel}
				open={detailOpen}
				onOpenChange={setDetailOpen}
			/>
		</PageContainer>
	);
}
