use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, FuzzySelect, Input};
use spotify_rs::model::PlayableItem;
use spotify_rs::{AuthCodeClient, RedirectUrl, Token};
use std::fs::File;
use std::io::{Read, Write};
use std::time::Duration;
use tokio::time::sleep;

async fn program(client: &mut AuthCodeClient<Token>) -> bool {
    let theme: ColorfulTheme = ColorfulTheme::default();

    let Ok(track) = spotify_rs::get_currently_playing_track(None, client).await else {
        return false;
    };
    let Some(track) = track.item else {
        return false;
    };
    let track = Track::from_playable_item(track);

    let Ok(user) = spotify_rs::get_current_user_profile(client).await else {
        println!("Failed to get user profile!");
        return false;
    };

    let Ok(playlists) = spotify_rs::current_user_playlists().limit(50).get(client).await else {
        println!("Failed to get user playlists!");
        return false;
    };
    let playlists: Vec<_> = playlists
        .filtered_items()
        .into_iter()
        .filter(|p| p.owner.id == user.id)
        .collect();
    let playlist_names = playlists
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>();
    let playlist_ids = playlists.iter().map(|p| p.id.as_str()).collect::<Vec<_>>();

    let items = vec!["Skip", "Reload", "Remove", "Quit"];
    let items_len = items.len();
    let items = [items, playlist_names].concat();

    let Ok(selection) = FuzzySelect::with_theme(&theme)
        .with_prompt(format!(
            "Currently playing: {} by {}",
            track.name,
            track.artists.join(", "),
        ))
        .items(&items)
        .interact()
    else {
        println!("Invalid selection!");
        return true;
    };

    match selection {
        0 => {
            save_current_track(client).await;
            let _ = spotify_rs::skip_to_next(None, client).await;
            sleep(Duration::from_secs(1)).await;
            return true;
        }
        1 => return true,
        2 => {
            if spotify_rs::remove_saved_tracks(&[track.id.as_str()], client)
                .await
                .is_err()
            {
                println!("Failed to remove track from library!");
            };
            let _ = spotify_rs::skip_to_next(None, client).await;
            sleep(Duration::from_secs(1)).await;
            return true;
        }
        3 => return false,
        i => {
            if i < items_len {
                panic!("Selected an action that isn't implemented yet!");
            }
            save_current_track(client).await;
            let playlist_id = playlist_ids[i - items_len];
            if spotify_rs::
                add_items_to_playlist(playlist_id, &[track.uri.as_str()])
                .send(client)
                .await
                .is_err()
            {
                println!(
                    "Failed to add track to playlist!\nTrack: {}\nPlaylist: {}",
                    track.uri, playlist_id
                );
            }
        }
    }

    let _: String = Input::with_theme(&theme)
        .with_prompt("Press Enter to continue when the song is finished")
        .allow_empty(true)
        .interact_text()
        .unwrap();

    true
}

#[tokio::main]
async fn main() {
    let theme: ColorfulTheme = ColorfulTheme::default();

    let mut client = match auth().await {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to authenticate, {}", e);
            return;
        }
    };

    if let Ok(mut f) = File::open("last.txt") {
        let mut last = String::new();
        if f.read_to_string(&mut last).is_err() {
            println!("Failed to read last.txt");
            return;
        };
        let Ok(user) = spotify_rs::get_current_user_profile(&client).await else {
            println!("Failed to get user profile!");
            return;
        };
        if let Err(e) = spotify_rs::
            start_playback()
            .context_uri(format!("spotify:user:{}:collection", user.id))
            .offset_uri(last.as_str())
            .send(&client)
            .await
        {
            println!("Failed to resume because {}", e);
            return;
        };
        sleep(Duration::from_secs(1)).await;
    } else {
        let confirmation = Confirm::with_theme(&theme)
            .with_prompt("No last track found! Play a track yourself to start the program")
            .interact();
        if !confirmation.is_ok_and(|b| b) {
            return;
        }
    }

    let _ = spotify_rs::toggle_playback_shuffle(false).send(&client).await;

    while program(&mut client).await {}

    save_current_track(&mut client).await;
}

