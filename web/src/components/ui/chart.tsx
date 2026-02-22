import * as React from "react";
import { ResponsiveContainer, Tooltip } from "recharts";
import type {
	NameType,
	Payload as TooltipPayload,
	ValueType,
} from "recharts/types/component/DefaultTooltipContent";

import { cn } from "@/lib/utils";

export type ChartConfig = Record<
	string,
	{
		label: string;
		color?: string;
	}
>;

type ChartContextValue = {
	config: ChartConfig;
};

const ChartContext = React.createContext<ChartContextValue | null>(null);

function useChart() {
	const context = React.useContext(ChartContext);

	if (!context) {
		throw new Error("useChart must be used within <ChartContainer />");
	}

	return context;
}

function buildChartStyle(config: ChartConfig): React.CSSProperties {
	const style: Record<string, string> = {};

	for (const [key, item] of Object.entries(config)) {
		if (item.color) {
			style[`--color-${key}`] = item.color;
		}
	}

	return style as React.CSSProperties;
}

type ChartContainerProps = React.ComponentProps<"div"> & {
	config: ChartConfig;
	ariaLabel: string;
};

const ChartContainer = React.forwardRef<HTMLDivElement, ChartContainerProps>(
	({ className, config, style, children, ariaLabel, ...props }, ref) => {
		return (
			<ChartContext.Provider value={{ config }}>
				<div
					ref={ref}
					className={cn("h-64 w-full text-xs", className)}
					role="img"
					aria-label={ariaLabel}
					style={{ ...buildChartStyle(config), ...style }}
					{...props}
				>
					<ResponsiveContainer>{children}</ResponsiveContainer>
				</div>
			</ChartContext.Provider>
		);
	},
);
ChartContainer.displayName = "ChartContainer";

const ChartTooltip = Tooltip;

type ChartTooltipContentProps = React.ComponentProps<"div"> & {
	active?: boolean;
	payload?: ReadonlyArray<TooltipPayload<ValueType, NameType>>;
	label?: string | number;
	valueFormatter?: (value: ValueType, key: string) => React.ReactNode;
};

const ChartTooltipContent = React.forwardRef<
	HTMLDivElement,
	ChartTooltipContentProps
>(({ active, payload, label, className, valueFormatter }, ref) => {
	const { config } = useChart();

	if (!active || !payload || payload.length === 0) {
		return null;
	}

	return (
		<div
			ref={ref}
			className={cn(
				"min-w-44 rounded-lg border bg-popover px-3 py-2 text-popover-foreground shadow-md",
				className,
			)}
		>
			{label ? (
				<p className="mb-2 text-xs font-medium text-muted-foreground">
					{label}
				</p>
			) : null}
			<div className="space-y-1">
				{payload.map((item) => {
					const key = String(item.dataKey ?? item.name ?? "");
					const series = config[key];

					return (
						<div
							key={key}
							className="flex items-center justify-between gap-4 text-xs"
						>
							<div className="flex items-center gap-2 text-muted-foreground">
								<span
									className="size-2 rounded-full"
									style={{
										backgroundColor:
											item.color ??
											(series?.color ? `var(--color-${key})` : "currentColor"),
									}}
								/>
								<span>{series?.label ?? item.name ?? key}</span>
							</div>
							<span className="font-medium text-foreground">
								{valueFormatter
									? valueFormatter(item.value as ValueType, key)
									: String(item.value ?? "")}
							</span>
						</div>
					);
				})}
			</div>
		</div>
	);
});
ChartTooltipContent.displayName = "ChartTooltipContent";

export { ChartContainer, ChartTooltip, ChartTooltipContent };
