use super::{GameMetadata, GameMetadataProvider, MetadataIdentity};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;

pub struct VndbProvider {
    client: reqwest::Client,
}
impl VndbProvider {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("ErogePlaytimeTracker/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(15))
                .build()?,
        })
    }
}

#[derive(Deserialize)]
struct Response {
    results: Vec<VisualNovel>,
}
#[derive(Deserialize)]
struct VisualNovel {
    id: String,
    title: String,
    titles: Vec<Title>,
    developers: Vec<Developer>,
    released: Option<String>,
    image: Option<Image>,
}
#[derive(Deserialize)]
struct Title {
    lang: String,
    title: String,
    main: bool,
}
#[derive(Deserialize)]
struct Developer {
    name: String,
    original: Option<String>,
}
#[derive(Deserialize)]
struct Image {
    url: String,
}

impl Response {
    fn metadata(self, expected: MetadataIdentity) -> Result<GameMetadata> {
        let vn = self
            .results
            .into_iter()
            .next()
            .context("VNDBのゲームが見つかりません")?;
        let source = MetadataIdentity::parse(&vn.id)?;
        if source != expected {
            bail!("VNDBの応答を解析できません");
        }
        let title = vn
            .titles
            .iter()
            .find(|t| t.lang == "ja" && !t.title.trim().is_empty())
            .or_else(|| {
                vn.titles
                    .iter()
                    .find(|t| t.main && !t.title.trim().is_empty())
            })
            .map(|t| t.title.as_str())
            .unwrap_or(&vn.title)
            .trim()
            .to_string();
        if title.is_empty() {
            bail!("タイトルを解析できません");
        }
        let brand = vn
            .developers
            .first()
            .map(|d| {
                d.original
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&d.name)
                    .trim()
                    .to_string()
            })
            .filter(|name| !name.is_empty());
        let release_date = vn
            .released
            .as_deref()
            .filter(|date| date.len() == 10)
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .map(|date| date.to_string());
        let mut warnings = Vec::new();
        if vn.released.is_some() && release_date.is_none() {
            warnings.push("発売日を年月日まで取得できなかったため、発売日は未設定です。".into());
        }
        Ok(GameMetadata {
            source,
            title,
            brand,
            release_date,
            thumbnail_url: vn.image.map(|image| image.url),
            warnings,
        })
    }
}

#[async_trait]
impl GameMetadataProvider for VndbProvider {
    async fn fetch_game(&self, input: &str) -> Result<GameMetadata> {
        let source = MetadataIdentity::parse(input)?;
        let id = source
            .vndb_id()
            .context("ErogameScapeまたはVNDBのURL・IDを入力してください")?;
        let response = self.client.post("https://api.vndb.org/kana/vn")
            .json(&serde_json::json!({
                "filters": ["id", "=", id],
                "fields": "title,titles{lang,title,main},developers{name,original},released,image.url",
                "results": 1
            }))
            .send().await.context("VNDBへ接続できません")?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            bail!("VNDBの取得回数制限に達しました。しばらく待ってからお試しください。");
        }
        response
            .error_for_status()
            .context("VNDBがエラーを返しました")?
            .json::<Response>()
            .await
            .context("VNDBの応答を解析できません")?
            .metadata(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::json!({"results": [{
            "id": "v548", "title": "Romanized title",
            "titles": [
                {"lang": "en", "title": "English title", "main": true},
                {"lang": "ja", "title": "日本語タイトル", "main": false}
            ],
            "developers": [{"name": "First brand", "original": "ブランド"}, {"name": "Second brand", "original": null}],
            "released": "2009-07-24",
            "image": {"url": "https://t.vndb.org/cv/16/79716.jpg", "sexual": 2, "violence": 2}
        }], "more": false})
    }
    fn parse(json: serde_json::Value) -> Result<GameMetadata> {
        serde_json::from_value::<Response>(json)?.metadata(MetadataIdentity::Vndb("v548".into()))
    }
    #[test]
    fn normalizes_titles_first_developer_date_and_flagged_image() {
        let m = parse(fixture()).unwrap();
        assert_eq!(m.title, "日本語タイトル");
        assert_eq!(m.brand.as_deref(), Some("ブランド"));
        assert_eq!(m.release_date.as_deref(), Some("2009-07-24"));
        assert_eq!(
            m.thumbnail_url.as_deref(),
            Some("https://t.vndb.org/cv/16/79716.jpg")
        );
        assert!(m.warnings.is_empty());
        let mut json = fixture();
        json["results"][0]["image"]["sexual"] = serde_json::json!(0);
        assert_eq!(parse(json).unwrap().thumbnail_url, m.thumbnail_url);
    }
    #[test]
    fn handles_missing_image_brand_and_partial_or_unknown_dates() {
        for date in [
            serde_json::json!("2009"),
            serde_json::json!("2009-07"),
            serde_json::json!("TBA"),
            serde_json::json!("2009-02-30"),
            serde_json::Value::Null,
        ] {
            let has_date = !date.is_null();
            let mut json = fixture();
            let vn = &mut json["results"][0];
            vn["released"] = date;
            vn["image"] = serde_json::Value::Null;
            vn["developers"] = serde_json::json!([]);
            vn["titles"] =
                serde_json::json!([{ "lang": "en", "title": "Original title", "main": true }]);
            let m = parse(json).unwrap();
            assert_eq!(m.title, "Original title");
            assert!(m.release_date.is_none());
            assert!(m.brand.is_none());
            assert!(m.thumbnail_url.is_none());
            assert_eq!(!m.warnings.is_empty(), has_date);
        }
        let mut json = fixture();
        json["results"][0]["titles"] = serde_json::json!([]);
        json["results"][0]["developers"][0]["original"] = serde_json::Value::Null;
        let m = parse(json).unwrap();
        assert_eq!(m.title, "Romanized title");
        assert_eq!(m.brand.as_deref(), Some("First brand"));
    }
    #[test]
    fn rejects_missing_or_mismatched_entries() {
        assert!(parse(serde_json::json!({"results": []})).is_err());
        let mut json = fixture();
        json["results"][0]["id"] = serde_json::json!("v1");
        assert!(parse(json).is_err());
    }
}
