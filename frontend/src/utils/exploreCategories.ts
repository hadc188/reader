export interface ExploreCategory {
  title: string
  url: string
}

export function parseExploreCategories(rule?: string | null): ExploreCategory[] {
  const trimmedRule = rule?.trim()
  if (!trimmedRule) return []

  try {
    if (trimmedRule.startsWith('[')) {
      const parsed = JSON.parse(normalizeRelaxedExploreJson(trimmedRule))
      if (Array.isArray(parsed)) {
        return parsed
          .map((item) => ({
            title: String(item?.title || '').trim(),
            url: String(item?.url || '').trim(),
          }))
          .filter((item) => item.title)
      }
    }
  } catch {
    // Fall back to the plain text parser below.
  }

  return trimmedRule
    .split(/\n|<br>/i)
    .map((line) => line.trim())
    .filter(Boolean)
    .flatMap((line) => {
      if (!line.includes('::')) return []
      const [title, url] = line.split('::').map((part) => part.trim())
      return title ? [{ title, url: url || '' }] : []
    })
}

function normalizeRelaxedExploreJson(rule: string) {
  let normalized = ''
  let inString = false
  let quote = ''
  let escaped = false

  for (const char of rule) {
    if (inString) {
      normalized += char
      if (escaped) {
        escaped = false
      } else if (char === '\\') {
        escaped = true
      } else if (char === quote) {
        inString = false
      }
      continue
    }

    if (char === '"' || char === "'") {
      inString = true
      quote = char
      normalized += char
    } else if (char === '<') {
      normalized += '{'
    } else if (char === '>') {
      normalized += '}'
    } else {
      normalized += char
    }
  }

  return normalized
}

export function isExploreCategorySection(category: ExploreCategory) {
  return !category.url.trim()
}

export function getInitialExploreCategoryUrl(categories: ExploreCategory[]) {
  return categories.find((category) => !isExploreCategorySection(category))?.url || ''
}

export function getExploreCategoryKey(category: ExploreCategory, index: number) {
  return `${category.url || 'section'}:${index}:${category.title}`
}