async fn save_current_track(
    client: &mut AuthCodeClient<Token>,
) {
    if let Some(last) = spotify_rs::get_currently_playing_track(None, client)
        .await
        .ok()
        .and_then(|i| i.item)
    {
        let Ok(mut f) = File::create("last.txt") else {
            println!("Failed to create last.txt");
            return;
        };
        let uri = Track::from_playable_item(last).uri;
        if f.write(uri.as_bytes()).is_err() {
            println!("Failed to write to last.txt");
        };
    }
}

struct Track {
    uri: String,
    id: String,
    name: String,
    artists: Vec<String>,
}

impl Track {
    fn from_playable_item(item: PlayableItem) -> Self {
        match item {
            PlayableItem::Track(t) => Self {
                uri: t.uri,
                id: t.id,
                name: t.name,
                artists: t.artists.iter().map(|a| a.name.clone()).collect(),
            },
            PlayableItem::Episode(e) => Self {
                uri: e.uri,
                id: e.id,
                name: e.name,
                artists: vec![e.show.name],
            },
        }
    }
}

const CLIENT_ID: &str = "dd5192114eb24212b167e154bb908a4c";
const CLIENT_SECRET: &str = "41918c7f458d49ca841537960cc0682e";
const REDIRECT_URI: &str = "https://playlistjockeycallback.flami.dev";
const SCOPES: [&str; 12] = [
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "playlist-read-private",
    "playlist-read-collaborative",
    "playlist-modify-private",
    "playlist-modify-public",
    "user-read-recently-played",
    "user-library-modify",
    "user-library-read",
    "user-read-private",
    "user-read-email",
];
const AUTH_TOKEN_FILE: &str = "token.txt";

async fn auth() -> Result<AuthCodeClient<Token>, String> {
    let theme: ColorfulTheme = ColorfulTheme::default();

    let auto_refresh = true;

    if let Ok(mut f) = File::open(AUTH_TOKEN_FILE) {
        let mut token = String::new();
        f.read_to_string(&mut token)
            .map_err(|_| "token.txt exists but couldn't read it")?;
        let token = serde_json::from_str(&token).map_err(|_| "token.txt exists but couldn't parse it")?;
        if let Ok(client) =
            AuthCodeClient::from_access_token(CLIENT_ID, CLIENT_SECRET, auto_refresh, token).await
        {
            println!("Authenticated using existing token!");
            return Ok(client);
        }
        println!("Existing token is invalid, need to re-authenticate");
    }

    let redirect_url = RedirectUrl::new(REDIRECT_URI.to_string())
        .map_err(|_| "REDIRECT_URI is not a valid URL")?;
    let (client, url) =
        AuthCodeClient::new(CLIENT_ID, CLIENT_SECRET, SCOPES, redirect_url, auto_refresh);
    println!("Open this URL in your browser: {}", url);
    let url: String = Input::with_theme(&theme)
        .with_prompt("Enter the URL you were redirected to")
        .interact_text()
        .map_err(|_| "somehow could not read the url?")?;
    let url = url
        .split_once("?code=")
        .ok_or("URL doesn't include '?code='")?
        .1;
    let (code, state) = url
        .split_once("&state=")
        .ok_or("URL doesn't include '&state='")?;
    let spotify = client
        .authenticate(code, state)
        .await
        .map_err(|_| "final login call went wrong")?;
    let mut file =
        File::create(AUTH_TOKEN_FILE).map_err(|_| "couldn't create token.txt to save the token")?;
    let token = spotify
        .token()
        .read()
        .map(|t| t.clone())
        .map_err(|_| "couldn't read token from client")?;
    let json_token =
        serde_json::to_string(&token).map_err(|_| "couldn't serialize token to json")?;
    file.write_all(json_token.as_bytes())
        .map_err(|_| "couldn't write to token.txt")?;
    println!("Authentication successful!");
    Ok(spotify)
}
