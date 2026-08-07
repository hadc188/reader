# 用户管理 API

## 用户注册

```
POST /reader3/register
```

请求体:
```json
{
  "username": "用户名",
  "password": "密码"
}
```

## 用户登录

```
POST /reader3/login
```

请求体:
```json
{
  "username": "用户名",
  "password": "密码"
}
```

响应:
```json
{
  "success": true,
  "data": {
    "token": "jwt-token-string",
    "username": "用户名"
  },
  "errorMsg": null
}
```

## 获取用户信息

```
GET /reader3/getUserInfo
```

Header:
```
Authorization: Bearer <token>
```

## 修改密码

```
POST /reader3/changePassword
```

请求体:
```json
{
  "oldPassword": "旧密码",
  "newPassword": "新密码"
}
```

## 获取用户配置

```
GET /reader3/getUserConfig
```

响应:
```json
{
  "success": true,
  "data": {
    "theme": "light",
    "fontSize": 16,
    "lineHeight": 1.5,
    "readConfig": { ... }
  },
  "errorMsg": null
}
```

## 保存用户配置

```
POST /reader3/saveUserConfig
```

请求体:
```json
{
  "theme": "dark",
  "fontSize": 18,
  "lineHeight": 1.8
}
```
