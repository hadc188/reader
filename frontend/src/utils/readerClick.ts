const READER_INTERACTIVE_SELECTOR = [
  'button',
  'a',
  'input',
  'textarea',
  'select',
  '[role="button"]',
  '[contenteditable="true"]',
  '.tts-controls',
  '.mobile-controls',
  '.reader-search-panel',
  '.selection-menu',
].join(', ')

export function isReaderInteractiveClickTarget(target: Pick<HTMLElement, 'closest'> | null | undefined) {
  return Boolean(target?.closest(READER_INTERACTIVE_SELECTOR))
}
