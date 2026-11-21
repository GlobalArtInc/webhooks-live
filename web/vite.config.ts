import react from '@vitejs/plugin-react';
import { configDefaults, defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: 'dom',
          environment: 'jsdom',
          include: ['src/**/*.test.{ts,tsx}'],
          exclude: [...configDefaults.exclude, 'src/features/export/**'],
        },
      },
      {
        extends: true,
        test: {
          name: 'export',
          environment: 'node',
          include: ['src/features/export/**/*.test.ts'],
        },
      },
    ],
  },
});
