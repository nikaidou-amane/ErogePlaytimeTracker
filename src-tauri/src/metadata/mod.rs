mod erogamescape;
mod vndb;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use erogamescape::ErogameScapeProvider;
use serde::{Deserialize, Serialize};
use url::Url;
use vndb::VndbProvider;

const INVALID_INPUT: &str = "ErogameScapeまたはVNDBのURL・IDを入力してください";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", content = "id")]
pub enum MetadataIdentity {
    #[serde(rename = "erogamescape")]
    ErogameScape(i64),
    #[serde(rename = "vndb")]
    Vndb(String),
}

impl MetadataIdentity {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        let positive_id = |value: &str| -> Result<i64> {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .context(INVALID_INPUT)
        };
        if !input.is_empty() && input.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(Self::ErogameScape(positive_id(input)?));
        }
        if let Some(id) = input.strip_prefix('v').or_else(|| input.strip_prefix('V'))
            && !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_digit())
        {
            return Ok(Self::Vndb(format!("v{}", positive_id(id)?)));
        }
        let url = Url::parse(input).context(INVALID_INPUT)?;
        if !matches!(url.scheme(), "https" | "http")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            bail!(INVALID_INPUT);
        }
        match url.host_str() {
            Some("vndb.org" | "www.vndb.org") => {
                let path = url.path().trim_end_matches('/');
                let id = path.strip_prefix("/v").context(INVALID_INPUT)?;
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                    bail!(INVALID_INPUT);
                }
                Ok(Self::Vndb(format!("v{}", positive_id(id)?)))
            }
            Some("erogamescape.dyndns.org" | "erogamescape.org") => {
                let id = url
                    .query_pairs()
                    .find(|(key, _)| matches!(key.as_ref(), "game" | "game_id" | "id"))
                    .map(|(_, value)| value.into_owned())
                    .or_else(|| {
                        url.path_segments()?
                            .rfind(|part| {
                                !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
                            })
                            .map(str::to_owned)
                    })
                    .context(INVALID_INPUT)?;
                Ok(Self::ErogameScape(positive_id(&id)?))
            }
            _ => bail!(INVALID_INPUT),
        }
    }

    pub fn provider(&self) -> &'static str {
        match self {
            Self::ErogameScape(_) => "erogamescape",
            Self::Vndb(_) => "vndb",
        }
    }

    pub fn url(&self) -> String {
        match self {
            Self::ErogameScape(id) => format!(
                "https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/game.php?game={id}"
            ),
            Self::Vndb(id) => format!("https://vndb.org/{id}"),
        }
    }

    pub fn erogamescape_id(&self) -> Option<i64> {
        match self {
            Self::ErogameScape(id) => Some(*id),
            _ => None,
        }
    }

    pub fn vndb_id(&self) -> Option<&str> {
        match self {
            Self::Vndb(id) => Some(id),
            _ => None,
        }
    }

    pub fn normalized(&self) -> Result<Self> {
        Self::parse(&self.url())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GameMetadata {
    pub source: MetadataIdentity,
    pub title: String,
    pub brand: Option<String>,
    pub release_date: Option<String>,
    pub thumbnail_url: Option<String>,
    pub warnings: Vec<String>,
}
#[async_trait]
pub trait GameMetadataProvider: Send + Sync {
    async fn fetch_game(&self, input: &str) -> Result<GameMetadata>;
}

pub async fn fetch_game(input: &str) -> Result<GameMetadata> {
    let source = MetadataIdentity::parse(input)?;
    match source {
        MetadataIdentity::ErogameScape(_) => {
            ErogameScapeProvider::new()?.fetch_game(&source.url()).await
        }
        MetadataIdentity::Vndb(_) => VndbProvider::new()?.fetch_game(&source.url()).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_and_normalizes_both_providers() {
        for input in [
            "548",
            " https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/game.php?game=548 ",
            "https://erogamescape.org/game.php?game_id=548",
        ] {
            assert_eq!(
                MetadataIdentity::parse(input).unwrap(),
                MetadataIdentity::ErogameScape(548)
            );
        }
        for input in [
            "v548",
            " V00548 ",
            "https://vndb.org/v548",
            "https://www.vndb.org/v548/?view=1#main",
        ] {
            assert_eq!(
                MetadataIdentity::parse(input).unwrap(),
                MetadataIdentity::Vndb("v548".into())
            );
        }
        for source in [
            MetadataIdentity::ErogameScape(548),
            MetadataIdentity::Vndb("v548".into()),
        ] {
            assert_eq!(MetadataIdentity::parse(&source.url()).unwrap(), source);
        }
    }

    #[test]
    fn rejects_other_hosts_entry_types_and_invalid_ids() {
        for input in [
            "",
            "0",
            "-1",
            "v0",
            "v-1",
            "v999999999999999999999",
            "https://vndb.org/r548",
            "https://vndb.org/v548/chars",
            "https://vndb.org.example.test/v548",
            "https://example.test/game.php?game=548",
            "https://vndb.org@localhost/v548",
            "file://vndb.org/v548",
        ] {
            assert!(MetadataIdentity::parse(input).is_err(), "{input}");
        }
    }
}
