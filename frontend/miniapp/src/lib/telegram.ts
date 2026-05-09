/**
 * Tiny wrapper over the Telegram Mini App SDK.
 *
 * We keep this thin so that anything not yet supported by `@telegram-apps`
 * can fall back to `window.Telegram.WebApp` directly without rewriting call
 * sites.
 */

interface TelegramInitDataUnsafe {
  start_param?: string;
}

interface TelegramWebApp {
  initData: string;
  initDataUnsafe?: TelegramInitDataUnsafe;
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

/**
 * Value of the `start_param` Telegram passes when the Mini App is opened
 * via an inline `web_app` button carrying a `start_parameter`. The bot
 * uses it to bind the session to a specific pickem (the Telegram chat the
 * user clicked from). `undefined` when the Mini App is opened from a
 * context that does not set one (e.g. the bot's global menu button).
 */
export function getStartParam(): string | undefined {
  return webApp()?.initDataUnsafe?.start_param;
}

/** Tell Telegram the Mini App finished loading and expand to full height. */
export function notifyReady(): void {
  const app = webApp();
  if (!app) return;
  app.ready();
  app.expand();
}
