import type { WorkflowPort } from '@/types'
import { Input } from '@/components/ui/input'
import { Checkbox } from '@/components/ui/checkbox'
import { PathAutocomplete } from './PathAutocomplete'
import type { ParamValue } from './port-field-utils'

export function PortField({
  port,
  value,
  onChange,
}: {
  port: WorkflowPort
  value: ParamValue
  onChange: (value: ParamValue) => void
}) {
  if (port.port_type === 'Bool') {
    return (
      <div className="flex items-center justify-between gap-3">
        <span className="text-sm text-foreground">{port.name}</span>
        <Checkbox
          checked={Boolean(value)}
          onCheckedChange={(v) => { onChange(!!v) }}
        />
      </div>
    )
  }

  if (port.port_type === 'Int') {
    return (
      <div className="space-y-1">
        <span className="text-sm text-foreground">{port.name}</span>
        <Input
          type="number"
          step={1}
          value={String(value)}
          onChange={(e) => {
            const parsed = parseInt(e.target.value, 10)
            if (!Number.isNaN(parsed)) onChange(parsed)
          }}
        />
      </div>
    )
  }

  if (port.port_type === 'Float') {
    return (
      <div className="space-y-1">
        <span className="text-sm text-foreground">{port.name}</span>
        <Input
          type="number"
          step={0.01}
          value={String(value)}
          onChange={(e) => {
            const parsed = parseFloat(e.target.value)
            if (!Number.isNaN(parsed)) onChange(parsed)
          }}
        />
      </div>
    )
  }

  if (port.port_type === 'Path') {
    return (
      <div className="space-y-1">
        <span className="text-sm text-foreground">{port.name}</span>
        <PathAutocomplete
          value={String(value)}
          onChange={(v) => { onChange(v) }}
        />
      </div>
    )
  }

  return (
    <div className="space-y-1">
      <span className="text-sm text-foreground">{port.name}</span>
      <Input
        type="text"
        value={String(value)}
        onChange={(e) => { onChange(e.target.value) }}
        placeholder={port.name}
      />
    </div>
  )
}
