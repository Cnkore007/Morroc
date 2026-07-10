use regex::Regex;
use std::fs;
use std::path::Path;

fn main() {
    let hercules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/hercules");

    let len_file = hercules.join("src/common/packets/packets2019_len_main.h");

    let contents = fs::read_to_string(&len_file).expect("无法读取 Hercules 包长度表");

    let re = Regex::new(r"packetLen\(\s*(0x[0-9a-fA-F]+)\s*,\s*(-?\d+)\s*\)").unwrap();
    let mut entries: Vec<(u16, i16)> = re
        .captures_iter(&contents)
        .map(|cap| {
            let id = u16::from_str_radix(&cap[1][2..], 16).unwrap();
            let len = cap[2].parse::<i16>().unwrap();
            (id, len)
        })
        .collect();

    entries.sort_by_key(|(id, _)| *id);

    let mut out = String::new();
    out.push_str("// 本文件由 build.rs 自动生成，不要手动修改。\n");
    out.push_str("// 来源: vendor/hercules/src/common/packets/packets2019_len_main.h\n");
    out.push('\n');
    out.push_str("/// 包长度查找表（packet_id -> length）。\n");
    out.push_str("/// length 为 -1 表示动态长度（包内含长度字段）。\n");
    out.push_str("pub const LENGTHS: &[(u16, i16)] = &[\n");
    for (id, len) in &entries {
        out.push_str(&format!("    (0x{:04x}, {}),\n", id, len));
    }
    out.push_str("];\n");
    out.push('\n');
    out.push_str("/// 查询包长度。\n");
    out.push_str("pub fn packet_len(packet_id: u16) -> Option<i16> {\n");
    out.push_str("    LENGTHS\n");
    out.push_str("        .binary_search_by_key(&packet_id, |(id, _)| *id)\n");
    out.push_str("        .ok()\n");
    out.push_str("        .map(|i| LENGTHS[i].1)\n");
    out.push_str("}\n");
    out.push('\n');
    out.push_str("/// 2019 main 分支的默认包 ID 混淆密钥（20190530 等版本无混淆）。\n");
    out.push_str(
        "pub const CRYPTO_KEYS: (u32, u32, u32) = (0x00000000, 0x00000000, 0x00000000);\n",
    );

    let out_path =
        Path::new(&std::env::var_os("OUT_DIR").expect("OUT_DIR 未设置")).join("lengths.rs");
    fs::write(&out_path, out).expect("无法写入生成的长度表");

    println!("cargo:rerun-if-changed={}", len_file.display());
}
