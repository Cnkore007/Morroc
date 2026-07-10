//! Morroc Converter CLI
//!
//! 用法：
//!   morroc-converter --legacy-db /path/to/legacy --out-dir ./converted
//!   morroc-converter --db /path/to/db/re --npc /path/to/npc/file.txt --out-dir ./converted
//!   morroc-converter --input-dir /path/to/source --out-dir ./converted

use anyhow::Context;
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "morroc-converter", about = "Morroc 数据与脚本转换器")]
struct Cli {
    /// Legacy config 数据库源码根目录。
    #[arg(long, value_name = "DIR")]
    legacy_db: Option<PathBuf>,

    /// YAML 数据库源码根目录（含 db/item_db.yml 等）。
    #[arg(long, value_name = "DIR")]
    yaml_db: Option<PathBuf>,

    /// 数据库目录（item_db.conf 等所在目录）。
    #[arg(long, value_name = "DIR")]
    db: Option<PathBuf>,

    /// NPC 脚本文件。
    #[arg(long, value_name = "FILE")]
    npc: Option<PathBuf>,

    /// 统一输入目录（自动检测 legacy config / YAML 数据库 / NPC）。
    #[arg(long, value_name = "DIR")]
    input_dir: Option<PathBuf>,

    /// 输出目录。
    #[arg(long, value_name = "DIR", default_value = "data/converted")]
    out_dir: PathBuf,

    /// 输出格式：json 或 toml。
    #[arg(long, value_name = "FORMAT", default_value = "json")]
    format: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 确保输出目录存在。
    fs::create_dir_all(&cli.out_dir)
        .with_context(|| format!("创建输出目录 {} 失败", cli.out_dir.display()))?;

    // 来源选项互斥检查。
    let source_flags = [
        cli.legacy_db.is_some(),
        cli.yaml_db.is_some(),
        cli.db.is_some(),
        cli.npc.is_some(),
        cli.input_dir.is_some(),
    ];
    let count = source_flags.iter().filter(|&&b| b).count();
    if count == 0 {
        anyhow::bail!("必须指定 --legacy-db、--yaml-db、--db、--npc 或 --input-dir 之一");
    }
    if count > 1 {
        anyhow::bail!("--legacy-db、--yaml-db、--db、--npc、--input-dir 只能指定一个");
    }

    if let Some(legacy_db) = &cli.legacy_db {
        info!("从 legacy config 目录转换: {}", legacy_db.display());
        let db = morroc_converter::convert_legacy_db(legacy_db)?;
        write_database(&db, &cli.out_dir, &cli.format)?;
        info!(
            "转换完成: {} 个道具, {} 个怪物, {} 个技能, {} 个 NPC",
            db.items.len(),
            db.mobs.len(),
            db.skills.len(),
            db.npcs.len()
        );
    } else if let Some(yaml_db) = &cli.yaml_db {
        info!("从 YAML 数据库目录转换: {}", yaml_db.display());
        let db_dir = find_yaml_db_dir(yaml_db)?;
        let db = morroc_converter::yaml_db::convert_database_dir(&db_dir)?;
        write_database(&db, &cli.out_dir, &cli.format)?;
        info!(
            "转换完成: {} 个道具, {} 个怪物, {} 个技能",
            db.items.len(),
            db.mobs.len(),
            db.skills.len()
        );
    } else if let Some(db_dir) = &cli.db {
        info!("从数据库目录转换: {}", db_dir.display());
        let db = morroc_converter::convert_database_dir(db_dir)?;
        write_database(&db, &cli.out_dir, &cli.format)?;
        info!(
            "转换完成: {} 个道具, {} 个怪物, {} 个技能",
            db.items.len(),
            db.mobs.len(),
            db.skills.len()
        );
    } else if let Some(npc_file) = &cli.npc {
        info!("转换 NPC 脚本: {}", npc_file.display());
        let npc = morroc_converter::convert_npc_file(npc_file)?;
        let out = cli.out_dir.join("npc.json");
        let json = serde_json::to_string_pretty(&npc).context("序列化 NPC 脚本失败")?;
        fs::write(&out, json).with_context(|| format!("写入 {} 失败", out.display()))?;
        info!("已写入 {}", out.display());
    } else if let Some(input_dir) = &cli.input_dir {
        let format = detect_source_format(input_dir)?;
        info!(
            "从统一输入目录转换: {}（识别为 {:?}）",
            input_dir.display(),
            format
        );
        match format {
            SourceFormat::LegacyRepo => {
                let db = morroc_converter::convert_legacy_db(input_dir)?;
                write_database(&db, &cli.out_dir, &cli.format)?;
                info!(
                    "转换完成: {} 个道具, {} 个怪物, {} 个技能, {} 个 NPC",
                    db.items.len(),
                    db.mobs.len(),
                    db.skills.len(),
                    db.npcs.len()
                );
            }
            SourceFormat::LegacyDb => {
                let db = morroc_converter::convert_database_dir(input_dir)?;
                write_database(&db, &cli.out_dir, &cli.format)?;
                info!(
                    "转换完成: {} 个道具, {} 个怪物, {} 个技能",
                    db.items.len(),
                    db.mobs.len(),
                    db.skills.len()
                );
            }
            SourceFormat::YamlRepo => {
                let db_dir = find_yaml_db_dir(input_dir)?;
                let db = morroc_converter::yaml_db::convert_database_dir(&db_dir)?;
                write_database(&db, &cli.out_dir, &cli.format)?;
                info!(
                    "转换完成: {} 个道具, {} 个怪物, {} 个技能",
                    db.items.len(),
                    db.mobs.len(),
                    db.skills.len()
                );
            }
            SourceFormat::YamlDb => {
                let db = morroc_converter::yaml_db::convert_database_dir(input_dir)?;
                write_database(&db, &cli.out_dir, &cli.format)?;
                info!(
                    "转换完成: {} 个道具, {} 个怪物, {} 个技能",
                    db.items.len(),
                    db.mobs.len(),
                    db.skills.len()
                );
            }
            SourceFormat::NpcOnly => {
                let npcs = morroc_converter::convert_npc_dir(input_dir)?;
                let out = cli.out_dir.join("npc.json");
                let json = serde_json::to_string_pretty(&npcs).context("序列化 NPC 脚本失败")?;
                fs::write(&out, json).with_context(|| format!("写入 {} 失败", out.display()))?;
                info!("已写入 {}", out.display());
            }
        }
    }

