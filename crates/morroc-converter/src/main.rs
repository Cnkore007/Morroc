//! Morroc Converter CLI
//!
//! 用法：
//!   morroc-converter --hercules /path/to/hercules --out-dir ./converted
//!   morroc-converter --db /path/to/db/re --npc /path/to/npc/file.txt --out-dir ./converted

use anyhow::Context;
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "morroc-converter", about = "Morroc 数据与脚本转换器")]
struct Cli {
    /// Hercules 源码根目录。
    #[arg(long, value_name = "DIR")]
    hercules: Option<PathBuf>,

    /// rAthena 源码根目录（含 db/item_db.yml 等）。
    #[arg(long, value_name = "DIR")]
    rathena: Option<PathBuf>,

    /// 数据库目录（item_db.conf 等所在目录）。
    #[arg(long, value_name = "DIR")]
    db: Option<PathBuf>,

    /// NPC 脚本文件。
    #[arg(long, value_name = "FILE")]
    npc: Option<PathBuf>,

    /// 输出目录。
    #[arg(long, value_name = "DIR", default_value = "./converted")]
    out_dir: PathBuf,

    /// 输出格式：json 或 toml。
    #[arg(long, value_name = "FORMAT", default_value = "json")]
    format: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    fs::create_dir_all(&cli.out_dir)
        .with_context(|| format!("创建输出目录 {} 失败", cli.out_dir.display()))?;

    if let Some(hercules) = &cli.hercules {
        info!("从 Hercules 目录转换: {}", hercules.display());
        let db = morroc_converter::convert_hercules(hercules)?;
        write_database(&db, &cli.out_dir, &cli.format)?;
        info!(
            "转换完成: {} 个道具, {} 个怪物, {} 个技能, {} 个 NPC",
            db.items.len(),
            db.mobs.len(),
            db.skills.len(),
            db.npcs.len()
        );
    } else if let Some(rathena) = &cli.rathena {
        info!("从 rAthena 目录转换: {}", rathena.display());
        let db_dir = find_rathena_db_dir(rathena)?;
        let db = morroc_converter::rathena::convert_database_dir(&db_dir)?;
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
    } else {
        eprintln!("错误: 必须指定 --hercules、--rathena、--db 或 --npc 之一");
        std::process::exit(1);
    }

    Ok(())
}

fn find_rathena_db_dir(root: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    if root.join("item_db.yml").exists() {
        return Ok(root.to_path_buf());
    }
    let db = root.join("db");
    if db.join("item_db.yml").exists() {
        return Ok(db);
    }
    Err(anyhow::anyhow!(
        "无法定位 rAthena 数据库目录: {} 或其 db 子目录中不存在 item_db.yml",
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
