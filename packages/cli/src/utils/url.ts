/**
 * URL helpers shared across CLI commands
 */

export interface ConfigLookup {
  getConfig: (key: string) => string | null;
}

export function resolveTargetUrl(target: string, db: ConfigLookup, overrideUrl?: string): string {
  if (overrideUrl) return overrideUrl;
  if (isHttpUrl(target)) return target;

  const base = resolveWikiUrl(db);
  const title = replaceSpaces(target, '_');
  return `${base}/wiki/${encodeURIComponent(title)}`;
}

export function resolveWikiUrl(db: ConfigLookup): string {
  const envUrl = process.env.WIKI_URL;
  if (envUrl) return trimTrailingSlash(envUrl);
  const configUrl = db.getConfig('wiki_url');
  if (configUrl) return trimTrailingSlash(configUrl);
  const apiUrl = db.getConfig('wiki_api_url');
  const derived = deriveWikiUrl(apiUrl);
  return derived || 'https://wiki.remilia.org';
}

export function deriveWikiUrl(apiUrl: string | null): string | null {
  if (!apiUrl) return null;
  try {
    const url = new URL(apiUrl);
    const pathLower = url.pathname.toLowerCase();
    if (pathLower.endsWith('/api.php')) {
      url.pathname = url.pathname.slice(0, -8);
    } else if (pathLower.endsWith('api.php')) {
      url.pathname = url.pathname.slice(0, -7);
    }
    const normalized = trimTrailingSlash(url.toString());
    return normalized.length > 0 ? normalized : null;
  } catch {
    return null;
  }
}

export function trimTrailingSlash(value: string): string {
  let out = value;
  while (out.endsWith('/')) out = out.slice(0, -1);
  return out;
}

export function replaceSpaces(value: string, replacement: string): string {
  let out = '';
  for (let i = 0; i < value.length; i++) {
    const ch = value[i];
    out += ch === ' ' ? replacement : ch;
  }
  return out;
}

export function isHttpUrl(value: string): boolean {
  return value.startsWith('http://') || value.startsWith('https://');
}
