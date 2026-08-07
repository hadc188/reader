// Resolve stored asset URLs to URLs the desktop app can actually load.
//
// Uploaded files (e.g. AI book maps) are stored as relative asset URLs like
// `/assets/default/ai-maps/x.png`. The desktop app serves them over the custom
// `reader` scheme as `http://reader.localhost/files?path=<rel>`. Already
// absolute URLs pass through unchanged.

export function resolveAssetUrl(url?: string | null): string {
  if (!url) return ''
  if (/^https?:\/\//i.test(url)) return url
  if (url.startsWith('/')) {
    return `http://reader.localhost/files?path=${encodeURIComponent(url.replace(/^\//, ''))}`
  }
  return url
}
