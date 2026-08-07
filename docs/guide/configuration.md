# 配置

Reader-Rust 通过环境变量进行配置。默认配置定义在 `src/app/config.rs` 中。

## 环境变量

### 服务器配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `SERVER_HOST` | `0.0.0.0` | 服务器绑定地址 |
| `SERVER_PORT` | `8080` | 服务器端口 |

### 数据库配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `DATABASE_URL` | `sqlite:storage/reader.db?mode=rwc` | SQLite 数据库连接字符串 |

### 存储配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `STORAGE_DIR` | `storage/` | 数据存储目录 |
| `ASSETS_DIR` | `assets/` | 资源文件目录 |

### Web 前端配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `WEB_ROOT` | `../reader/web` | 静态 Web 文件目录 |

### 其他配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `LOG_LEVEL` | `info` | 日志级别 (trace, debug, info, warn, error) |
| `REQUEST_TIMEOUT_SECS` | `15` | HTTP 请求超时时间(秒) |

## 配置格式

环境变量使用 `__`（双下划线）作为分隔符表示层级结构。

例如，在嵌套配置中:
```bash
export APP__SERVER__PORT=3000
```

## 配置文件

你也可以创建 `.env` 文件:

```bash
SERVER_PORT=3000
DATABASE_URL=sqlite:custom/path/reader.db?mode=rwc
LOG_LEVEL=debug
```

## 开发配置

开发时建议:

```bash
LOG_LEVEL=debug
cargo run
```

生产环境建议:

```bash
LOG_LEVEL=info
SERVER_PORT=80
cargo run --release
```
