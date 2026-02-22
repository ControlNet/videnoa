import type * as React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { extractFrames, processFrame } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ScrollArea, ScrollBar } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import type { FrameInfo } from "@/types";

// ─── Spinner ─────────────────────────────────────────────────────────────────

function Spinner({
	className = "",
	ariaLabel = "Loading",
}: {
	className?: string;
	ariaLabel?: string;
}) {
	return (
		<svg
			className={`animate-spin ${className}`}
			xmlns="http://www.w3.org/2000/svg"
			fill="none"
			viewBox="0 0 24 24"
			role="img"
			aria-label={ariaLabel}
		>
			<circle
				className="opacity-25"
				cx="12"
				cy="12"
				r="10"
				stroke="currentColor"
				strokeWidth="4"
			/>
			<path
				className="opacity-75"
				fill="currentColor"
				d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
			/>
		</svg>
	);
}

// ─── Zoom helpers ────────────────────────────────────────────────────────────

const MIN_ZOOM = 1;
const MAX_ZOOM = 8;
const ZOOM_STEP = 0.25;

function clampZoom(z: number): number {
	return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));
}

// ─── Slider Mode ─────────────────────────────────────────────────────────────

function SliderMode({
	beforeUrl,
	afterUrl,
	zoom,
	panX,
	panY,
	sliderPosition,
	onSliderChange,
	beforeLabel,
	afterLabel,
	emptyStateText,
}: {
	beforeUrl: string;
	afterUrl: string | null;
	zoom: number;
	panX: number;
	panY: number;
	sliderPosition: number;
	onSliderChange: (pos: number) => void;
	beforeLabel: string;
	afterLabel: string;
	emptyStateText: string;
}) {
	const containerRef = useRef<HTMLDivElement>(null);
	const dragging = useRef(false);

	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			dragging.current = true;
			(e.target as HTMLElement).setPointerCapture(e.pointerId);
			const rect = containerRef.current?.getBoundingClientRect();
			if (rect) {
				const x = ((e.clientX - rect.left) / rect.width) * 100;
				onSliderChange(Math.min(100, Math.max(0, x)));
			}
		},
		[onSliderChange],
	);

	const handlePointerMove = useCallback(
		(e: React.PointerEvent) => {
			if (!dragging.current) return;
			const rect = containerRef.current?.getBoundingClientRect();
			if (rect) {
				const x = ((e.clientX - rect.left) / rect.width) * 100;
				onSliderChange(Math.min(100, Math.max(0, x)));
			}
		},
		[onSliderChange],
	);

	const handlePointerUp = useCallback(() => {
		dragging.current = false;
	}, []);

	const imgTransform = `scale(${String(zoom)}) translate(${String(panX)}px, ${String(panY)}px)`;

	return (
		<div
			ref={containerRef}
			className="relative h-full w-full select-none overflow-hidden rounded-lg bg-black/40"
			onPointerDown={handlePointerDown}
			onPointerMove={handlePointerMove}
			onPointerUp={handlePointerUp}
		>
			{/* Before image (left portion) */}
			<div
				className="absolute inset-0"
				style={{ clipPath: `inset(0 ${String(100 - sliderPosition)}% 0 0)` }}
			>
				<img
					src={beforeUrl}
					alt={beforeLabel}
					className="h-full w-full object-contain"
					style={{ transform: imgTransform, transformOrigin: "center center" }}
					draggable={false}
				/>
			</div>

			{/* After image (right portion) */}
			{afterUrl ? (
				<div
					className="absolute inset-0"
					style={{ clipPath: `inset(0 0 0 ${String(sliderPosition)}%)` }}
				>
					<img
						src={afterUrl}
						alt={afterLabel}
						className="h-full w-full object-contain"
						style={{
							transform: imgTransform,
							transformOrigin: "center center",
						}}
						draggable={false}
					/>
				</div>
			) : (
				<div
					className="absolute inset-0 flex items-center justify-center"
					style={{ clipPath: `inset(0 0 0 ${String(sliderPosition)}%)` }}
				>
					<span className="text-muted-foreground text-sm">
						{emptyStateText}
					</span>
				</div>
			)}

			{/* Divider line */}
			<div
				className="absolute top-0 bottom-0 z-10 w-0.5 bg-foreground/80"
				style={{
					left: `${String(sliderPosition)}%`,
					transform: "translateX(-50%)",
				}}
			>
				{/* Grip circle */}
				<div className="absolute top-1/2 left-1/2 flex h-8 w-8 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border-2 border-foreground/80 bg-background/90 shadow-lg">
					<div className="flex gap-0.5">
						<div className="h-3 w-0.5 rounded-full bg-foreground/60" />
						<div className="h-3 w-0.5 rounded-full bg-foreground/60" />
					</div>
				</div>
			</div>

			{/* Labels */}
			<div className="pointer-events-none absolute top-3 left-3 z-10">
				<Badge
					variant="secondary"
					className="bg-background/80 backdrop-blur-sm"
				>
					{beforeLabel}
				</Badge>
			</div>
			<div className="pointer-events-none absolute top-3 right-3 z-10">
				<Badge
					variant="secondary"
					className="bg-background/80 backdrop-blur-sm"
				>
					{afterLabel}
				</Badge>
			</div>
		</div>
	);
}

