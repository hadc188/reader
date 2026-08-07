import { test, expect } from '@playwright/test'
import { closeSettings, gotoApp, openSettings, triggerGlobalSearch } from './support/app'

test.describe('Standard E2E: app shell', () => {
  test('navigates across core tabs', async ({ page }) => {
    await gotoApp(page, '/')

    await expect(page.getByTitle('书架')).toBeVisible()
    await page.getByTitle('书海').click()
    await expect(page).toHaveURL(/#\/explore$/)
    await expect(page.getByRole('heading', { name: '发现书海' })).toBeVisible()

    await page.getByTitle('最近').click()
    await expect(page).toHaveURL(/#\/recent$/)

    await page.getByTitle('RSS').click()
    await expect(page).toHaveURL(/#\/rss$/)
    await expect(page.getByRole('heading', { name: '文章列表' })).toBeVisible()

    await page.getByTitle('书架').click()
    await expect(page).toHaveURL(/#\/?$/)
    await expect(page.getByRole('heading', { name: /书架/ })).toBeVisible()
  })

  test('opens settings and management entry points', async ({ page }) => {
    await gotoApp(page, '/')
    await openSettings(page)

    await expect(page.getByRole('button', { name: '书源管理', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '用户管理', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '备份与恢复', exact: true })).toBeVisible()

    await closeSettings(page)
  })

  test('enters search mode and exposes scope filters', async ({ page }) => {
    await gotoApp(page, '/')
    await triggerGlobalSearch(page, '测试')

    await expect(page.getByRole('button', { name: '全部书源', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '按分组', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '单个书源', exact: true })).toBeVisible()
  })

  test('opens bookshelf management dialogs', async ({ page }) => {
    await gotoApp(page, '/')

    await page.getByRole('button', { name: '分组管理' }).click()
    await expect(page.getByRole('heading', { name: '分组管理' })).toBeVisible()
    await page.locator('.modal-container .close-btn').first().click()
    await expect(page.getByRole('heading', { name: '分组管理' })).not.toBeVisible()

    await page.getByRole('button', { name: '缓存管理' }).click()
    await expect(page.getByRole('heading', { name: '缓存管理' })).toBeVisible()
  })

  test('opens RSS source management page', async ({ page }) => {
    await gotoApp(page, '/rss')
    await page.getByRole('button', { name: '管理订阅源', exact: true }).click()
    await expect(page).toHaveURL(/#\/rss\/manage$/)
  })
})
