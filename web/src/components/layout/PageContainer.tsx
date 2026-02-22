import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

interface PageContainerProps {
  title?: string
  children?: ReactNode
  className?: string
}

export function PageContainer({ title, children, className }: PageContainerProps) {
  return (
    <div className={cn('flex-1 p-6 overflow-auto', className)}>
      {title && (
        <h2 className="text-lg font-semibold tracking-tight mb-6">{title}</h2>
      )}
      {children}
    </div>
  )
}
