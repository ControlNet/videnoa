import { useEffect, useState, useCallback } from 'react'
import { create } from 'zustand'
import { CheckCircle2, XCircle, Info, X } from 'lucide-react'

type ToastType = 'success' | 'error' | 'info'

interface ToastItem {
  id: string
  type: ToastType
  message: string
}

interface ToastState {
  toasts: ToastItem[]
  add: (type: ToastType, message: string) => void
  remove: (id: string) => void
}

let counter = 0

const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  add: (type, message) => {
    const id = String(++counter)
    set((s) => ({ toasts: [...s.toasts, { id, type, message }] }))
  },
  remove: (id) => {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }))
  },
}))

// eslint-disable-next-line react-refresh/only-export-components
export const toast = {
  success: (message: string) => { useToastStore.getState().add('success', message) },
  error: (message: string) => { useToastStore.getState().add('error', message) },
  info: (message: string) => { useToastStore.getState().add('info', message) },
}

const ICON: Record<ToastType, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
}

const COLOR: Record<ToastType, string> = {
  success: 'border-green-500/40 bg-green-500/10 text-green-400',
  error: 'border-red-500/40 bg-red-500/10 text-red-400',
  info: 'border-blue-500/40 bg-blue-500/10 text-blue-400',
}

function ToastCard({ item, onDismiss }: { item: ToastItem; onDismiss: () => void }) {
  const [visible, setVisible] = useState(false)
  const [exiting, setExiting] = useState(false)

  const dismiss = useCallback(() => {
    setExiting(true)
    setTimeout(onDismiss, 200)
  }, [onDismiss])

  useEffect(() => {
    requestAnimationFrame(() => { setVisible(true) })
    const timer = setTimeout(dismiss, 4000)
    return () => { clearTimeout(timer) }
  }, [dismiss])

  const Icon = ICON[item.type]

  return (
    <div
      className={`flex items-center gap-2.5 rounded-lg border px-3.5 py-2.5 shadow-lg backdrop-blur-md transition-all duration-200 ${COLOR[item.type]} ${
        visible && !exiting
          ? 'translate-x-0 opacity-100'
          : 'translate-x-4 opacity-0'
      }`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span className="text-sm font-medium text-foreground">{item.message}</span>
      <button
        type="button"
        onClick={dismiss}
        className="ml-1 shrink-0 text-muted-foreground hover:text-foreground transition-colors"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}

export function Toaster() {
  const toasts = useToastStore((s) => s.toasts)
  const remove = useToastStore((s) => s.remove)

  if (toasts.length === 0) return null

  return (
    <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2">
      {toasts.map((t) => (
        <ToastCard key={t.id} item={t} onDismiss={() => { remove(t.id) }} />
      ))}
    </div>
  )
}
