import type { Preset } from '@/types';
import type { WorkflowEntry } from '@/api/client';

export function presetToEntry(preset: Preset): WorkflowEntry {
  const iface = preset.workflow.interface;
  const has_interface =
    Array.isArray(iface?.inputs) && iface.inputs.length > 0;
  return {
    filename: `${preset.id}.json`,
    name: preset.name,
    description: preset.description,
    workflow: preset.workflow,
    has_interface,
  };
}
