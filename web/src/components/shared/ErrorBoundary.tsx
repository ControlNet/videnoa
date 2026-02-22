import {
	AlertTriangle,
	ChevronDown,
	ChevronRight,
	RotateCcw,
} from "lucide-react";
import type { ErrorInfo, ReactNode } from "react";
import { Component, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

// ─── Fallback UI ──────────────────────────────────────────────────────────────

interface ErrorFallbackProps {
	error: Error;
	resetError: () => void;
}

export function ErrorFallback({ error, resetError }: ErrorFallbackProps) {
	const { t } = useTranslation("common");
	const [showDetails, setShowDetails] = useState(false);

	return (
		<div className="flex h-full w-full items-center justify-center p-8">
			<Card className="max-w-lg w-full border-destructive/30 bg-destructive/5">
				<CardHeader className="pb-3">
					<div className="flex items-center gap-3">
						<div className="flex h-10 w-10 items-center justify-center rounded-xl bg-destructive/20">
							<AlertTriangle className="h-5 w-5 text-destructive" />
						</div>
						<div>
							<CardTitle className="text-base">
								{t("errorBoundary.title")}
							</CardTitle>
							<p className="text-xs text-muted-foreground mt-0.5">
								{t("errorBoundary.description")}
							</p>
						</div>
					</div>
				</CardHeader>
				<CardContent className="space-y-3">
					<p className="text-sm text-muted-foreground">{error.message}</p>

					<button
						type="button"
						className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
						onClick={() => {
							setShowDetails((v) => !v);
						}}
					>
						{showDetails ? (
							<ChevronDown className="h-3 w-3" />
						) : (
							<ChevronRight className="h-3 w-3" />
						)}
						{showDetails
							? t("errorBoundary.hideDetails")
							: t("errorBoundary.showDetails")}
					</button>

					{showDetails && (
						<pre className="max-h-48 overflow-auto rounded-md bg-secondary/60 p-3 text-[11px] text-muted-foreground font-mono leading-relaxed">
							{error.stack ?? t("errorBoundary.noStackTrace")}
						</pre>
					)}

					<Button size="sm" variant="secondary" onClick={resetError}>
						<RotateCcw className="h-3.5 w-3.5" />
						{t("errorBoundary.tryAgain")}
					</Button>
				</CardContent>
			</Card>
		</div>
	);
}

// ─── Error Boundary ───────────────────────────────────────────────────────────

interface ErrorBoundaryProps {
	children: ReactNode;
	fallback?: (props: ErrorFallbackProps) => ReactNode;
}

interface ErrorBoundaryState {
	error: Error | null;
}

export class ErrorBoundary extends Component<
	ErrorBoundaryProps,
	ErrorBoundaryState
> {
	constructor(props: ErrorBoundaryProps) {
		super(props);
		this.state = { error: null };
	}

	static getDerivedStateFromError(error: Error): ErrorBoundaryState {
		return { error };
	}

	componentDidCatch(error: Error, info: ErrorInfo): void {
		console.error("[ErrorBoundary]", error, info.componentStack);
	}

	private resetError = (): void => {
		this.setState({ error: null });
	};

	render() {
		const { error } = this.state;
		if (error) {
			if (this.props.fallback) {
				return this.props.fallback({ error, resetError: this.resetError });
			}
			return <ErrorFallback error={error} resetError={this.resetError} />;
		}
		return this.props.children;
	}
}