// ─── Side-by-Side Mode ───────────────────────────────────────────────────────

function SideBySideMode({
	beforeUrl,
	afterUrl,
	zoom,
	panX,
	panY,
	beforeLabel,
	afterLabel,
	emptyStateText,
}: {
	beforeUrl: string;
	afterUrl: string | null;
	zoom: number;
	panX: number;
	panY: number;
	beforeLabel: string;
	afterLabel: string;
	emptyStateText: string;
}) {
	const imgTransform = `scale(${String(zoom)}) translate(${String(panX)}px, ${String(panY)}px)`;

	return (
		<div className="flex h-full w-full gap-1 overflow-hidden rounded-lg">
			{/* Before panel */}
			<div className="relative flex-1 overflow-hidden bg-black/40">
				<div className="pointer-events-none absolute top-3 left-3 z-10">
					<Badge
						variant="secondary"
						className="bg-background/80 backdrop-blur-sm"
					>
						{beforeLabel}
					</Badge>
				</div>
				<img
					src={beforeUrl}
					alt={beforeLabel}
					className="h-full w-full object-contain"
					style={{ transform: imgTransform, transformOrigin: "center center" }}
					draggable={false}
				/>
			</div>

			{/* Divider */}
			<Separator orientation="vertical" className="bg-border/50" />

			{/* After panel */}
			<div className="relative flex-1 overflow-hidden bg-black/40">
				<div className="pointer-events-none absolute top-3 left-3 z-10">
					<Badge
						variant="secondary"
						className="bg-background/80 backdrop-blur-sm"
					>
						{afterLabel}
					</Badge>
				</div>
				{afterUrl ? (
					<img
						src={afterUrl}
						alt={afterLabel}
						className="h-full w-full object-contain"
						style={{
							transform: imgTransform,
							transformOrigin: "center center",
						}}
						draggable={false}
					/>
				) : (
					<div className="flex h-full items-center justify-center">
						<span className="text-muted-foreground text-sm">
							{emptyStateText}
						</span>
					</div>
				)}
			</div>
		</div>
	);
}

// ─── Overlay Mode ────────────────────────────────────────────────────────────

