import { test, expect } from '@playwright/test'
import { gotoApp, loginFromModal, triggerGlobalSearch, waitForToast } from './support/app'
import { hasLogin, hasSearchFixture, readE2EConfig } from './support/env'

const config = readE2EConfig()

test.describe('Standard E2E: optional full workflows', () => {
  test('auth flow works with configured credentials', async ({ page }) => {
    test.skip(!hasLogin(config), 'Set E2E_USERNAME and E2E_PASSWORD to enable auth regression.')

    await gotoApp(page, '/')
    await loginFromModal(page, config.username!, config.password!)
    await waitForToast(page, '登录成功')
  })

  test('search results can open detail modal with fixture data', async ({ page }) => {
    test.skip(!hasSearchFixture(config), 'Set E2E_SEARCH_KEYWORD and E2E_EXPECTED_BOOK_NAME to enable search regression.')

    await gotoApp(page, '/')
    await triggerGlobalSearch(page, config.searchKeyword!)

    const card = page.locator('.book-card').filter({ hasText: config.expectedBookName! }).first()
    await expect(card).toBeVisible()
    await card.locator('.card-cover').click()
    await expect(page.locator('.detail-modal')).toBeVisible()
    await expect(page.locator('.detail-modal')).toContainText(config.expectedBookName!)
  })

  test('source manager supports import path when fixture file is configured', async ({ page }) => {
    test.skip(!config.sourceImportFile, 'Set E2E_SOURCE_IMPORT_FILE to enable source import regression.')

    await gotoApp(page, '/')
    await page.getByTitle(/设置|用户/).click()
    await page.getByRole('button', { name: '书源管理' }).click()
    await expect(page.getByRole('heading', { name: /书源管理/ })).toBeVisible()

    const chooser = page.locator('input[type="file"]').first()
    await chooser.setInputFiles(config.sourceImportFile!)
    await expect(page.locator('.toast')).toContainText(/导入|成功|失败/)
  })
})
