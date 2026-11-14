export type SystemThemeMode = 'light' | 'dark'

function upsertMeta(name: string, content: string, media?: string) {
  const selector = media
    ? `meta[name="${name}"][media="${media}"]`
    : `meta[name="${name}"]:not([media])`
  const existing = document.querySelector(selector) as HTMLMetaElement | null
  const meta = document.createElement('meta')
  meta.setAttribute('name', name)
  meta.setAttribute('content', content)
  if (media) {
    meta.setAttribute('media', media)
  }
  if (existing?.parentNode) {
    existing.parentNode.replaceChild(meta, existing)
  } else {
    document.head.appendChild(meta)
  }
  return meta
}

export function applySystemTheme(
  mode: SystemThemeMode,
  themeColor?: string,
  options: { transparentBackground?: boolean } = {},
) {
  if (typeof document === 'undefined') return

  const resolvedThemeColor = themeColor || (mode === 'dark' ? '#141414' : '#faf9f7')
  const background = options.transparentBackground ? 'transparent' : resolvedThemeColor
  const root = document.documentElement
  const app = document.getElementById('app')

  root.setAttribute('data-theme', mode)
  root.style.setProperty('color-scheme', mode)
  root.style.colorScheme = mode
  root.style.background = background
  root.style.backgroundColor = background
  document.body.style.background = background
  document.body.style.backgroundColor = background
  document.body.style.colorScheme = mode
  if (app) {
    app.style.background = background
    app.style.backgroundColor = background
    app.style.colorScheme = mode
  }

  upsertMeta('theme-color', resolvedThemeColor)
  upsertMeta('theme-color', mode === 'light' ? resolvedThemeColor : '#faf9f7', '(prefers-color-scheme: light)')
  upsertMeta('theme-color', mode === 'dark' ? resolvedThemeColor : '#141414', '(prefers-color-scheme: dark)')
  upsertMeta('msapplication-navbutton-color', resolvedThemeColor)
  upsertMeta('color-scheme', mode)
  upsertMeta('apple-mobile-web-app-status-bar-style', 'black-translucent')

  requestAnimationFrame(() => {
    upsertMeta('theme-color', resolvedThemeColor)
    upsertMeta('msapplication-navbutton-color', resolvedThemeColor)
  })
}
