//! Bounded, data-only local animation packs. No archives, scripts, URLs or HTTP control port.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

const MANIFEST_LIMIT: u64 = 128 * 1024;
const FILE_LIMIT: u64 = 32 * 1024 * 1024;
const PACK_LIMIT: u64 = 200 * 1024 * 1024;
const SLOTS: &[&str] = &[
    "idle",
    "blink",
    "look",
    "think",
    "walk-left",
    "walk-right",
    "rest",
    "happy",
    "greet",
    "pat",
    "poke",
    "tail",
    "eat",
    "sweep",
    "wake",
    "talk",
    "annoyed",
    "dizzy",
    "land",
    "drag",
    "fall",
    "working",
    "waiting",
    "ended",
    "interrupted",
    "toolError",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Clip {
    pub id: String,
    pub slot: String,
    pub weight: u8,
    pub frame_ms: u32,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub poster: String,
    pub frames: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub clips: Vec<Clip>,
}
#[derive(Debug, Serialize)]
pub struct InstalledPack {
    #[serde(flatten)]
    pub pack: Pack,
    pub directory: String,
}
pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && ![
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ]
        .contains(&value)
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn filename(value: &str, extension: &str) -> bool {
    value.strip_suffix(extension).is_some_and(valid_id)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "素材文件不存在")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return Err("素材文件类型或大小不符合要求".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("素材文件超出大小限制".into());
    }
    Ok(bytes)
}
fn parse(path: &Path) -> Result<Pack, String> {
    let pack: Pack = serde_json::from_slice(&read_bounded(path, MANIFEST_LIMIT)?)
        .map_err(|_| "动画清单格式无效")?;
    if pack.schema != 1
        || !valid_id(&pack.id)
        || pack.id == "builtin"
        || pack.name.is_empty()
        || pack.name.chars().count() > 64
        || pack.name.chars().any(char::is_control)
        || pack.clips.is_empty()
        || pack.clips.len() > 64
    {
        return Err("动画包信息超出范围".into());
    }
    let mut ids = BTreeSet::new();
    for c in &pack.clips {
        if !valid_id(&c.id)
            || !ids.insert(&c.id)
            || !SLOTS.contains(&c.slot.as_str())
            || !(1..=10).contains(&c.weight)
            || !(40..=5000).contains(&c.frame_ms)
            || c.frames.len() > 120
            || !filename(&c.poster, ".png")
            || (c.frames.is_empty() == c.video.is_none())
            || c.frames.iter().any(|f| !filename(f, ".png"))
            || c.video.as_ref().is_some_and(|v| !filename(v, ".webm"))
        {
            return Err("动作配置无效；每个动作请选择 PNG 序列或 WebM 视频".into());
        }
    }
    Ok(pack)
}
fn asset_names(pack: &Pack) -> BTreeSet<&str> {
    pack.clips
        .iter()
        .flat_map(|c| {
            std::iter::once(c.poster.as_str())
                .chain(c.frames.iter().map(String::as_str))
                .chain(c.video.iter().map(String::as_str))
        })
        .collect()
}
fn check_asset(name: &str, bytes: &[u8]) -> Result<(), String> {
    // All posters/frames use the same canvas and foot line. Header checks precede browser decoding.
    if name.ends_with(".png") {
        if bytes.len() < 33
            || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
            || &bytes[12..16] != b"IHDR"
            || u32::from_be_bytes(bytes[16..20].try_into().unwrap()) != 256
            || u32::from_be_bytes(bytes[20..24].try_into().unwrap()) != 256
            || bytes[25] != 6
        {
            return Err("PNG 必须是 256×256 RGBA 图片".into());
        }
    } else if bytes.len() < 4 || bytes[..4] != [0x1a, 0x45, 0xdf, 0xa3] {
        return Err("WebM 文件头无效".into());
    }
    Ok(())
}
pub fn list(root: &Path) -> Vec<InstalledPack> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut packs: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            if !e.file_type().ok()?.is_dir() {
                return None;
            }
            let pack = parse(&e.path().join("manifest.json")).ok()?;
            if e.file_name().to_str() != Some(pack.id.as_str()) {
                return None;
            }
            Some(InstalledPack {
                pack,
                directory: e.path().to_string_lossy().into_owned(),
            })
        })
        .take(32)
        .collect();
    packs.sort_by(|a, b| a.pack.name.cmp(&b.pack.name));
    packs
}
pub fn import(manifest: &Path, root: &Path) -> Result<InstalledPack, String> {
    let pack = parse(manifest)?;
    let parent = manifest
        .parent()
        .ok_or("清单缺少目录")?
        .canonicalize()
        .map_err(|_| "无法读取素材目录")?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    if fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(valid_id))
        .count()
        >= 32
    {
        return Err("最多保存 32 个动画包".into());
    }
    let destination = root.join(&pack.id);
    if destination.exists() {
        return Err("同名动画包已存在，请修改清单 id 后重新导入".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = root.join(format!(".import-{}-{stamp}", std::process::id()));
    fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result: Result<(), String> = (|| {
        let mut total = 0;
        for name in asset_names(&pack) {
            let source = parent.join(name);
            if source
                .canonicalize()
                .map_err(|_| "素材文件不存在")?
                .parent()
                != Some(parent.as_path())
            {
                return Err("素材必须位于清单同一目录".into());
            }
            let bytes = read_bounded(&source, FILE_LIMIT)?;
            total += bytes.len() as u64;
            if total > PACK_LIMIT {
                return Err("动画包超过 200 MiB".into());
            }
            check_asset(name, &bytes)?;
            fs::write(temporary.join(name), bytes).map_err(|e| e.to_string())?;
        }
        fs::write(
            temporary.join("manifest.json"),
            serde_json::to_vec_pretty(&pack).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result?;
    Ok(InstalledPack {
        pack,
        directory: destination.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> std::path::PathBuf {
        // Header probe only; actual decoder validation is a frontend responsibility.
        let mut png = vec![0; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&256u32.to_be_bytes());
        png[20..24].copy_from_slice(&256u32.to_be_bytes());
        png[25] = 6;
        fs::write(root.join("idle.png"), png).unwrap();
        let p = root.join("manifest.json");
        fs::write(&p,r#"{"schema":1,"id":"test-pack","name":"Test","clips":[{"id":"one","slot":"idle","weight":1,"frameMs":120,"loop":true,"poster":"idle.png","frames":["idle.png"]}]}"#).unwrap();
        p
    }
    #[test]
    fn import_list_and_duplicate_are_bounded() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        assert_eq!(import(&p, dest.path()).unwrap().pack.id, "test-pack");
        assert_eq!(list(dest.path()).len(), 1);
        assert!(import(&p, dest.path()).is_err());
    }
    #[test]
    fn traversal_unknown_fields_and_oversize_rejected() {
        let source = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        let good = fs::read_to_string(&p).unwrap();
        for bad in [
            good.replace("idle.png", "../idle.png"),
            good.replace("120", "1"),
            good.replace("\"schema\":1", "\"schema\":1,\"script\":\"x\""),
            good.replace("\"test-pack\"", "\"builtin\""),
        ] {
            fs::write(&p, bad).unwrap();
            assert!(parse(&p).is_err());
        }
        fs::write(&p, vec![b' '; MANIFEST_LIMIT as usize + 1]).unwrap();
        assert!(parse(&p).is_err());
    }
    #[test]
    fn missing_bad_media_and_partial_import_cleanup() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        fs::write(source.path().join("idle.png"), b"bad").unwrap();
        assert!(import(&p, dest.path()).is_err());
        assert_eq!(fs::read_dir(dest.path()).unwrap().count(), 0);
        fs::remove_file(source.path().join("idle.png")).unwrap();
        assert!(import(&p, dest.path()).is_err());
        assert!(list(dest.path()).is_empty());
    }
    #[test]
    fn png_dimensions_and_rgba_required() {
        assert!(!valid_id("../bad"));
        assert!(!valid_id("C:drive"));
        assert!(!valid_id("pack_name"));
        assert!(valid_id("penguin-1"));
        assert!(!valid_id("nul"));
        assert!(!filename("con.png", ".png"));
        assert!(check_asset("one.webm", &[0x1a, 0x45, 0xdf, 0xa3]).is_ok());
        assert!(check_asset("one.webm", b"not-video").is_err());
    }
    #[test]
    fn actual_penguin_video_pack_roundtrips_without_modifying_source() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        manifest["clips"][0]["frames"] = serde_json::json!([]);
        manifest["clips"][0]["video"] = serde_json::json!("greeting.webm");
        fs::write(&p, serde_json::to_vec(&manifest).unwrap()).unwrap();
        fs::write(
            source.path().join("idle.png"),
            include_bytes!("../../penguin/companion-v2/wave-0.png"),
        )
        .unwrap();
        let video = include_bytes!("../../penguin/companion-v2/greeting.webm");
        fs::write(source.path().join("greeting.webm"), video).unwrap();
        let installed = import(&p, dest.path()).unwrap();
        assert_eq!(
            installed.pack.clips[0].video.as_deref(),
            Some("greeting.webm")
        );
        assert_eq!(
            fs::read(dest.path().join("test-pack/greeting.webm")).unwrap(),
            video
        );
        assert_eq!(
            fs::read(source.path().join("greeting.webm")).unwrap(),
            video
        );
    }
    #[test]
    fn oversized_files_and_pack_count_fail_before_copying() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        fs::File::create(source.path().join("idle.png"))
            .unwrap()
            .set_len(FILE_LIMIT + 1)
            .unwrap();
        assert!(import(&p, dest.path()).is_err());
        assert!(fs::read_dir(dest.path()).unwrap().next().is_none());
        for n in 0..32 {
            fs::create_dir(dest.path().join(format!("pack-{n}"))).unwrap();
        }
        assert!(import(&p, dest.path()).unwrap_err().contains("32"));
    }
}
