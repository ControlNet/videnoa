const DEFAULT_UNKNOWN_ERROR = 'Unknown error';

export function getErrorMessage(error: unknown, fallback: string = DEFAULT_UNKNOWN_ERROR): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }

  if (typeof error === 'string' && error.trim().length > 0) {
    return error;
  }

  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    const message = (error as { message: string }).message;
    if (message.trim().length > 0) return message;
  }

  return fallback;
}

export function formatErrorWithPrefix(prefix: string, error: unknown, fallback?: string): string {
  const detail = getErrorMessage(error, fallback);
  return `${prefix}: ${detail}`;
}
