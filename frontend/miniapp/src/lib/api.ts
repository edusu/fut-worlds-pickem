/**
 * Thin wrapper over `fetch` for the FutWorldsPickem HTTP API.
 *
 * Every protected endpoint requires the Telegram-signed initData payload,
 * which we forward verbatim as the `X-Telegram-Init-Data` header. The bot
 * server validates the signature with HMAC-SHA256 against the bot token.
 */

import { getInitDataRaw } from './telegram';

const baseUrl = import.meta.env.VITE_API_BASE_URL ?? '';

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const initData = getInitDataRaw();
  const headers = new Headers(init?.headers);
  headers.set('Content-Type', 'application/json');
  if (initData) {
    headers.set('X-Telegram-Init-Data', initData);
  }

  const response = await fetch(`${baseUrl}${path}`, { ...init, headers });
  if (!response.ok) {
    const body = await response.text();
    throw new ApiError(response.status, body || response.statusText);
  }

  return (await response.json()) as T;
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body: unknown) =>
    request<T>(path, { method: 'POST', body: JSON.stringify(body) }),
};
