//! This is a [Rust](https://rust-lang.org/) library to help you create chat bots for [racetime.gg](https://racetime.gg/).
//!
//! For documentation, see also <https://github.com/racetimeGG/racetime-app/wiki/Category-bots>.

#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        collections::BTreeMap,
        time::Duration,
    },
    collect_mac::collect,
    itertools::Itertools as _,
    lazy_regex::regex_captures,
    serde::Deserialize,
    url::Url,
};
pub use crate::{
    bot::Bot,
    handler::RaceHandler,
};

pub mod bot;
pub mod handler;
pub mod model;

const RACETIME_HOST: &str = "racetime.gg";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)] Custom(#[from] Box<dyn std::error::Error + Send>),
    #[error(transparent)] HeaderToStr(#[from] reqwest::header::ToStrError),
    #[error(transparent)] InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
    #[error(transparent)] Io(#[from] std::io::Error),
    #[error(transparent)] Json(#[from] serde_json::Error),
    #[error(transparent)] Task(#[from] tokio::task::JoinError),
    #[error(transparent)] UrlParse(#[from] url::ParseError),
    #[error("websocket connection closed by the server")]
    EndOfStream,
    #[error("the startrace location did not match the input category")]
    LocationCategory,
    #[error("the startrace location header did not have the expected format")]
    LocationFormat,
    #[error("the startrace response did not include a location header")]
    MissingLocationHeader,
    #[error("HTTP error{}: {0}", if let Some(url) = .0.url() { format!(" at {url}") } else { String::default() })]
    Reqwest(#[from] reqwest::Error),
    #[error("server errors:{}", .0.into_iter().map(|msg| format!("\n• {msg}")).format(""))]
    Server(Vec<String>),
    #[error("WebSocket error: {0}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("expected text message from websocket, but received {0:?}")]
    UnexpectedMessageType(tokio_tungstenite::tungstenite::Message),
}

/// Generate a HTTP/HTTPS URI from the given URL path fragment.
fn http_uri(url: &str) -> Result<Url, Error> {
    uri("https", url)
}

/// Generate a URI from the given protocol and URL path fragment.
fn uri(proto: &str, url: &str) -> Result<Url, Error> {
    Ok(format!("{proto}://{RACETIME_HOST}{url}").parse()?)
}

/// Get an OAuth2 token from the authentication server.
pub async fn authorize(client_id: &str, client_secret: &str, client: &reqwest::Client) -> Result<(String, Duration), Error> {
    #[derive(Deserialize)]
    struct AuthResponse {
        access_token: String,
        expires_in: Option<u64>,
    }

    let data = client.post(http_uri("/o/token")?)
        .form(&collect![as BTreeMap<_, _>:
            "client_id" => client_id,
            "client_secret" => client_secret,
            "grant_type" => "client_credentials",
        ])
        .send().await?
        .error_for_status()?
        .json::<AuthResponse>().await?;
    Ok((
        data.access_token,
        Duration::from_secs(data.expires_in.unwrap_or(36000)),
    ))
}

pub struct StartRace {
    pub goal: String,
    pub goal_is_custom: bool,
    pub team_race: bool,
    pub invitational: bool,
    pub unlisted: bool,
    pub info_user: String,
    pub info_bot: String,
    pub require_even_teams: bool,
    pub start_delay: u8,
    pub time_limit: u8,
    pub time_limit_auto_complete: bool,
    pub streaming_required: Option<bool>,
    pub auto_start: bool,
    pub allow_comments: bool,
    pub hide_comments: bool,
    pub allow_prerace_chat: bool,
    pub allow_midrace_chat: bool,
    pub allow_non_entrant_chat: bool,
    pub chat_message_delay: u8,
}

impl StartRace {
    /// Creates a race room with the specified configuration and returns its slug.
    ///
    /// An access token can be obtained using [`authorize`].
    pub async fn start(&self, access_token: &str, client: &reqwest::Client, category: &str) -> Result<String, Error> {
        fn form_bool(value: bool) -> &'static str { if value { "1" } else { "0" } }

        let start_delay = self.start_delay.to_string();
        let time_limit = self.time_limit.to_string();
        let chat_message_delay = self.chat_message_delay.to_string();
        let mut form = collect![as BTreeMap<_, _>:
            if self.goal_is_custom { "custom_goal" } else { "goal" } => &*self.goal,
            "team_race" => form_bool(self.team_race),
            "invitational" => form_bool(self.invitational),
            "unlisted" => form_bool(self.unlisted),
            "info_user" => &*self.info_user,
            "info_bot" => &*self.info_bot,
            "require_even_teams" => form_bool(self.require_even_teams),
            "start_delay" => &*start_delay,
            "time_limit" => &*time_limit,
            "time_limit_auto_complete" => form_bool(self.time_limit_auto_complete),
            "auto_start" => form_bool(self.auto_start),
            "allow_comments" => form_bool(self.allow_comments),
            "hide_comments" => form_bool(self.hide_comments),
            "allow_prerace_chat" => form_bool(self.allow_prerace_chat),
            "allow_midrace_chat" => form_bool(self.allow_midrace_chat),
            "allow_non_entrant_chat" => form_bool(self.allow_non_entrant_chat),
            "chat_message_delay" => &*chat_message_delay,
        ];
        if let Some(streaming_required) = self.streaming_required {
            form.insert("streaming_required", form_bool(streaming_required));
        }
        let response = client.post(http_uri(&format!("/o/{category}/startrace"))?)
            .bearer_auth(access_token)
            .form(&form)
            .send().await?
            .error_for_status()?;
        let location = response
            .headers()
            .get("location").ok_or(Error::MissingLocationHeader)?
            .to_str()?;
        let (_, location_category, slug) = regex_captures!("^/([^/]+)/([^/]+)$", location).ok_or(Error::LocationFormat)?;
        if location_category != category { return Err(Error::LocationCategory) }
        Ok(slug.to_owned())
    }
}
