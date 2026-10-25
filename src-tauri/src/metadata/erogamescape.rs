use super::{GameMetadata, GameMetadataProvider, MetadataIdentity};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use scraper::{Html, Selector};
use url::Url;
pub struct ErogameScapeProvider {
    client: reqwest::Client,
}
impl ErogameScapeProvider {
    pub fn with_proxy(proxy: Option<&str>) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .user_agent("ErogePlaytimeTracker/0.1 (personal metadata client)")
            .timeout(std::time::Duration::from_secs(15));
        if let Some(proxy) = proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        }
        Ok(Self {
            client: builder.build()?,
        })
    }
    fn id(input: &str) -> Result<i64> {
        MetadataIdentity::parse(input)?
            .erogamescape_id()
            .context("ErogameScapeまたはVNDBのURL・IDを入力してください")
    }
    pub(crate) fn parse(id: i64, url: &str, html: &str) -> Result<GameMetadata> {
        let doc = Html::parse_document(html);
        let text = |selectors: &[&str]| -> Option<String> {
            selectors.iter().find_map(|s| {
                Selector::parse(s)
                    .ok()
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|n| n.text().collect::<String>().trim().to_string())
                    .filter(|x| !x.is_empty())
            })
        };
        let title = text(&["#game_title > a", "#game_title", "#soft-title > .bold"])
            .context("タイトルを解析できません")?;
        let brand = text(&["#brand a", "a[href*='brand.php']", ".brand"]);
        let release_date = text(&[
            "#sellday td > a",
            "#sellday td",
            "#soft-title a[href*='toukei_hatubaibi_month.php']",
            "time",
        ])
        .and_then(|x| normalize_date(&x));
        let thumbnail_url = [
            "#main_image img",
            "#game_image img",
            "img.package",
            "img[src*='game']",
        ]
        .iter()
        .find_map(|s| {
            Selector::parse(s)
                .ok()
                .and_then(|sel| doc.select(&sel).next())
                .and_then(|n| n.value().attr("src"))
        })
        .and_then(|src| Url::parse(url).ok()?.join(src).ok())
        .map(|x| x.to_string());
        Ok(GameMetadata {
            source: MetadataIdentity::ErogameScape(id),
            title,
            brand,
            release_date,
            thumbnail_url,
            warnings: Vec::new(),
        })
    }
}
#[async_trait]
impl GameMetadataProvider for ErogameScapeProvider {
    async fn fetch_game(&self, input: &str) -> Result<GameMetadata> {
        let id = Self::id(input)?;
        let url =
            format!("https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/game.php?game={id}");
        let html = self
            .client
            .get(&url)
            .send()
            .await
            .context("ErogameScapeへ接続できません")?
            .error_for_status()
            .context("ErogameScapeがエラーを返しました")?
            .text()
            .await?;
        if html.len() < 100 {
            bail!("ErogameScapeの応答が空です")
        };
        Self::parse(id, &url, &html)
    }
}
fn normalize_date(s: &str) -> Option<String> {
    let normalized: String = s
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    // The date may be surrounded by a table heading or parentheses. Locate a
    // YYYY-MM-DD segment instead of assuming it is the first whitespace token.
    normalized.char_indices().find_map(|(start, _)| {
        let candidate: String = normalized[start..].chars().take(10).collect();
        chrono::NaiveDate::parse_from_str(&candidate, "%Y-%m-%d")
            .ok()
            .map(|date| date.to_string())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_parser() {
        let h = r#"<html><head><title>ErogameScape -エロゲー批評空間-</title></head><body>
        <header><h1><a>ErogameScape -エロゲー批評空間-</a></h1></header>
        <h2 id='game_title'><a>グリザイアの果実 -LE FRUIT DE LA GRISAIA-</a></h2>
        <div id='soft-title'><span class='bold'>グリザイアの果実</span> (<a href='brand.php?brand=275'>FrontWing</a>)</div>
        <div id='main_image'><img src='https://pics.example.test/package.jpg'></div>
        <table><tr id='brand'><th>ブランド</th><td><a href='brand.php?brand=275'>FrontWing</a></td></tr>
        <tr id='sellday'><th>発売日</th><td><a href='date.php'>2011-02-25</a></td></tr></table></body></html>"#;
        let m =
            ErogameScapeProvider::parse(42, "https://example.test/game.php?game=42", h).unwrap();
        assert_eq!(m.title, "グリザイアの果実 -LE FRUIT DE LA GRISAIA-");
        assert_eq!(m.brand.as_deref(), Some("FrontWing"));
        assert_eq!(m.release_date.as_deref(), Some("2011-02-25"));
        assert_eq!(
            m.thumbnail_url.as_deref(),
            Some("https://pics.example.test/package.jpg")
        );
    }

    #[test]
    fn date_can_be_extracted_from_surrounding_text() {
        assert_eq!(
            normalize_date("発売日 (2011/02/25)"),
            Some("2011-02-25".into())
        );
    }
    #[test]
    fn extracts_id() {
        assert_eq!(ErogameScapeProvider::id("123").unwrap(), 123);
        assert_eq!(
            ErogameScapeProvider::id("https://erogamescape.dyndns.org/game.php?game=99").unwrap(),
            99
        );
    }
}