function OverlayMode({
	beforeUrl,
	afterUrl,
	zoom,
	panX,
	panY,
	overlayOpacity,
	onOpacityChange,
	beforeLabel,
	afterLabel,
}: {
	beforeUrl: string;
	afterUrl: string | null;
	zoom: number;
	panX: number;
	panY: number;
	overlayOpacity: number;
	onOpacityChange: (opacity: number) => void;
	beforeLabel: string;
	afterLabel: string;
}) {
	const imgTransform = `scale(${String(zoom)}) translate(${String(panX)}px, ${String(panY)}px)`;

	return (
		<div className="flex h-full flex-col gap-2">
			<div className="relative flex-1 overflow-hidden rounded-lg bg-black/40">
				{/* Before image (base layer) */}
				<img
					src={beforeUrl}
					alt={beforeLabel}
					className="absolute inset-0 h-full w-full object-contain transition-opacity duration-200"
					style={{
						transform: imgTransform,
						transformOrigin: "center center",
						opacity: 1 - overlayOpacity,
					}}
					draggable={false}
				/>

				{/* After image (overlay layer) */}
				{afterUrl ? (
					<img
						src={afterUrl}
						alt={afterLabel}
						className="absolute inset-0 h-full w-full object-contain transition-opacity duration-200"
						style={{
							transform: imgTransform,
							transformOrigin: "center center",
							opacity: overlayOpacity,
						}}
						draggable={false}
					/>
				) : null}

				{/* Label */}
				<div className="pointer-events-none absolute top-3 left-3 z-10">
					<Badge
						variant="secondary"
						className="bg-background/80 backdrop-blur-sm"
					>
						{overlayOpacity < 0.5 ? beforeLabel : afterLabel}
					</Badge>
				</div>
			</div>

			{/* Opacity slider */}
			<div className="flex items-center gap-3 px-1">
				<span className="text-muted-foreground w-12 text-xs">
					{beforeLabel}
				</span>
				<input
					type="range"
					min="0"
					max="1"
					step="0.01"
					value={overlayOpacity}
					onChange={(e) => {
						onOpacityChange(parseFloat(e.target.value));
					}}
					className="h-1.5 flex-1 cursor-pointer appearance-none rounded-full bg-secondary [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-primary [&::-webkit-slider-thumb]:shadow-md [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:appearance-none [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-primary [&::-moz-range-thumb]:shadow-md [&::-moz-range-thumb]:border-0 [&::-moz-range-track]:rounded-full [&::-moz-range-track]:bg-secondary"
				/>
				<span className="text-muted-foreground w-12 text-right text-xs">
					{afterLabel}
				</span>
			</div>
		</div>
	);
}

// ─── ComparisonViewer ────────────────────────────────────────────────────────

