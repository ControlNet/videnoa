import { Check, Loader2, RotateCcw, Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getConfig, updateConfig } from "@/api/client";
import { PasswordSettings, SessionSettings } from "@/auth/AccessSettings";
import { toast } from "@/components/shared/Toaster";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { AppConfig } from "@/types";
import { DEFAULT_AUTH_CONFIG } from "@/types";
import { IrohSettings } from "./IrohSettings";
import {
	CheckRow,
	Field,
	SettingsGrid,
	SettingsSection,
	settingsInputClass,
} from "./section";

const SECTIONS = [
	{ id: "settings-storage", labelKey: "sections.paths.title" },
	{ id: "settings-password", labelKey: "sections.password.navLabel" },
	{ id: "settings-session", labelKey: "sections.session.navLabel" },
	{ id: "settings-remote", labelKey: "sections.remote.navLabel" },
	{ id: "settings-diagnostics", labelKey: "sections.performance.title" },
	{ id: "settings-server", labelKey: "sections.server.title" },
] as const;

const PATH_FIELDS = [
	{ key: "models_dir", id: "models-dir", labelKey: "sections.paths.fields.models" },
	{ key: "trt_cache_dir", id: "trt-cache-dir", labelKey: "sections.paths.fields.trtCache" },
	{ key: "presets_dir", id: "presets-dir", labelKey: "sections.paths.fields.presets" },
	{ key: "workflows_dir", id: "workflows-dir", labelKey: "sections.paths.fields.workflows" },
] as const;

