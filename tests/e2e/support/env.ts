export type E2EConfig = {
  username?: string
  password?: string
  searchKeyword?: string
  expectedBookName?: string
  sourceImportFile?: string
}

export function readE2EConfig(): E2EConfig {
  return {
    username: process.env.E2E_USERNAME,
    password: process.env.E2E_PASSWORD,
    searchKeyword: process.env.E2E_SEARCH_KEYWORD,
    expectedBookName: process.env.E2E_EXPECTED_BOOK_NAME,
    sourceImportFile: process.env.E2E_SOURCE_IMPORT_FILE,
  }
}

export function hasLogin(config: E2EConfig) {
  return Boolean(config.username && config.password)
}

export function hasSearchFixture(config: E2EConfig) {
  return Boolean(config.searchKeyword && config.expectedBookName)
}
