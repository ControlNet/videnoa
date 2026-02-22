import {
	ArrowDownToLine,
	ArrowLeftRight,
	ArrowUpFromLine,
	Braces,
	Download,
	FileVideo,
	Film,
	Globe,
	HardDrive,
	Hash,
	Microscope,
	Palette,
	PanelLeftClose,
	PanelLeftOpen,
	Radio,
	Replace,
	Scaling,
	Scissors,
	Split,
	Workflow,
} from "lucide-react";
import { type DragEvent, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { JellyfinLogo } from "@/components/shared/JellyfinLogo";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { getLocalizedNodeTitle } from "@/i18n/node-title";
import {
	type NodeDescriptor,
	useDescriptors,
} from "@/stores/node-definitions-store";
import { useUIStore } from "@/stores/ui-store";

const ICON_REGISTRY: Record<
	string,
	React.ComponentType<{ className?: string }>
> = {
	"file-video": FileVideo,
	microscope: Microscope,
	film: Film,
	"hard-drive": HardDrive,
	globe: Globe,
	radio: Radio,
	scaling: Scaling,
	palette: Palette,
	scissors: Scissors,
	hash: Hash,
	tv: JellyfinLogo,
	"arrow-down-to-line": ArrowDownToLine,
	"arrow-up-from-line": ArrowUpFromLine,
	download: Download,
	workflow: Workflow,
	split: Split,
	braces: Braces,
	replace: Replace,
	"arrow-left-right": ArrowLeftRight,
};

const CATEGORY_ORDER = ["input", "processing", "output", "utility", "workflow"];
const DND_MIME_TYPES = ["application/reactflow", "text/plain"] as const;

function onDragStart(event: DragEvent, nodeType: string) {
	for (const mimeType of DND_MIME_TYPES) {
		try {
			event.dataTransfer.setData(mimeType, nodeType);
		} catch {
			void 0;
		}
	}
	event.dataTransfer.effectAllowed = "move";
}

function NodeItem({ descriptor }: { descriptor: NodeDescriptor }) {
	const { t } = useTranslation("editor");
	const accent = descriptor.accent_color;
	const IconComp = ICON_REGISTRY[descriptor.icon];

	return (
		<button
			type="button"
			className="flex items-center gap-2.5 px-3 py-2 rounded-md cursor-grab border border-transparent hover:border-border/60 hover:bg-secondary/40 transition-colors group w-full text-left"
			draggable
			onDragStart={(e) => {
				onDragStart(e, descriptor.node_type);
			}}
		>
			<div
				className="flex items-center justify-center size-7 rounded-md shrink-0"
				style={{ background: `${accent}22`, color: accent }}
			>
				{IconComp && <IconComp className="size-3.5" />}
			</div>
			<span className="text-xs font-medium text-foreground/90 flex-1 truncate">
				{getLocalizedNodeTitle(
					t,
					descriptor.node_type,
					descriptor.display_name,
				)}
			</span>
		</button>
	);
}

export function NodePalette() {
	const collapsed = useUIStore((s) => s.sidebarCollapsed);
	const toggle = useUIStore((s) => s.toggleSidebar);
	const descriptors = useDescriptors();

	const categories = useMemo(() => {
		const groups: Record<string, NodeDescriptor[]> = {};
		for (const d of descriptors) {
			const cat = d.category;
			if (!groups[cat]) groups[cat] = [];
			groups[cat].push(d);
		}
		const ordered = CATEGORY_ORDER.filter((cat) => groups[cat]).map((cat) => ({
			label: cat,
			nodes: groups[cat],
		}));
		const knownSet = new Set(CATEGORY_ORDER);
		for (const cat of Object.keys(groups)) {
			if (!knownSet.has(cat)) {
				ordered.push({ label: cat, nodes: groups[cat] });
			}
		}
		return ordered;
	}, [descriptors]);

	return (
		<div
			className="flex flex-col border-r border-border/50 bg-card/80 backdrop-blur-sm transition-all duration-200"
			style={{ width: collapsed ? 42 : 220 }}
		>
			<div className="flex items-center justify-between px-2 py-2 border-b border-border/40">
				{!collapsed && (
					<span className="text-[10px] font-semibold tracking-wider uppercase text-muted-foreground pl-1">
						Nodes
					</span>
				)}
				<Button variant="ghost" size="icon" className="size-7" onClick={toggle}>
					{collapsed ? (
						<PanelLeftOpen className="size-3.5" />
					) : (
						<PanelLeftClose className="size-3.5" />
					)}
				</Button>
			</div>

			{!collapsed && (
				<ScrollArea className="flex-1">
					<div className="p-2 space-y-3">
						{categories.map((cat) => (
							<div key={cat.label}>
								<div className="text-[9px] font-bold tracking-widest uppercase text-muted-foreground/60 px-3 pb-1">
									{cat.label}
								</div>
								{cat.nodes.map((d) => (
									<NodeItem key={d.node_type} descriptor={d} />
								))}
							</div>
						))}
					</div>
				</ScrollArea>
			)}
		</div>
	);
}
