import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { enResources } from "@/i18n/locales/en";
import { isDesktopRuntime } from "@/lib/runtime-desktop";
import {
	FALLBACK_LOCALE,
	I18N_NAMESPACES,
	isSupportedLocale,
	LOCALE_STORAGE_KEY,
	SUPPORTED_LOCALES,
	type SupportedLocale,
} from "@/i18n/locales/types";
import { zhCNResources } from "@/i18n/locales/zh-CN";

interface ConfigPayload {
	locale?: unknown;
	[key: string]: unknown;
}

const resources = {
	en: enResources,
	"zh-CN": zhCNResources,
};

let isLanguageChangeListenerBound = false;
let desktopConfigLocale: SupportedLocale | null = null;

function persistLocale(locale: SupportedLocale) {
	if (typeof window === "undefined") return;
	window.localStorage.setItem(LOCALE_STORAGE_KEY, locale);
}

function syncDocumentLanguage(locale: SupportedLocale) {
	if (typeof document === "undefined") return;
	document.documentElement.lang = locale;
}

function resolveBrowserLocale(): SupportedLocale {
	if (typeof navigator === "undefined") return FALLBACK_LOCALE;

	const locale = navigator.language;
	if (isSupportedLocale(locale)) return locale;
	if (locale.toLowerCase().startsWith("zh")) return "zh-CN";

	return FALLBACK_LOCALE;
}

function resolveInitialLocale(): SupportedLocale {
	if (typeof window === "undefined") return FALLBACK_LOCALE;

	const savedLocale = window.localStorage.getItem(LOCALE_STORAGE_KEY);
	if (!savedLocale) {
		const browserLocale = resolveBrowserLocale();
		persistLocale(browserLocale);
		return browserLocale;
	}

	if (isSupportedLocale(savedLocale)) return savedLocale;

	persistLocale(FALLBACK_LOCALE);
	return FALLBACK_LOCALE;
}

async function fetchDesktopConfig(): Promise<ConfigPayload | null> {
	if (typeof window === "undefined" || !isDesktopRuntime()) return null;

	try {
		const response = await fetch("/api/config");
		if (!response.ok) return null;
		const payload = (await response.json()) as ConfigPayload;
		return payload && typeof payload === "object" ? payload : null;
	} catch {
		return null;
	}
}

async function persistDesktopLocale(locale: SupportedLocale): Promise<void> {
	if (typeof window === "undefined" || !isDesktopRuntime()) return;
	if (desktopConfigLocale === locale) return;

	const config = await fetchDesktopConfig();
	if (!config) return;

	try {
		const response = await fetch("/api/config", {
			method: "PUT",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ ...config, locale }),
		});

		if (response.ok) {
			desktopConfigLocale = locale;
		}
	} catch {
		return;
	}
}

export async function resolveStartupLocale(): Promise<SupportedLocale> {
	if (typeof window === "undefined") return FALLBACK_LOCALE;

	if (isDesktopRuntime()) {
		const config = await fetchDesktopConfig();
		if (config && typeof config.locale === "string") {
			const configLocale = normalizeLocale(config.locale);
			desktopConfigLocale = configLocale;
			persistLocale(configLocale);
			return configLocale;
		}
	}

	return resolveInitialLocale();
}

function normalizeLocale(locale: string): SupportedLocale {
	return isSupportedLocale(locale) ? locale : FALLBACK_LOCALE;
}

function bindLanguageChangeListener() {
	if (isLanguageChangeListenerBound) return;

	i18n.on("languageChanged", (nextLocale) => {
		const locale = normalizeLocale(nextLocale);
		persistLocale(locale);
		syncDocumentLanguage(locale);
		void persistDesktopLocale(locale);

		if (locale !== nextLocale) {
			void i18n.changeLanguage(locale);
		}
	});

	isLanguageChangeListenerBound = true;
}

export function initializeI18n(initialLocale?: SupportedLocale) {
	const resolvedInitialLocale = initialLocale ?? resolveInitialLocale();

	if (isDesktopRuntime()) {
		desktopConfigLocale = resolvedInitialLocale;
	}
	syncDocumentLanguage(resolvedInitialLocale);

	if (!i18n.isInitialized) {
		void i18n.use(initReactI18next).init({
			resources,
			lng: resolvedInitialLocale,
			fallbackLng: FALLBACK_LOCALE,
			supportedLngs: [...SUPPORTED_LOCALES],
			ns: [...I18N_NAMESPACES],
			defaultNS: "common",
			interpolation: { escapeValue: false },
			react: { useSuspense: false },
		});
	}

	bindLanguageChangeListener();
	return i18n;
}

export { i18n };
export type { SupportedLocale } from "@/i18n/locales/types";
