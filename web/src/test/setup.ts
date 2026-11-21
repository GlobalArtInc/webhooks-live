import { configure } from '@testing-library/react';

const ASYNC_UTIL_TIMEOUT_MS = 15_000;

configure({ asyncUtilTimeout: ASYNC_UTIL_TIMEOUT_MS });