export function SettingsPage() {
	const { t } = useTranslation(["settings", "common"]);
	const [config, setConfig] = useState<AppConfig | null>(null);
	const [formState, setFormState] = useState<AppConfig | null>(null);
	const [loading, setLoading] = useState(true);
	const [saving, setSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [saveSuccess, setSaveSuccess] = useState(false);

	const normalizeConfig = useCallback((data: AppConfig): AppConfig => {
		return {
			...data,
			auth: data.auth ?? { ...DEFAULT_AUTH_CONFIG },
			performance: {
				profiling_enabled: data.performance?.profiling_enabled ?? false,
			},
		};
	}, []);

	useEffect(() => {
		const reload = () => {
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
		};
		reload();
		window.addEventListener("videnoa-password-changed", reload);
		return () => {
			window.removeEventListener("videnoa-password-changed", reload);
		};
	}, [normalizeConfig, t]);

	const isDirty = useMemo(() => {
		if (!config || !formState) return false;
		return JSON.stringify(config) !== JSON.stringify(formState);
	}, [config, formState]);

	const handleSave = useCallback(async () => {
		if (!formState) return;
		const auth = formState.auth ?? DEFAULT_AUTH_CONFIG;
		if (
			!Number.isSafeInteger(auth.session_absolute_seconds) ||
			!Number.isSafeInteger(auth.session_idle_seconds) ||
			auth.session_idle_seconds < 1 ||
			auth.session_idle_seconds > auth.session_absolute_seconds
		) {
			setError(t("errors.invalidSessionLifetime"));
			return;
		}
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
			setTimeout(() => {
				setSaveSuccess(false);
			}, 3000);
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
			prev ? { ...prev, performance: { ...prev.performance, [key]: value } } : prev,
		);
	}

	const commandRow = (
		<div className="sticky top-0 z-30 flex h-12 shrink-0 items-center gap-3 border-b border-border bg-background px-[18px]">
			<h2 className="text-[15px] font-semibold tracking-tight">
				{t("page.title")}
			</h2>
			<span className="font-mono text-[11px] text-muted-foreground">
				{t("page.configPath")}
			</span>
		</div>
	);

	if (loading || !formState) {
		return (
			<div className="flex min-h-full flex-col">
				{commandRow}
				<div className="flex flex-1 items-center justify-center py-20">
					<Loader2 className="size-6 animate-spin text-muted-foreground" />
				</div>
			</div>
		);
	}

	return (
		<div className="flex min-h-full flex-col">
			{commandRow}

			<div className="grid flex-1 grid-cols-1 items-start sm:grid-cols-[10.5rem_minmax(0,1fr)]">
				{/* An index, so a long form is one click deep rather than one scroll. */}
				<nav
					aria-label={t("nav.label")}
					className="sticky top-12 hidden flex-col gap-px px-2 py-4 sm:flex"
				>
					{SECTIONS.map((section) => (
						<a
							key={section.id}
							href={`#${section.id}`}
							className="flex h-7 items-center rounded-md px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
						>
							{t(section.labelKey)}
						</a>
					))}
				</nav>

				<div className="flex min-w-0 flex-col gap-6 py-5 pl-4 pr-[18px] sm:border-l sm:border-border sm:pl-5">
					<SettingsSection
						id="settings-storage"
						title={t("sections.paths.title")}
						note={t("sections.paths.description")}
					>
						<SettingsGrid className="sm:grid-cols-4">
							{PATH_FIELDS.map((field) => (
								<Field
									key={field.key}
									id={field.id}
									label={t(field.labelKey)}
									hint={field.key}
								>
									<Input
										id={field.id}
										className={settingsInputClass}
										value={formState.paths[field.key]}
										onChange={(e) => {
											updatePaths(field.key, e.target.value);
										}}
									/>
								</Field>
							))}
						</SettingsGrid>
					</SettingsSection>

					<PasswordSettings />

					<SessionSettings
						config={formState.auth ?? DEFAULT_AUTH_CONFIG}
						onChange={(auth) => {
							setFormState((prev) => (prev ? { ...prev, auth } : prev));
						}}
					/>

					<IrohSettings
						enabled={formState.iroh?.enabled ?? false}
						savedEnabled={config?.iroh?.enabled ?? false}
						onChange={(enabled) => {
							setFormState({ ...formState, iroh: { enabled } });
						}}
					/>

					<SettingsSection
						id="settings-diagnostics"
						title={t("sections.performance.title")}
						note={t("sections.performance.description")}
					>
						<div className="flex flex-wrap items-center gap-x-2 gap-y-1">
							<CheckRow
								id="performance-profiling-enabled"
								label={t("sections.performance.fields.profilingEnabled")}
								checked={formState.performance.profiling_enabled}
								onChange={(event) => {
									updatePerformance("profiling_enabled", event.target.checked);
								}}
							/>
							<span className="text-[12.5px] text-muted-foreground">
								—{" "}
								{formState.performance.profiling_enabled
									? t("sections.performance.fields.enabled")
									: t("sections.performance.fields.disabled")}
							</span>
						</div>
						<p className="text-[11px] leading-4 text-muted-foreground">
							{t("sections.performance.fields.profilingEnabledHint")}
						</p>
					</SettingsSection>

					{/* Read-only information is text. It was two disabled inputs at 40%
					    effective opacity, which nobody could read. */}
					<SettingsSection
						id="settings-server"
						title={t("sections.server.title")}
						note={t("sections.server.description")}
					>
						<dl className="grid max-w-md grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-[12.5px]">
							<dt className="text-muted-foreground">
								{t("sections.server.fields.port")}
							</dt>
							<dd className="font-mono">{formState.server.port}</dd>
							<dt className="text-muted-foreground">
								{t("sections.server.fields.host")}
							</dt>
							<dd className="font-mono">{formState.server.host}</dd>
						</dl>
					</SettingsSection>
				</div>
			</div>

			{/* Pinned to the foot of the route: a long form never hides its commit. */}
			<div className="sticky bottom-0 z-30 flex min-h-[52px] shrink-0 flex-wrap items-center gap-3 border-t border-border bg-background-deep px-[18px] py-2">
				{isDirty && (
					<span className="flex items-center gap-2 text-xs text-muted-foreground">
						<span className="size-1.5 rounded-full bg-yellow-500" />
						{t("footer.unsavedChanges")}
					</span>
				)}
				{saveSuccess && (
					<span className="flex items-center gap-1.5 text-xs text-green-600 dark:text-green-400">
						<Check className="size-3.5" />
						{t("footer.saveSuccess")}
					</span>
				)}
				{error && <span className="text-xs text-destructive">{error}</span>}
				<span className="flex-1" />
				<Button
					variant="ghost"
					size="sm"
					onClick={() => void handleReset()}
					disabled={saving}
				>
					<RotateCcw className="size-3.5" />
					{t("actions.reset")}
				</Button>
				<Button
					size="sm"
					onClick={() => void handleSave()}
					disabled={saving || !isDirty}
				>
					{saving ? (
						<Loader2 className="size-3.5 animate-spin" />
					) : (
						<Save className="size-3.5" />
					)}
					{t("actions.save")}
				</Button>
			</div>
		</div>
	);
}
