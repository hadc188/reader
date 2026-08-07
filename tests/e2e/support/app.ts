import { expect, type Page } from '@playwright/test'

export async function gotoApp(page: Page, path = '/') {
  const normalized = normalizeHashPath(path)
  await page.goto(normalized)
  await expect(page.locator('[data-v-app]')).toBeVisible()
}

export async function openSettings(page: Page) {
  await page.getByTitle(/设置|用户/).click()
  await expect(page.getByRole('heading', { name: '设置', exact: true })).toBeVisible()
}

export async function closeSettings(page: Page) {
  const closeButton = page.locator('.settings-drawer .close-btn').first()
  if (await closeButton.isVisible()) {
    await closeButton.click()
  }
}

export async function triggerGlobalSearch(page: Page, keyword: string) {
  const input = page.getByPlaceholder('搜索书籍...')
  await input.fill(keyword)
  await input.press('Enter')
  await expect(page.getByText(`搜索 "${keyword}"`)).toBeVisible()
}

export async function loginFromModal(page: Page, username: string, password: string) {
  await openSettings(page)
  await page.getByRole('button', { name: '登录 / 注册' }).click()
  await expect(page.getByRole('heading', { name: '登录' })).toBeVisible()
  await page.locator('#username').fill(username)
  await page.locator('#password').fill(password)
  await page.getByRole('button', { name: '登 录' }).click()
}

export async function waitForToast(page: Page, text: string) {
  await expect(page.locator('.toast').filter({ hasText: text }).first()).toBeVisible()
}

function normalizeHashPath(path: string) {
  if (!path || path === '/' || path === '#/' || path === '/#/') {
    return '/#/'
  }
  const clean = path.replace(/^#?\/?/, '')
  return `/#/${clean}`
}