    Ok(())
}

/// 输入目录格式。
#[derive(Debug, Clone, Copy, PartialEq)]
enum SourceFormat {
    LegacyRepo,
    LegacyDb,
    YamlRepo,
    YamlDb,
    NpcOnly,
}

/// 自动检测输入目录的数据格式。
///
/// 检测顺序：
/// 1. `db/re/item_db.conf` 或 `db/pre-re/item_db.conf` → legacy config 仓库
/// 2. `db/re/item_db.yml` 或 `db/pre-re/item_db.yml` → YAML 数据库仓库
/// 3. `db/item_db.yml` → YAML 数据库仓库
/// 4. 当前目录 `item_db.conf` → legacy config 数据库
/// 5. 当前目录 `item_db.yml` → YAML 数据库
/// 6. 存在 `npc/` 目录 → NPC 脚本
fn detect_source_format(dir: &std::path::Path) -> anyhow::Result<SourceFormat> {
    let db_re = dir.join("db/re");
    let db_pre = dir.join("db/pre-re");
    let db = dir.join("db");

    if db_re.join("item_db.conf").exists() || db_pre.join("item_db.conf").exists() {
        return Ok(SourceFormat::LegacyRepo);
    }
    if db_re.join("item_db.yml").exists() || db_pre.join("item_db.yml").exists() {
        return Ok(SourceFormat::YamlRepo);
    }
    if db.join("item_db.yml").exists() {
        return Ok(SourceFormat::YamlRepo);
    }
    if dir.join("item_db.conf").exists() {
        return Ok(SourceFormat::LegacyDb);
    }
    if dir.join("item_db.yml").exists() {
        return Ok(SourceFormat::YamlDb);
    }
    if dir.join("npc").is_dir() {
        return Ok(SourceFormat::NpcOnly);
    }

    anyhow::bail!(
        "无法识别 {} 的数据格式，请使用 --legacy-db 或 --yaml-db 显式指定",
        dir.display()
    )
}

/// 定位 YAML 数据库目录。
///
/// 优先顺序：db/re/、db/pre-re/、db/、根目录。
fn find_yaml_db_dir(root: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    for sub in ["db/re", "db/pre-re", "db"] {
        let candidate = root.join(sub);
        if candidate.join("item_db.yml").exists() {
            return Ok(candidate);
        }
    }
    if root.join("item_db.yml").exists() {
        return Ok(root.to_path_buf());
    }
    Err(anyhow::anyhow!(
        "无法定位 YAML 数据库目录: {} 及其 db/re、db/pre-re、db 子目录中均不存在 item_db.yml",
        root.display()
    ))
}

fn write_database(
    db: &morroc_converter::schema::GameDatabase,
    out_dir: &std::path::Path,
    format: &str,
) -> anyhow::Result<()> {
    match format {
        "json" => {
            let out = out_dir.join("database.json");
            morroc_converter::write_database_json(db, &out)?;
            info!("已写入 {}", out.display());
        }
        "toml" => {
            let out = out_dir.join("database.toml");
            morroc_converter::write_database_toml(db, &out)?;
            info!("已写入 {}", out.display());
        }
        other => {
            anyhow::bail!("不支持的输出格式: {}", other);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_legacy_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("db/re")).unwrap();
        fs::write(root.join("db/re/item_db.conf"), "item_db: ()").unwrap();
        assert_eq!(
            detect_source_format(root).unwrap(),
            SourceFormat::LegacyRepo
        );
    }

    #[test]
    fn detect_yaml_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("db/re")).unwrap();
        fs::write(root.join("db/re/item_db.yml"), "Body: []").unwrap();
        assert_eq!(detect_source_format(root).unwrap(), SourceFormat::YamlRepo);
    }

    #[test]
    fn detect_legacy_db() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("item_db.conf"), "item_db: ()").unwrap();
        assert_eq!(detect_source_format(root).unwrap(), SourceFormat::LegacyDb);
    }

    #[test]
    fn detect_yaml_db() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("item_db.yml"), "Body: []").unwrap();
        assert_eq!(detect_source_format(root).unwrap(), SourceFormat::YamlDb);
    }

    #[test]
    fn detect_npc_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("npc")).unwrap();
        assert_eq!(detect_source_format(root).unwrap(), SourceFormat::NpcOnly);
    }

    #[test]
    fn find_yaml_db_dir_re() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("db/re")).unwrap();
        fs::write(root.join("db/re/item_db.yml"), "Body: []").unwrap();
        assert_eq!(find_yaml_db_dir(root).unwrap(), root.join("db/re"));
    }

    #[test]
    fn find_yaml_db_dir_db() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("item_db.yml"), "Body: []").unwrap();
        assert_eq!(find_yaml_db_dir(root).unwrap(), root.to_path_buf());
    }
}
