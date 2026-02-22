import { Layers, LayoutGrid, List, Search, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ModelEntry, ModelType } from "@/api/client";
import { listModels } from "@/api/client";
import { PageContainer } from "@/components/layout/PageContainer";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { getErrorMessage } from "@/lib/presentation-error";
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

// ─── Badge styles ────────────────────────────────────────────────────────────

function typeBadgeClass(type: ModelType): string {
	return type === "SuperResolution"
		? "bg-orange-500/20 text-orange-400 border-orange-500/30"
		: "bg-cyan-500/20 text-cyan-400 border-cyan-500/30";
}

function typeBadgeLabelKey(type: ModelType): string {
	return type === "SuperResolution"
		? "typeBadge.superResolution"
		: "typeBadge.frameInterpolation";
}

// ─── Skeleton loader ─────────────────────────────────────────────────────────

function SkeletonCard() {
	return (
		<Card className="overflow-hidden">
			<CardHeader className="pb-3">
				<div className="h-5 w-3/4 rounded bg-muted animate-pulse" />
				<div className="flex gap-2 mt-2">
					<div className="h-5 w-24 rounded bg-muted animate-pulse" />
					<div className="h-5 w-10 rounded bg-muted animate-pulse" />
				</div>
			</CardHeader>
			<CardContent className="space-y-2">
				<div className="h-4 w-full rounded bg-muted animate-pulse" />
				<div className="h-4 w-2/3 rounded bg-muted animate-pulse" />
				<div className="h-3 w-1/2 rounded bg-muted animate-pulse mt-3" />
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

// ─── Model Card (Grid View) ──────────────────────────────────────────────────

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
			className="overflow-hidden transition-colors hover:border-muted-foreground/30 cursor-pointer"
			onClick={onClick}
		>
			<CardHeader className="pb-3">
				<CardTitle className="text-sm">{model.name}</CardTitle>
				<div className="flex flex-wrap items-center gap-1.5 mt-1">
					<Badge variant="outline" className={typeBadgeClass(model.model_type)}>
						{t(typeBadgeLabelKey(model.model_type))}
					</Badge>
					{model.scale != null && (
						<Badge
							variant="outline"
							className="bg-emerald-500/20 text-emerald-400 border-emerald-500/30"
						>
							{model.scale}x
						</Badge>
					)}
					{model.is_fp16 && (
						<Badge
							variant="outline"
							className="bg-purple-500/20 text-purple-400 border-purple-500/30"
						>
							FP16
						</Badge>
					)}
				</div>
			</CardHeader>
			<CardContent className="space-y-2 text-xs">
				<p className="font-mono text-muted-foreground truncate">
					{model.filename}
				</p>
				{model.description ? (
					<TooltipProvider delayDuration={300}>
						<Tooltip>
							<TooltipTrigger asChild>
								<p className="text-muted-foreground line-clamp-2 cursor-default">
									{model.description}
								</p>
							</TooltipTrigger>
							<TooltipContent
								side="bottom"
								className="max-w-xs text-xs bg-popover text-popover-foreground border"
							>
								{model.description}
							</TooltipContent>
						</Tooltip>
					</TooltipProvider>
				) : (
					<p className="text-muted-foreground italic">
						{t("card.noDescription")}
					</p>
				)}
				<p className="text-muted-foreground/70 pt-1">
					{t("card.range", { min: lo, max: hi })}
				</p>
				<p className="text-muted-foreground/70">
					{t("card.inputFormat", { format: model.input_format })}
				</p>
			</CardContent>
		</Card>
	);
}

