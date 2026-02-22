import type { WorkflowPort } from '@/types'

export type ParamValue = string | number | boolean

export function getDefaultValue(port: WorkflowPort): ParamValue {
  if (port.default_value != null) {
    if (port.port_type === 'Bool') return Boolean(port.default_value)
    if (port.port_type === 'Int') return Number(port.default_value)
    if (port.port_type === 'Float') return Number(port.default_value)
    return String(port.default_value)
  }
  if (port.port_type === 'Bool') return false
  if (port.port_type === 'Int') return 0
  if (port.port_type === 'Float') return 0.0
  return ''
}

export function buildDefaults(
  inputs: WorkflowPort[],
): Record<string, ParamValue> {
  const values: Record<string, ParamValue> = {}
  for (const port of inputs) {
    values[port.name] = getDefaultValue(port)
  }
  return values
}

export function convertParam(port: WorkflowPort, raw: ParamValue): ParamValue {
  if (port.port_type === 'Int') return parseInt(String(raw), 10)
  if (port.port_type === 'Float') return parseFloat(String(raw))
  if (port.port_type === 'Bool') return Boolean(raw)
  return String(raw)
}
