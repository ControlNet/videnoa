import {
	Check,
	FolderOpen,
	Loader2,
	Lock,
	RotateCcw,
	Save,
	Server,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getConfig, updateConfig } from "@/api/client";
import { PageContainer } from "@/components/layout/PageContainer";
import { toast } from "@/components/shared/Toaster";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { AppConfig } from "@/types";

// ─── Field helpers ───────────────────────────────────────────────────────────

function FieldLabel({
	children,
	htmlFor,
}: {
	children: React.ReactNode;
	htmlFor?: string;
}) {
	return (
		<label htmlFor={htmlFor} className="text-sm font-medium text-foreground">
			{children}
		</label>
	);
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export function SettingsPage() {
	const { t } = useTranslation("settings");
	const [config, setConfig] = useState<AppConfig | null>(null);
	const [formState, setFormState] = useState<AppConfig | null>(null);
	const [loading, setLoading] = useState(true);
	const [saving, setSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [saveSuccess, setSaveSuccess] = useState(false);

	const normalizeConfig = useCallback((data: AppConfig): AppConfig => {
		return {
			...data,
			performance: {
				profiling_enabled: data.performance?.profiling_enabled ?? false,
			},
		};
	}, []);

	// Fetch on mount
	useEffect(() => {
		void (async () => {
			try {
				const data = await getConfig();
				const normalized = normalizeConfig(data);
				setConfig(normalized);
				setFormState(structuredClone(normalized));
			} catch (err) {
				setError(err instanceof Error ? err.message : t("errors.loadConfig"));
			} finally {
				setLoading(false);
			}
		})();
	}, [normalizeConfig, t]);

	// Dirty detection
	const isDirty = useMemo(() => {
		if (!config || !formState) return false;
		return JSON.stringify(config) !== JSON.stringify(formState);
	}, [config, formState]);

	// ─── Handlers ────────────────────────────────────────────────────────────────

	const handleSave = useCallback(async () => {
		if (!formState) return;
		setSaving(true);
		setSaveSuccess(false);
		setError(null);
		try {
			const updated = await updateConfig(formState);
			const normalized = normalizeConfig(updated);
			setConfig(normalized);
			setFormState(structuredClone(normalized));
			setSaveSuccess(true);
			toast.success(t("toast.saveSuccess"));
			setTimeout(() => setSaveSuccess(false), 3000);
		} catch (err) {
			setError(err instanceof Error ? err.message : t("errors.saveConfig"));
		} finally {
			setSaving(false);
		}
	}, [formState, normalizeConfig, t]);

	const handleReset = useCallback(async () => {
		setError(null);
		setSaveSuccess(false);
		setLoading(true);
		try {
			const data = await getConfig();
			const normalized = normalizeConfig(data);
			setConfig(normalized);
			setFormState(structuredClone(normalized));
		} catch (err) {
			setError(err instanceof Error ? err.message : t("errors.loadConfig"));
		} finally {
			setLoading(false);
		}
	}, [normalizeConfig, t]);

	// Updater helpers
	function updatePaths<K extends keyof AppConfig["paths"]>(
		key: K,
		value: AppConfig["paths"][K],
	) {
		setFormState((prev) =>
			prev ? { ...prev, paths: { ...prev.paths, [key]: value } } : prev,
		);
	}

	function updatePerformance<K extends keyof AppConfig["performance"]>(
		key: K,
		value: AppConfig["performance"][K],
	) {
		setFormState((prev) =>
			prev
				? {
						...prev,
						performance: { ...prev.performance, [key]: value },
				  }
				: prev,
		);
	}

	// ─── Loading state ───────────────────────────────────────────────────────────

	if (loading || !formState) {
		return (
			<PageContainer title={t("page.title")}>
				<p className="text-muted-foreground text-sm -mt-4 mb-6">
					{t("page.description")}
				</p>
				<div className="flex items-center justify-center py-20">
					<Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
				</div>
			</PageContainer>
		);
	}

	// ─── Render ──────────────────────────────────────────────────────────────────

	return (
		<PageContainer title={t("page.title")}>
			<p className="text-muted-foreground text-sm -mt-4 mb-6">
				{t("page.description")}
			</p>

			<div className="grid grid-cols-1 lg:grid-cols-2 gap-6 pb-24">
				{/* ── Paths Section ──────────────────────────────────────────────────── */}
				<Card>
					<CardHeader>
						<div className="flex items-center gap-2">
							<FolderOpen className="h-4 w-4 text-muted-foreground" />
							<CardTitle className="text-base">
								{t("sections.paths.title")}
							</CardTitle>
						</div>
						<CardDescription>{t("sections.paths.description")}</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="space-y-2">
							<FieldLabel htmlFor="models-dir">
								{t("sections.paths.fields.modelsDir")}
							</FieldLabel>
							<Input
								id="models-dir"
								value={formState.paths.models_dir}
								onChange={(e) => updatePaths("models_dir", e.target.value)}
								className="font-mono text-xs"
							/>
						</div>
						<div className="space-y-2">
							<FieldLabel htmlFor="trt-cache-dir">
								{t("sections.paths.fields.trtCacheDir")}
							</FieldLabel>
							<Input
								id="trt-cache-dir"
								value={formState.paths.trt_cache_dir}
								onChange={(e) => updatePaths("trt_cache_dir", e.target.value)}
								className="font-mono text-xs"
							/>
						</div>
						<div className="space-y-2">
							<FieldLabel htmlFor="presets-dir">
								{t("sections.paths.fields.presetsDir")}
							</FieldLabel>
							<Input
								id="presets-dir"
								value={formState.paths.presets_dir}
								onChange={(e) => updatePaths("presets_dir", e.target.value)}
								className="font-mono text-xs"
							/>
						</div>
						<div className="space-y-2">
							<FieldLabel htmlFor="workflows-dir">
								{t("sections.paths.fields.workflowsDir")}
							</FieldLabel>
							<Input
								id="workflows-dir"
								value={formState.paths.workflows_dir}
								onChange={(e) => updatePaths("workflows_dir", e.target.value)}
								className="font-mono text-xs"
							/>
						</div>
					</CardContent>
				</Card>

				{/* ── Server Section (read-only) ─────────────────────────────────────── */}
				<Card className="opacity-80">
					<CardHeader>
						<div className="flex items-center gap-2">
							<Server className="h-4 w-4 text-muted-foreground" />
							<CardTitle className="text-base">
								{t("sections.server.title")}
							</CardTitle>
							<Badge variant="secondary" className="ml-auto text-[10px]">
								<Lock className="h-3 w-3 mr-1" />
								{t("sections.server.readOnlyBadge")}
							</Badge>
						</div>
						<CardDescription>
							{t("sections.server.description")}
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="space-y-2">
							<FieldLabel htmlFor="server-port">
								{t("sections.server.fields.port")}
							</FieldLabel>
							<Input
								id="server-port"
								type="number"
								value={formState.server.port}
								disabled
								className="font-mono text-xs"
							/>
						</div>
						<div className="space-y-2">
							<FieldLabel htmlFor="server-host">
								{t("sections.server.fields.host")}
							</FieldLabel>
							<Input
								id="server-host"
								value={formState.server.host}
								disabled
								className="font-mono text-xs"
							/>
						</div>
					</CardContent>
				</Card>

				<Card>
					<CardHeader>
						<div className="flex items-center gap-2">
							<Server className="h-4 w-4 text-muted-foreground" />
							<CardTitle className="text-base">
								{t("sections.performance.title")}
							</CardTitle>
						</div>
						<CardDescription>
							{t("sections.performance.description")}
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-2">
						<FieldLabel htmlFor="performance-profiling-enabled">
							{t("sections.performance.fields.profilingEnabled")}
						</FieldLabel>
						<label
							htmlFor="performance-profiling-enabled"
							className="flex items-center gap-3 rounded-md border border-border/60 bg-background/50 px-3 py-2"
						>
							<input
								id="performance-profiling-enabled"
								type="checkbox"
								checked={formState.performance.profiling_enabled}
								onChange={(event) =>
									updatePerformance("profiling_enabled", event.target.checked)
								}
								className="h-4 w-4 rounded border-border"
							/>
							<span className="text-sm text-foreground">
								{formState.performance.profiling_enabled
									? t("sections.performance.fields.enabled")
									: t("sections.performance.fields.disabled")}
							</span>
						</label>
						<p className="text-xs text-muted-foreground">
							{t("sections.performance.fields.profilingEnabledHint")}
						</p>
					</CardContent>
				</Card>
			</div>

			{/* ── Action footer ──────────────────────────────────────────────────── */}
			<div className="fixed bottom-0 left-0 right-0 z-40 border-t border-border bg-background/80 backdrop-blur-sm">
				<div className="flex items-center justify-between px-6 py-3 max-w-screen-xl mx-auto">
					<div className="flex items-center gap-3 min-h-[32px]">
						{isDirty && (
							<Badge
								variant="outline"
								className="text-yellow-400 border-yellow-500/40 bg-yellow-500/10"
							>
								{t("footer.unsavedChanges")}
							</Badge>
						)}
						{saveSuccess && (
							<span className="flex items-center gap-1.5 text-xs text-green-400">
								<Check className="h-3.5 w-3.5" />
								{t("footer.saveSuccess")}
							</span>
						)}
						{error && <span className="text-xs text-destructive">{error}</span>}
					</div>
					<div className="flex items-center gap-2">
						<Button
							variant="secondary"
							size="sm"
							onClick={() => void handleReset()}
							disabled={saving}
						>
							<RotateCcw className="h-3.5 w-3.5" />
							{t("actions.reset")}
						</Button>
						<Button
							size="sm"
							onClick={() => void handleSave()}
							disabled={saving || !isDirty}
						>
							{saving ? (
								<Loader2 className="h-3.5 w-3.5 animate-spin" />
							) : (
								<Save className="h-3.5 w-3.5" />
							)}
							{t("actions.save")}
						</Button>
					</div>
				</div>
			</div>
		</PageContainer>
	);
}
