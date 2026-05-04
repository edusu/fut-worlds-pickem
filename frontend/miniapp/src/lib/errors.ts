/**
 * Errors raised by the Mini App's API client.
 *
 * Kept in its own module so any future error subclasses (network errors,
 * validation errors, etc.) live alongside `ApiError` rather than next to
 * the fetch wrapper.
 */

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}
