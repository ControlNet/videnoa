import { create } from 'zustand';

export interface PortDescriptor {
  name: string;
  port_type: string;
  direction: string;
  required: boolean;
  default_value: unknown;
  ui_hint: string | null;
  enum_options: string[] | null;
  dynamic_type_param: string | null;
}

export interface NodeDescriptor {
  node_type: string;
  display_name: string;
  category: string;
  accent_color: string;
  icon: string;
  inputs: PortDescriptor[];
  outputs: PortDescriptor[];
}

interface NodeDefinitionsState {
  descriptors: NodeDescriptor[];
  loading: boolean;
  error: string | null;
  fetch: () => Promise<void>;
}

export const useNodeDefinitions = create<NodeDefinitionsState>((set, get) => ({
  descriptors: [],
  loading: false,
  error: null,
  fetch: async () => {
    if (get().descriptors.length > 0) return;
    set({ loading: true, error: null });
    try {
      const resp = await fetch('/api/nodes');
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const data: NodeDescriptor[] = await resp.json();
      set({ descriptors: data, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));

export function useDescriptor(nodeType: string): NodeDescriptor | undefined {
  return useNodeDefinitions((s) => s.descriptors.find((d) => d.node_type === nodeType));
}

export function useDescriptors(): NodeDescriptor[] {
  return useNodeDefinitions((s) => s.descriptors);
}
