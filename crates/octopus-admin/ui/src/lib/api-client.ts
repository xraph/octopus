const API_BASE = "/admin/api";

class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/**
 * Centralized handler invoked whenever the API returns 401. The auth provider
 * registers this on mount so the non-React client can trigger a redirect to the
 * login screen without importing the router.
 */
let unauthorizedHandler: (() => void) | null = null;

export function setUnauthorizedHandler(handler: (() => void) | null) {
  unauthorizedHandler = handler;
}

async function handleResponse<T>(res: Response): Promise<T> {
  if (res.status === 401) {
    unauthorizedHandler?.();
  }
  if (!res.ok) {
    const text = await res.text().catch(() => "Unknown error");
    throw new ApiError(res.status, text);
  }
  // 204 No Content (and empty bodies) should not be JSON-parsed.
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

// The dashboard is served from the same origin as the API, so an
// `same-origin` credentials policy is enough to carry the HttpOnly session
// cookie on every request.
const baseInit: RequestInit = { credentials: "same-origin" };

export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, { ...baseInit });
  return handleResponse<T>(res);
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...baseInit,
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return handleResponse<T>(res);
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...baseInit,
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return handleResponse<T>(res);
}

export async function apiDelete(path: string): Promise<void> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...baseInit,
    method: "DELETE",
  });
  await handleResponse<void>(res);
}
