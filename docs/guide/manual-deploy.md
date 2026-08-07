# 手动部署

## 编译二进制

### Linux/macOS

```bash
# 克隆项目
git clone https://github.com/givenge/reader-rust.git
cd reader-rust

# 编译发布版本
cargo build --release

# 二进制在 target/release/reader-rust
```

### 静态链接

如需静态链接（便于分发）：

```bash
# Linux x86
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

```bash
# linux aarch64
rustup target add aarch64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl
```
## 部署步骤

### 1. 准备目录

```bash
mkdir -p /opt/reader
cp target/release/reader-rust /opt/reader/
cd /opt/reader
mkdir -p storage assets web
```

### 2. 复制前端文件

```bash
cp -r /path/to/web/dist /opt/reader/web/
```

### 3. 创建配置文件

创建 `.env` 文件：

```bash
SERVER_HOST=0.0.0.0
SERVER_PORT=8080
DATABASE_URL=sqlite:storage/reader.db?mode=rwc
WEB_ROOT=./web/dist
STORAGE_DIR=./storage
ASSETS_DIR=./assets
LOG_LEVEL=info
```

### 4. 使用 systemd 管理

创建 `/etc/systemd/system/reader.service`：

```ini
[Unit]
Description=Reader-Rust
After=network.target

[Service]
Type=simple
User=www-data
WorkingDirectory=/opt/reader
ExecStart=/opt/reader/reader-rust
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
```

启动服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable reader
sudo systemctl start reader
```

### 5. 使用 Nginx 反向代理

```nginx
server {
    listen 80;
    server_name your-domain.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

## 更新

```bash
cd /path/to/reader-rust
git pull
cargo build --release
cp target/release/reader-rust /opt/reader/
sudo systemctl restart reader
```
