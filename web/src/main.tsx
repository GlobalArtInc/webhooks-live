import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { TokenSamplePage } from './app/TokenSamplePage';
import { I18nProvider } from './i18n/provider';
import './styles/fonts.css';
import './styles/tokens.css';
import './styles/base.css';

const container = document.getElementById('root');

if (!container) {
  throw new Error('Root element #root is missing from index.html');
}

createRoot(container).render(
  <StrictMode>
    <I18nProvider>
      <TokenSamplePage />
    </I18nProvider>
  </StrictMode>,
);
