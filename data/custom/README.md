# Morroc 自定义数据目录

本目录用于存放项目专属的自定义数据，例如：

- 目标客户端 `2026-04-01 main` 的专属道具、魔物、技能、地图等数据。
- 私有或测试用数据，不进入 Git 版本控制。
- 覆盖 `vendor/rathena/` 或 `vendor/hercules/` 中默认数据的补丁。

## 目录结构建议

```
data/custom/
  README.md          # 本文件
  db/                # 数据库文件
    re/              # Renewal 数据
      item_db.yml
      mob_db.yml
      skill_db.yml
    pre-re/          # Pre-Renewal 数据（如需要）
      item_db.yml
      mob_db.yml
      skill_db.yml
  npc/               # 自定义 NPC 脚本
    *.txt
```

## 支持的文件格式

- **Hercules 格式**：`.conf` 文件（libconfig 语法），字段名与 `vendor/hercules/db/re/` 一致。
- **rAthena 格式**：`.yml` 文件（YAML 语法），字段名与 `vendor/rathena/db/re/` 一致。
- **NPC 脚本**：`.txt` 文件，使用 Hercules/rAthena 标准 NPC 脚本语法。

## 使用方式

通过转换器的统一入口加载自定义数据：

```bash
cargo run -p morroc-converter -- --input-dir data/custom
```

如果 `data/custom/db/re/item_db.yml` 存在，转换器会识别为 rAthena 仓库格式；
如果 `data/custom/item_db.conf` 存在，则识别为 Hercules 数据库格式；
如果 `data/custom/npc/` 目录存在，则识别为 NPC 脚本目录。

## 与 vendor 数据的对应关系

| 数据类型 | Hercules 参考路径 | rAthena 参考路径 |
|---|---|---|
| 道具 | `vendor/hercules/db/re/item_db.conf` | `vendor/rathena/db/re/item_db.yml` |
| 魔物 | `vendor/hercules/db/re/mob_db.conf` | `vendor/rathena/db/re/mob_db.yml` |
| 技能 | `vendor/hercules/db/re/skill_db.conf` | `vendor/rathena/db/re/skill_db.yml` |
| NPC 脚本 | `vendor/hercules/npc/` | `vendor/rathena/npc/` |

## 数据合并策略

当前转换器不会自动合并多个来源的数据。建议：

1. 以 `vendor/rathena/` 作为默认数据源（最新、最完整）。
2. 将需要覆盖或新增的条目放入 `data/custom/` 的对应位置。
3. 未来在 `morroc-converter` 中实现 `import/` 合并机制后，可以自动加载 `vendor/rathena/` + `data/custom/`。

## 注意事项

- 本目录受 `.gitignore` 保护，不会进入 Git。请自行备份重要数据。
- 自定义数据必须与当前目标客户端版本（`2026-04-01 main`）的 episode 一致，否则可能出现包/显示异常。
- 添加新文件后，建议先用 `morroc-converter --input-dir data/custom` 本地验证，确认不 panic 且输出正常。
