// Dev-only logger (STYLE-GUIDE: the webview ships no `console.*` in
// production builds). All diagnostics go through here; the vite build
// replaces `import.meta.env.DEV` with false and drops the branches.

const isDev = import.meta.env.DEV;

export function logDebug(scope: string, ...parts: unknown[]): void {
  if (isDev) console.debug(`[panel:${scope}]`, ...parts);
}

export function logWarn(scope: string, ...parts: unknown[]): void {
  if (isDev) console.warn(`[panel:${scope}]`, ...parts);
}