// ─── Model Row (List View) ───────────────────────────────────────────────────

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
			className="flex w-full items-center gap-4 rounded-lg border border-border bg-card px-4 py-3 text-left transition-colors hover:border-muted-foreground/30 cursor-pointer"
			onClick={onClick}
		>
			<span className="w-48 shrink-0 truncate text-sm font-semibold">
				{model.name}
			</span>
			<Badge
				variant="outline"
				className={`${typeBadgeClass(model.model_type)} shrink-0`}
			>
				{t(typeBadgeLabelKey(model.model_type))}
			</Badge>
			<span className="w-12 shrink-0 text-center text-xs font-mono text-muted-foreground">
				{model.scale != null ? `${model.scale}x` : t("common:notAvailable")}
			</span>
			<span className="w-12 shrink-0 flex justify-center">
				{model.is_fp16 ? (
					<Badge
						variant="outline"
						className="bg-purple-500/20 text-purple-400 border-purple-500/30"
					>
						FP16
					</Badge>
				) : (
					<span className="text-xs text-muted-foreground">
						{t("common:notAvailable")}
					</span>
				)}
			</span>
			<span className="w-24 shrink-0 truncate text-xs text-muted-foreground">
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

// ─── Empty state ─────────────────────────────────────────────────────────────

function EmptyState() {
	const { t } = useTranslation("models");
	return (
		<div className="flex flex-col items-center justify-center py-20 text-center">
			<Search className="size-10 text-muted-foreground/40 mb-4" />
			<p className="text-sm text-muted-foreground">{t("empty.noModels")}</p>
		</div>
	);
}

// ─── Error state ─────────────────────────────────────────────────────────────

function ErrorState({ message }: { message: string }) {
	return (
		<div className="flex flex-col items-center justify-center py-20 text-center">
			<p className="text-sm text-destructive">{message}</p>
		</div>
	);
}

// ─── Main Page ───────────────────────────────────────────────────────────────

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
			{/* Header */}
			<div className="mb-6">
				<h2 className="text-lg font-semibold tracking-tight">
					{t("page.title")}
				</h2>
				<p className="text-sm text-muted-foreground mt-0.5">{subtitle}</p>
			</div>

			{/* Filter bar */}
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:gap-4 mb-6">
				{/* Search */}
				<div className="relative flex-1 max-w-sm">
					<Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
					<Input
						placeholder={t("search.placeholder")}
						value={searchQuery}
						onChange={(e) => setSearchQuery(e.target.value)}
						className="pl-9"
					/>
				</div>

				{/* Type filter */}
				<div className="flex items-center gap-1">
					{TYPE_FILTERS.map((f) => (
						<Button
							key={f.value}
							variant={typeFilter === f.value ? "secondary" : "ghost"}
							size="sm"
							onClick={() => setTypeFilter(f.value)}
							className={
								typeFilter === f.value
									? "bg-secondary text-foreground"
									: "text-muted-foreground"
							}
						>
							<f.icon className="size-3.5 mr-1" />
							{t(f.labelKey)}
						</Button>
					))}
				</div>

				{/* View toggle */}
				<div className="flex items-center gap-1 sm:ml-auto">
					<Button
						variant={view === "grid" ? "secondary" : "ghost"}
						size="icon"
						onClick={() => setView("grid")}
						aria-label={t("view.grid")}
					>
						<LayoutGrid className="size-4" />
					</Button>
					<Button
						variant={view === "list" ? "secondary" : "ghost"}
						size="icon"
						onClick={() => setView("list")}
						aria-label={t("view.list")}
					>
						<List className="size-4" />
					</Button>
				</div>
			</div>

			{/* Content */}
			{loading ? (
				view === "grid" ? (
					<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
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
				<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
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
					{/* List header */}
					<div className="flex items-center gap-4 px-4 py-2 text-xs font-medium text-muted-foreground uppercase tracking-wider">
						<span className="w-48 shrink-0">{t("listHeaders.name")}</span>
						<span className="w-28 shrink-0">{t("listHeaders.type")}</span>
						<span className="w-12 shrink-0 text-center">
							{t("listHeaders.scale")}
						</span>
						<span className="w-12 shrink-0 text-center">FP16</span>
						<span className="w-24 shrink-0">{t("listHeaders.input")}</span>
						<span className="w-44 shrink-0">{t("listHeaders.filename")}</span>
						<span className="flex-1">{t("listHeaders.description")}</span>
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
