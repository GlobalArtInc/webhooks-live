import { createBrowserHistory } from '@tanstack/react-router';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { AppProviders, createAppQueryClient } from './app/providers';
import { createAppRouter } from './app/router';
import './styles/fonts.css';
import './styles/tokens.css';
import './styles/base.css';

const container = document.getElementById('root');

if (!container) {
  throw new Error('Root element #root is missing from index.html');
}

const queryClient = createAppQueryClient();
const router = createAppRouter({ history: createBrowserHistory(), queryClient });

createRoot(container).render(
  <StrictMode>
    <AppProviders queryClient={queryClient} router={router} />
  </StrictMode>,
);
