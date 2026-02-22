import { BrowserRouter, Route, Routes } from 'react-router'
import { AppShell } from '@/components/layout/AppShell'
import { ErrorBoundary } from '@/components/shared/ErrorBoundary'
import { Toaster } from '@/components/shared/Toaster'
import { EditorPage } from '@/pages/editor/EditorPage'
import { JobsPage } from '@/pages/jobs/JobsPage'
import { ModelsPage } from '@/pages/models/ModelsPage'
import { PerformancePage } from '@/pages/performance/PerformancePage'
import { ComparisonViewer } from '@/pages/preview/ComparisonViewer'
import { BatchPanel } from '@/pages/settings/BatchPanel'
import { SettingsPage } from '@/pages/settings/SettingsPage'

export default function App() {
  return (
    <BrowserRouter>
      <AppShell>
        <Routes>
          <Route path="/" element={<ErrorBoundary><EditorPage /></ErrorBoundary>} />
          <Route path="/jobs" element={<ErrorBoundary><JobsPage /></ErrorBoundary>} />
          <Route path="/models" element={<ErrorBoundary><ModelsPage /></ErrorBoundary>} />
          <Route path="/performance" element={<ErrorBoundary><PerformancePage /></ErrorBoundary>} />
          <Route path="/settings" element={<ErrorBoundary><SettingsPage /></ErrorBoundary>} />
        </Routes>
      </AppShell>
      <ComparisonViewer />
      <BatchPanel />
      <Toaster />
    </BrowserRouter>
  )
}
