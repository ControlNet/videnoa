import {
	Gauge,
	Globe,
	Minus,
	Monitor,
	Moon,
	Package,
	Play,
	Settings,
	Square,
	Sun,
	Workflow,
	X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink } from "react-router";
import { healthCheck, listJobs } from "@/api/client";
import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuRadioGroup,
	DropdownMenuRadioItem,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import {
	FALLBACK_LOCALE,
	isSupportedLocale,
	SUPPORTED_LOCALES,
	type SupportedLocale,
} from "@/i18n/locales/types";
import { createDesktopWindowController } from "@/lib/runtime-desktop";
import { cn } from "@/lib/utils";
import { type ThemeMode, useUIStore } from "@/stores/ui-store";

const navItems = [
	{ to: "/", labelKey: "header.nav.editor", icon: Workflow },
	{ to: "/jobs", labelKey: "header.nav.jobs", icon: Play },
	{ to: "/models", labelKey: "header.nav.models", icon: Package },
	{ to: "/performance", labelKey: "header.nav.performance", icon: Gauge },
	{ to: "/settings", labelKey: "header.nav.settings", icon: Settings },
] as const;

const themeCycle: ThemeMode[] = ["system", "light", "dark"];
const themeIcons: Record<ThemeMode, typeof Sun> = {
	system: Monitor,
	light: Sun,
	dark: Moon,
};
const themeLabelKeys: Record<ThemeMode, string> = {
	system: "header.theme.system",
	light: "header.theme.light",
	dark: "header.theme.dark",
};

type ServerStatus = "offline" | "busy" | "idle";

const PROJECT_REPO_URL = "https://github.com/ControlNet/videnoa";

const serverStatusLabelKeys: Record<ServerStatus, string> = {
	offline: "header.server.offline",
	busy: "header.server.busy",
	idle: "header.server.idle",
};

const serverStatusOuterClass: Record<ServerStatus, string> = {
	offline: "bg-red-500/18 border-red-400/45",
	busy: "bg-yellow-500/20 border-yellow-400/50",
	idle: "bg-green-500/20 border-green-400/50",
};

const serverStatusInnerClass: Record<ServerStatus, string> = {
	offline: "bg-red-400 shadow-[0_0_8px_rgba(248,113,113,0.55)]",
	busy: "bg-yellow-300 animate-pulse shadow-[0_0_10px_rgba(253,224,71,0.65)]",
	idle: "bg-green-400 shadow-[0_0_9px_rgba(74,222,128,0.65)]",
};

function resolveActiveLocale(locale: string): SupportedLocale {
	return isSupportedLocale(locale) ? locale : FALLBACK_LOCALE;
}