export function ComparisonViewer() {
	const { t } = useTranslation("preview");
	const activeModal = useUIStore((s) => s.activeModal);
	const closeModal = useUIStore((s) => s.closeModal);
	const isOpen = activeModal === "preview";

	// ── State ──────────────────────────────────────────────────────────────────
	const [videoPath, setVideoPath] = useState("");
	const [frames, setFrames] = useState<FrameInfo[]>([]);
	const [previewId, setPreviewId] = useState<string | null>(null);
	const [selectedFrame, setSelectedFrame] = useState(0);
	const [processedUrl, setProcessedUrl] = useState<string | null>(null);
	const [zoom, setZoom] = useState(1);
	const [panX, setPanX] = useState(0);
	const [panY, setPanY] = useState(0);
	const [sliderPosition, setSliderPosition] = useState(50);
	const [overlayOpacity, setOverlayOpacity] = useState(0);
	const [loading, setLoading] = useState(false);
	const [processing, setProcessing] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const viewerRef = useRef<HTMLDivElement>(null);
	const isPanning = useRef(false);
	const lastMouse = useRef({ x: 0, y: 0 });

	// ── Frame selection ────────────────────────────────────────────────────────

	const selectFrame = useCallback(
		(index: number) => {
			if (index >= 0 && index < frames.length) {
				setSelectedFrame(index);
				setProcessedUrl(null);
			}
		},
		[frames.length],
	);

	// ── Keyboard navigation ────────────────────────────────────────────────────

	useEffect(() => {
		if (!isOpen) return;

		function handleKeyDown(e: KeyboardEvent) {
			if (e.key === "ArrowLeft") {
				e.preventDefault();
				setSelectedFrame((prev) => {
					const next = Math.max(0, prev - 1);
					if (next !== prev) setProcessedUrl(null);
					return next;
				});
			} else if (e.key === "ArrowRight") {
				e.preventDefault();
				setSelectedFrame((prev) => {
					const next = Math.min(frames.length - 1, prev + 1);
					if (next !== prev) setProcessedUrl(null);
					return next;
				});
			}
		}

		window.addEventListener("keydown", handleKeyDown);
		return () => {
			window.removeEventListener("keydown", handleKeyDown);
		};
	}, [isOpen, frames.length]);

	// ── Zoom via mouse wheel ───────────────────────────────────────────────────

	const handleWheel = useCallback((e: React.WheelEvent) => {
		e.preventDefault();
		const direction = e.deltaY < 0 ? 1 : -1;
		setZoom((prev) => clampZoom(prev + direction * ZOOM_STEP));
	}, []);

	// ── Pan via mouse drag ─────────────────────────────────────────────────────

	const handleMouseDown = useCallback(
		(e: React.MouseEvent) => {
			if (zoom <= 1) return;
			// Only pan with middle-click or when zoomed with left-click on the viewer area
			if (e.button === 0 || e.button === 1) {
				isPanning.current = true;
				lastMouse.current = { x: e.clientX, y: e.clientY };
			}
		},
		[zoom],
	);

	const handleMouseMove = useCallback(
		(e: React.MouseEvent) => {
			if (!isPanning.current) return;
			const dx = e.clientX - lastMouse.current.x;
			const dy = e.clientY - lastMouse.current.y;
			lastMouse.current = { x: e.clientX, y: e.clientY };
			setPanX((prev) => prev + dx / zoom);
			setPanY((prev) => prev + dy / zoom);
		},
		[zoom],
	);

	const handleMouseUp = useCallback(() => {
		isPanning.current = false;
	}, []);

	// ── Reset zoom ─────────────────────────────────────────────────────────────

	const resetZoom = useCallback(() => {
		setZoom(1);
		setPanX(0);
		setPanY(0);
	}, []);

	// ── Extract frames ─────────────────────────────────────────────────────────

	const handleExtract = useCallback(async () => {
		if (!videoPath.trim()) return;
		setLoading(true);
		setError(null);
		setFrames([]);
		setPreviewId(null);
		setProcessedUrl(null);
		setSelectedFrame(0);

		try {
			const result = await extractFrames(videoPath.trim(), 10);
			setPreviewId(result.preview_id);
			setFrames(result.frames);
		} catch (err) {
			setError(err instanceof Error ? err.message : t("errors.extractFrames"));
		} finally {
			setLoading(false);
		}
	}, [t, videoPath]);

	// ── Process frame ──────────────────────────────────────────────────────────

	const handleProcess = useCallback(async () => {
		if (!previewId) return;
		setProcessing(true);
		setError(null);

		try {
			const workflow = useWorkflowStore.getState().exportWorkflow();
			const result = await processFrame(previewId, selectedFrame, workflow);
			setProcessedUrl(result.processed_url);
		} catch (err) {
			setError(err instanceof Error ? err.message : t("errors.processFrame"));
		} finally {
			setProcessing(false);
		}
	}, [previewId, selectedFrame, t]);

	// ── Current before URL ─────────────────────────────────────────────────────

	const currentFrame = frames[selectedFrame];
	const beforeUrl = currentFrame?.url ?? "";

	return (
		<Dialog
			open={isOpen}
			onOpenChange={(open) => {
				if (!open) closeModal();
			}}
		>
			<DialogContent className="flex max-w-5xl flex-col gap-0 overflow-hidden p-0 sm:max-h-[92vh] sm:min-h-[80vh]">
				{/* ── Header ────────────────────────────────────────────────────── */}
				<DialogHeader className="space-y-0 border-b border-border px-5 py-3.5">
					<div className="flex items-center justify-between">
						<div>
							<DialogTitle className="text-base">
								{t("dialog.title")}
							</DialogTitle>
							<DialogDescription className="text-muted-foreground text-xs mt-0.5">
								{t("dialog.description")}
							</DialogDescription>
						</div>
						{frames.length > 0 && (
							<div className="flex items-center gap-2">
								<Badge variant="outline" className="font-mono text-xs">
									{`${String(Math.round(zoom * 100))}%`}
								</Badge>
								{zoom > 1 && (
									<Button variant="ghost" size="sm" onClick={resetZoom}>
										{t("actions.fit")}
									</Button>
								)}
							</div>
						)}
					</div>
				</DialogHeader>

				{/* ── Video path input ──────────────────────────────────────────── */}
				<div className="flex gap-2 border-b border-border px-5 py-3">
					<Input
						placeholder={t("inputs.videoPathPlaceholder")}
						value={videoPath}
						onChange={(e) => {
							setVideoPath(e.target.value);
						}}
						onKeyDown={(e) => {
							if (e.key === "Enter") void handleExtract();
						}}
						disabled={loading}
						className="flex-1"
					/>
					<Button
						onClick={() => {
							void handleExtract();
						}}
						disabled={loading || !videoPath.trim()}
						size="sm"
					>
						{loading ? (
							<>
								<Spinner
									className="size-3.5"
									ariaLabel={t("spinner.loadingAriaLabel")}
								/>
								{t("actions.extracting")}
							</>
						) : (
							t("actions.extractFrames")
						)}
					</Button>
				</div>

				{/* ── Error message ─────────────────────────────────────────────── */}
				{error && (
					<div className="mx-5 mt-2 rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">
						{error}
					</div>
				)}

				{/* ── Comparison area ───────────────────────────────────────────── */}
				{frames.length > 0 ? (
					<div className="flex flex-1 flex-col gap-0 overflow-hidden">
						<Tabs
							defaultValue="slider"
							className="flex flex-1 flex-col overflow-hidden px-5 pt-3"
						>
							<div className="mb-2 flex items-center justify-between">
								<TabsList>
									<TabsTrigger value="slider">{t("tabs.slider")}</TabsTrigger>
									<TabsTrigger value="side-by-side">
										{t("tabs.sideBySide")}
									</TabsTrigger>
									<TabsTrigger value="overlay">{t("tabs.overlay")}</TabsTrigger>
								</TabsList>

								<Button
									onClick={() => {
										void handleProcess();
									}}
									disabled={processing || !previewId}
									size="sm"
								>
									{processing ? (
										<>
											<Spinner
												className="size-3.5"
												ariaLabel={t("spinner.loadingAriaLabel")}
											/>
											{t("actions.processing")}
										</>
									) : (
										t("actions.processFrame")
									)}
								</Button>
							</div>

							{/* Viewer container with zoom/pan events */}
							<div
								ref={viewerRef}
								role="application"
								className="relative flex-1 overflow-hidden"
								onWheel={handleWheel}
								onMouseDown={handleMouseDown}
								onMouseMove={handleMouseMove}
								onMouseUp={handleMouseUp}
								onMouseLeave={handleMouseUp}
								style={{
									cursor:
										zoom > 1
											? isPanning.current
												? "grabbing"
												: "grab"
											: "default",
								}}
							>
								{/* Processing overlay */}
								{processing && (
									<div className="absolute inset-0 z-20 flex items-center justify-center bg-background/60 backdrop-blur-sm">
										<div className="flex flex-col items-center gap-2">
											<Spinner
												className="size-8 text-primary"
												ariaLabel={t("spinner.loadingAriaLabel")}
											/>
											<span className="text-muted-foreground text-sm">
												{t("overlay.processingFrame")}
											</span>
										</div>
									</div>
								)}

								<TabsContent value="slider" className="mt-0 h-full">
									{beforeUrl && (
										<SliderMode
											beforeUrl={beforeUrl}
											afterUrl={processedUrl}
											zoom={zoom}
											panX={panX}
											panY={panY}
											sliderPosition={sliderPosition}
											onSliderChange={setSliderPosition}
											beforeLabel={t("label.before")}
											afterLabel={t("label.after")}
											emptyStateText={t("empty.processToCompare")}
										/>
									)}
								</TabsContent>

								<TabsContent value="side-by-side" className="mt-0 h-full">
									{beforeUrl && (
										<SideBySideMode
											beforeUrl={beforeUrl}
											afterUrl={processedUrl}
											zoom={zoom}
											panX={panX}
											panY={panY}
											beforeLabel={t("label.before")}
											afterLabel={t("label.after")}
											emptyStateText={t("empty.processToCompare")}
										/>
									)}
								</TabsContent>

								<TabsContent value="overlay" className="mt-0 h-full">
									{beforeUrl && (
										<OverlayMode
											beforeUrl={beforeUrl}
											afterUrl={processedUrl}
											zoom={zoom}
											panX={panX}
											panY={panY}
											overlayOpacity={overlayOpacity}
											onOpacityChange={setOverlayOpacity}
											beforeLabel={t("label.before")}
											afterLabel={t("label.after")}
										/>
									)}
								</TabsContent>
							</div>
						</Tabs>

						{/* ── Frame thumbnails ─────────────────────────────────────────── */}
						<div className="border-t border-border px-5 py-2.5">
							<div className="mb-1.5 flex items-center justify-between">
								<span className="text-muted-foreground text-xs font-medium">
									{t("thumbnails.frameCounter", {
										current: selectedFrame + 1,
										total: frames.length,
									})}
								</span>
								<span className="text-muted-foreground text-xs">
									{t("thumbnails.keyboardHint")}
								</span>
							</div>
							<ScrollArea className="w-full">
								<div className="flex gap-1.5 pb-1">
									{frames.map((frame, i) => (
										<button
											key={frame.index}
											type="button"
											onClick={() => {
												selectFrame(i);
											}}
											className={`relative flex-shrink-0 overflow-hidden rounded-md border-2 transition-all ${
												i === selectedFrame
													? "border-primary shadow-sm shadow-primary/20"
													: "border-transparent opacity-60 hover:opacity-90"
											}`}
										>
											<img
												src={frame.url}
												alt={t("thumbnails.frameAlt", { index: frame.index })}
												className="h-12 w-16 object-cover"
												draggable={false}
											/>
											<span className="absolute bottom-0 left-0 right-0 bg-black/60 text-center text-[9px] leading-tight text-white/80">
												{String(frame.index)}
											</span>
										</button>
									))}
								</div>
								<ScrollBar orientation="horizontal" />
							</ScrollArea>
						</div>
					</div>
				) : (
					/* ── Empty state ───────────────────────────────────────────────── */
					<div className="flex flex-1 items-center justify-center p-12">
						<div className="flex flex-col items-center gap-3 text-center">
							{loading ? (
								<>
									<Spinner
										className="size-10 text-primary"
										ariaLabel={t("spinner.loadingAriaLabel")}
									/>
									<span className="text-muted-foreground text-sm">
										{t("empty.extractingFrames")}
									</span>
								</>
							) : (
								<>
									<div className="flex size-14 items-center justify-center rounded-xl bg-muted">
										<svg
											className="size-7 text-muted-foreground"
											xmlns="http://www.w3.org/2000/svg"
											fill="none"
											viewBox="0 0 24 24"
											strokeWidth={1.5}
											stroke="currentColor"
											role="img"
											aria-label={t("empty.imagePlaceholderAriaLabel")}
										>
											<path
												strokeLinecap="round"
												strokeLinejoin="round"
												d="m2.25 15.75 5.159-5.159a2.25 2.25 0 0 1 3.182 0l5.159 5.159m-1.5-1.5 1.409-1.409a2.25 2.25 0 0 1 3.182 0l2.909 2.909M3.75 21h16.5A2.25 2.25 0 0 0 22.5 18.75V5.25A2.25 2.25 0 0 0 20.25 3H3.75A2.25 2.25 0 0 0 1.5 5.25v13.5A2.25 2.25 0 0 0 3.75 21Z"
											/>
										</svg>
									</div>
									<div>
										<p className="text-sm font-medium text-foreground">
											{t("empty.noFramesTitle")}
										</p>
										<p className="text-muted-foreground mt-0.5 text-xs">
											{t("empty.noFramesDescription")}
										</p>
									</div>
								</>
							)}
						</div>
					</div>
				)}
			</DialogContent>
		</Dialog>
	);
}
