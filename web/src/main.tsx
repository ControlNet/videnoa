import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { initializeI18n, resolveStartupLocale } from '@/i18n';
import './index.css';
import App from './App';

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error('Root element #root was not found');
}

const appRoot: HTMLElement = rootElement;

async function bootstrap() {
  const startupLocale = await resolveStartupLocale();
  initializeI18n(startupLocale);

  createRoot(appRoot).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void bootstrap();