export function Header() {
	const { t, i18n } = useTranslation("common");
	const theme = useUIStore((s) => s.theme);
	const setTheme = useUIStore((s) => s.setTheme);
	const [serverStatus, setServerStatus] = useState<ServerStatus>("offline");
	const desktopWindowController = createDesktopWindowController();
	const desktopRuntime = desktopWindowController.isDesktop;
	const activeLocale = resolveActiveLocale(
		i18n.resolvedLanguage ?? i18n.language,
	);
	const localeOptions = SUPPORTED_LOCALES.map((locale) => ({
		value: locale,
		autonym: t(`header.locale.label.${locale}`, { lng: locale }),
	}));
	const currentLocaleAutonym =
		localeOptions.find(({ value }) => value === activeLocale)?.autonym ??
		activeLocale;

	const nextTheme =
		themeCycle[(themeCycle.indexOf(theme) + 1) % themeCycle.length];
	const Icon = themeIcons[theme];
	const currentThemeLabel = t(themeLabelKeys[theme]);
	const nextThemeLabel = t(themeLabelKeys[nextTheme]);
	const themeToggleButton = (
		<TooltipProvider delayDuration={200}>
			<Tooltip>
				<TooltipTrigger asChild>
					<Button
						variant="ghost"
						size="sm"
						onClick={() => {
							setTheme(nextTheme);
						}}
						className="h-8 w-8 p-0"
						aria-label={t("header.theme.toggleAriaLabel", {
							current: currentThemeLabel,
							next: nextThemeLabel,
						})}
						title={t("header.theme.toggleAriaLabel", {
							current: currentThemeLabel,
							next: nextThemeLabel,
						})}
					>
						<Icon className="h-4 w-4" />
					</Button>
				</TooltipTrigger>
				<TooltipContent>
					{t("header.theme.toggleTooltip", {
						current: currentThemeLabel,
						next: nextThemeLabel,
					})}
				</TooltipContent>
			</Tooltip>
		</TooltipProvider>
	);

	const localeToggleButton = (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button
					variant="ghost"
					size="sm"
					className="h-8 w-8 p-0"
					aria-label={t("header.locale.menuAriaLabel")}
					title={t("header.locale.menuTooltip", {
						current: currentLocaleAutonym,
					})}
				>
					<Globe className="h-4 w-4" />
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" className="min-w-40">
				<DropdownMenuRadioGroup
					value={activeLocale}
					onValueChange={(value) => {
						if (!isSupportedLocale(value)) return;
						void i18n.changeLanguage(value);
					}}
				>
					{localeOptions.map(({ value, autonym }) => (
						<DropdownMenuRadioItem key={value} value={value}>
							{autonym}
						</DropdownMenuRadioItem>
					))}
				</DropdownMenuRadioGroup>
			</DropdownMenuContent>
		</DropdownMenu>
	);

	useEffect(() => {
		let disposed = false;

		const refreshServerStatus = async () => {
			try {
				await healthCheck();
			} catch {
				if (!disposed) setServerStatus("offline");
				return;
			}

			try {
				const jobs = await listJobs();
				const hasRunningWorkflow = jobs.some((job) => job.status === "running");
				if (!disposed) {
					setServerStatus(hasRunningWorkflow ? "busy" : "idle");
				}
			} catch {
				if (!disposed) setServerStatus("idle");
			}
		};

		void refreshServerStatus();
		const timer = window.setInterval(() => {
			void refreshServerStatus();
		}, 5000);

		return () => {
			disposed = true;
			window.clearInterval(timer);
		};
	}, []);

	const serverStatusLabel = t(serverStatusLabelKeys[serverStatus]);

	return (
		<header
			className="flex items-center h-12 px-4 border-b border-border shrink-0 bg-background"
			data-desktop-runtime={desktopRuntime ? "true" : "false"}
		>
			<div className="flex items-center gap-2.5 mr-6">
				<div
					className={cn(
						"h-6 w-6 rounded-lg border flex items-center justify-center shrink-0 transition-colors",
						serverStatusOuterClass[serverStatus],
					)}
					title={serverStatusLabel}
				>
					<div
						className={cn(
							"h-2 w-2 rounded-full transition-colors",
							serverStatusInnerClass[serverStatus],
						)}
					/>
				</div>
				<a
					href={PROJECT_REPO_URL}
					target="_blank"
					rel="noopener noreferrer"
					className="text-sm font-semibold text-foreground whitespace-nowrap hover:text-primary transition-colors"
					style={{ fontFamily: "'Geist Mono', monospace" }}
				>
					Videnoa
				</a>
			</div>

			<nav className="flex items-center gap-1">
				{navItems.map(({ to, labelKey, icon: NavIcon }) => (
					<NavLink
						key={to}
						to={to}
						end={to === "/"}
						className={({ isActive }) =>
							cn(
								"flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium cursor-pointer",
								"transition-colors duration-150",
								isActive
									? "bg-primary/15 text-primary"
									: "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
							)
						}
					>
						<NavIcon className="h-4 w-4 shrink-0" />
						<span>{t(labelKey)}</span>
					</NavLink>
				))}
			</nav>

			{desktopRuntime ? (
				<>
					<div className="ml-auto h-8 min-w-24 flex-1" data-tauri-drag-region />
					<div className="flex items-center gap-1">
						{localeToggleButton}
						{themeToggleButton}
						<div className="flex items-center gap-0.5">
							<Button
								variant="ghost"
								size="sm"
								aria-label={t("header.window.minimize")}
								onClick={() => {
									void desktopWindowController.minimize();
								}}
								className="h-8 w-8 p-0"
							>
								<Minus className="h-4 w-4" />
							</Button>
							<Button
								variant="ghost"
								size="sm"
								aria-label={t("header.window.toggleMaximize")}
								onClick={() => {
									void desktopWindowController.toggleMaximize();
								}}
								className="h-8 w-8 p-0"
							>
								<Square className="h-3.5 w-3.5" />
							</Button>
							<Button
								variant="ghost"
								size="sm"
								aria-label={t("header.window.close")}
								onClick={() => {
									void desktopWindowController.close();
								}}
								className="h-8 w-8 p-0"
							>
								<X className="h-4 w-4" />
							</Button>
						</div>
					</div>
				</>
			) : (
				<div className="ml-auto flex items-center gap-1">
					{localeToggleButton}
					{themeToggleButton}
				</div>
			)}
		</header>
	);
}
