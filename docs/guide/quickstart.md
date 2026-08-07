# 快速开始

## 环境要求

- Rust 1.70+ (推荐最新稳定版)
- Node.js 16+ (用于前端开发)

## 克隆项目

```bash
git clone https://github.com/givenge/reader-rust.git
cd reader-rust
```

## 运行后端

```bash
# 开发模式运行
cargo run

# 构建发布版本
cargo build --release
```

服务器默认运行在 `0.0.0.0:8080`。

## 运行前端

```bash
cd web
npm install
npm run serve
```

前端开发服务器通常运行在 `http://localhost:8081`。

## 验证安装

打开浏览器访问前端地址，如果能看到界面并可以添加书源，说明安装成功。

## 默认配置

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| SERVER_HOST | 0.0.0.0 | 服务器绑定地址 |
| SERVER_PORT | 8080 | 服务器端口 |
| DATABASE_URL | sqlite:storage/reader.db?mode=rwc | SQLite 数据库路径 |
| LOG_LEVEL | info | 日志级别 |

更多配置选项请参考 [配置指南](./configuration)。
