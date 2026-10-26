//! Wire types shared by the host, the server and the SDKs. See docs/PROTOCOL.md.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum Action {
    OpenUrl {
        url: String,
    },
    DeepLink {
        url: String,
    },
    Callback {
        name: String,
    },
    Card,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum Icon {
    /// First letter of the title in a circle.
    #[default]
    Monogram,
    /// Base64-encoded PNG, drawn square.
    Png { data: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            background: "#E8F04A".into(),
            foreground: "#2A2C05".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardButton {
    pub label: String,
    pub action: Action,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Card {
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub buttons: Vec<CardButton>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub icon: Icon,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub action: Action,
    #[serde(default)]
    pub card: Option<Card>,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default = "default_duration")]
    pub duration_secs: u32,
    #[serde(default)]
    pub starts_at: Option<i64>,
    #[serde(default)]
    pub ends_at: Option<i64>,
}

fn default_weight() -> u32 {
    1
}

fn default_duration() -> u32 {
    20
}

impl Item {
    pub fn is_live(&self, now: i64) -> bool {
        self.starts_at.is_none_or(|s| now >= s) && self.ends_at.is_none_or(|e| now < e)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entitlements {
    #[serde(default)]
    pub watermark: bool,
    #[serde(default = "default_max_items")]
    pub max_items: u32,
}

fn default_max_items() -> u32 {
    20
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct License {
    pub typ: String,
    pub lic: String,
    pub dev: String,
    pub app_id: String,
    #[serde(default)]
    pub bundle_id: String,
    pub plan: String,
    #[serde(default)]
    pub entitlements: Entitlements,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feed {
    pub typ: String,
    pub app_id: String,
    pub iat: i64,
    pub exp: i64,
    #[serde(default = "default_poll")]
    pub poll_secs: u64,
    #[serde(default)]
    pub allowed_url_prefixes: Vec<String>,
    #[serde(default)]
    pub items: Vec<Item>,
}

fn default_poll() -> u64 {
    300
}
