import type { TFunction } from "i18next";

const NODE_TITLE_KEY_BY_TYPE: Record<string, string> = {
	VideoInput: "nodeTitle.VideoInput",
	SuperResolution: "nodeTitle.SuperResolution",
	FrameInterpolation: "nodeTitle.FrameInterpolation",
	VideoOutput: "nodeTitle.VideoOutput",
	Downloader: "nodeTitle.Downloader",
	JellyfinVideo: "nodeTitle.JellyfinVideo",
	WorkflowInput: "nodeTitle.WorkflowInput",
	WorkflowOutput: "nodeTitle.WorkflowOutput",
	Workflow: "nodeTitle.Workflow",
	Resize: "nodeTitle.Resize",
	Rescale: "nodeTitle.Rescale",
	ColorSpace: "nodeTitle.ColorSpace",
	SceneDetect: "nodeTitle.SceneDetect",
	StreamOutput: "nodeTitle.StreamOutput",
	Constant: "nodeTitle.Constant",
	PathDivider: "nodeTitle.PathDivider",
	PathJoiner: "nodeTitle.PathJoiner",
	StringReplace: "nodeTitle.StringReplace",
	StringTemplate: "nodeTitle.StringTemplate",
	TypeConversion: "nodeTitle.TypeConversion",
	HttpRequest: "nodeTitle.HttpRequest",
	Print: "nodeTitle.Print",
};

export function nodeTitleKeyFromType(nodeType: string): string | undefined {
	return NODE_TITLE_KEY_BY_TYPE[nodeType];
}

export function getLocalizedNodeTitle(
	t: TFunction<"editor">,
	nodeType: string,
	displayName: string,
): string {
	const key = nodeTitleKeyFromType(nodeType);
	if (!key) return displayName;
	return t(key, { defaultValue: displayName });
}
