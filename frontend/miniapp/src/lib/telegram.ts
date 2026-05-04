/**
 * Tiny wrapper over the Telegram Mini App SDK.
 *
 * We keep this thin so that anything not yet supported by `@telegram-apps`
 * can fall back to `window.Telegram.WebApp` directly without rewriting call
 * sites.
 */

interface TelegramWebApp {
  initData: string;
  ready: () => void;
  expand: () => void;
}

interface TelegramWindow {
  Telegram?: { WebApp?: TelegramWebApp };
}

function webApp(): TelegramWebApp | undefined {
  return (window as unknown as TelegramWindow).Telegram?.WebApp;
}

/** Raw, signed `initData` query string. Forward this to the API verbatim. */
export function getInitDataRaw(): string | undefined {
  return webApp()?.initData;
}

/** Tell Telegram the Mini App finished loading and expand to full height. */
export function notifyReady(): void {
  const app = webApp();
  if (!app) return;
  app.ready();
  app.expand();
}
