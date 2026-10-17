use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

const SUPPORTED_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

pub async fn download(client: &reqwest::Client, url: &str, dir: &Path) -> Result<PathBuf> {
    let bytes = client
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let ext = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments()?.next_back().map(str::to_owned))
        .and_then(|n| Path::new(&n).extension()?.to_str().map(str::to_owned))
        .map(|extension| extension.to_ascii_lowercase())
        .filter(|extension| SUPPORTED_EXTENSIONS.contains(&extension.as_str()))
        .unwrap_or_else(|| "jpg".into());
    let name = format!("{:x}.{}", Sha256::digest(url.as_bytes()), ext);
    let path = dir.join(name);
    std::fs::write(&path, bytes)
        .with_context(|| format!("thumbnailを書き込めません: {}", path.display()))?;
    Ok(path)
}

pub fn import_local(source: &str, dir: &Path) -> Result<PathBuf> {
    let source = Path::new(source);
    if !source.is_file() {
        anyhow::bail!("サムネイル画像が見つかりません")
    }
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| SUPPORTED_EXTENSIONS.contains(&extension.as_str()))
        .ok_or_else(|| anyhow::anyhow!("サムネイルはJPG、PNG、WebPを選択してください"))?;
    let file = std::fs::File::open(source).context("サムネイル画像を読み取れません")?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .context("サムネイル画像を読み取れません")?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let name = format!("{:x}.{extension}", digest.finalize());
    let path = dir.join(name);
    if source != path {
        std::fs::copy(source, &path)
            .with_context(|| format!("thumbnailを書き込めません: {}", path.display()))?;
    }
    Ok(path)
}

pub fn store_png(bytes: &[u8], dir: &Path) -> Result<PathBuf> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        anyhow::bail!("トリミング画像がPNG形式ではありません")
    }
    let path = dir.join(format!("{:x}.png", Sha256::digest(bytes)));
    std::fs::write(&path, bytes)
        .with_context(|| format!("thumbnailを書き込めません: {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("eroge-playtime-tracker-{name}-{unique}"))
    }

    #[test]
    fn imports_supported_local_thumbnail_into_cache() {
        let root = test_directory("thumbnail-import");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let source = root.join("cover.PNG");
        std::fs::write(&source, b"image bytes").unwrap();

        let imported = import_local(source.to_str().unwrap(), &cache).unwrap();

        assert_eq!(imported.parent(), Some(cache.as_path()));
        assert_eq!(
            imported.extension().and_then(|value| value.to_str()),
            Some("png")
        );
        assert_eq!(std::fs::read(&imported).unwrap(), b"image bytes");
        assert_eq!(
            import_local(imported.to_str().unwrap(), &cache).unwrap(),
            imported
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsupported_local_thumbnail() {
        let root = test_directory("thumbnail-extension");
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("cover.gif");
        std::fs::write(&source, b"image bytes").unwrap();

        let error = import_local(source.to_str().unwrap(), &root).unwrap_err();

        assert!(error.to_string().contains("JPG、PNG、WebP"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stores_cropped_png_in_thumbnail_cache() {
        let root = test_directory("thumbnail-crop");
        std::fs::create_dir_all(&root).unwrap();
        let png = b"\x89PNG\r\n\x1a\ncropped image";

        let stored = store_png(png, &root).unwrap();

        assert_eq!(stored.parent(), Some(root.as_path()));
        assert_eq!(
            stored.extension().and_then(|value| value.to_str()),
            Some("png")
        );
        assert_eq!(std::fs::read(stored).unwrap(), png);
        std::fs::remove_dir_all(root).unwrap();
    }
}
