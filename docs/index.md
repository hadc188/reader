---
layout: home

hero:
  name: "Reader-Rust"
  text: "阅读服务端 Rust版"
  tagline: 高性能书源阅读服务器，支持自定义书源、多格式解析
  actions:
    - theme: brand
      text: 快速开始
      link: /guide/quickstart
    - theme: alt
      text: 用户手册
      link: /guide/user-manual
    - theme: alt
      text: API文档
      link: /api/

features:
  - icon: ⚡
    title: 高性能
    details: 基于 Rust 和 axum 构建，内存安全，并发处理能力强
  - icon: 🔧
    title: 灵活书源
    details: 支持 CSS选择器、JSONPath、XPath、正则、JavaScript 多种解析方式
  - icon: 🌐
    title: 规则引擎
    details: 自动检测内容类型，智能匹配解析规则
  - icon: 💾
    title: 数据持久化
    details: SQLite 存储书源配置，文件缓存章节内容
---