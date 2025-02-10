use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, FuzzySelect, Input};
use spotify_rs::auth::{NoVerifier, Token};
use spotify_rs::client::Client;
use spotify_rs::model::PlayableItem;
use spotify_rs::{AuthCodeClient, AuthCodeFlow, RedirectUrl};
use std::fs::File;
use std::io::{Read, Write};
use std::time::Duration;
use tokio::time::sleep;

type Spotify = Client<Token, AuthCodeFlow, NoVerifier>;

async fn program(client: &mut Spotify) -> bool {
    let theme: ColorfulTheme = ColorfulTheme::default();

    let Ok(track) = client.get_currently_playing_track(None).await else {
        return false;
    };
    let Some(track) = track.item else {
        return false;
    };
    let track = Track::from_playable_item(track);

    let Ok(user) = client.get_current_user_profile().await else {
        println!("Failed to get user profile!");
        return false;
    };

    let Ok(playlists) = client.current_user_playlists().limit(50).get().await else {
        println!("Failed to get user playlists!");
        return false;
    };
    let playlists: Vec<_> = playlists
        .items
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
            if let Err(_) = client.skip_to_next(None).await {
                println!("Failed to skip to next track!");
            }
        }
        1 => return true,
        2 => {
            if let Err(_) = client.remove_saved_tracks(&[track.uri.as_str()]).await {
                println!("Failed to remove track from library!");
            };
            if let Err(_) = client.skip_to_next(None).await {
                println!("Failed to skip to next track!");
            };
        }
        3 => return false,
        i => {
            if i < items_len {
                panic!("Selected an action that isn't implemented yet!");
            }
            let playlist_id = playlist_ids[i - items_len];
            if let Err(_) = client
                .add_items_to_playlist(playlist_id, &[track.uri.as_str()])
                .send()
                .await
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

    let mut spotify = match auth().await {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to authenticate, {}", e);
            return;
        }
    };

    if let Ok(mut f) = File::open("last.txt") {
        let mut last = String::new();
        if let Err(_) = f.read_to_string(&mut last) {
            println!("Failed to read last.txt");
            return;
        };
        let Ok(user) = spotify.get_current_user_profile().await else {
            println!("Failed to get user profile!");
            return;
        };
        if let Err(e) = spotify
            .start_playback()
            .context_uri(format!("spotify:user:{}:collection", user.id))
            .offset_uri(last.as_str())
            .send()
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

    while program(&mut spotify).await {}

    if let Some(last) = spotify
        .get_currently_playing_track(None)
        .await
        .ok()
        .and_then(|i| i.item)
    {
        let Ok(mut f) = File::create("last.txt") else {
            println!("Failed to create last.txt");
            return;
        };
        let uri = Track::from_playable_item(last).uri;
        if let Err(_) = f.write(uri.as_bytes()) {
            println!("Failed to write to last.txt");
        };
    }
}

struct Track {
    uri: String,
    name: String,
    artists: Vec<String>,
}

impl Track {
    fn from_playable_item(item: PlayableItem) -> Self {
        match item {
            PlayableItem::Track(t) => Self {
                uri: t.uri,
                name: t.name,
                artists: t.artists.iter().map(|a| a.name.clone()).collect(),
            },
            PlayableItem::Episode(e) => Self {
                uri: e.uri,
                name: e.name,
                artists: vec![e.show.name],
            },
        }
    }
}

async fn auth() -> Result<Client<Token, AuthCodeFlow, NoVerifier>, String> {
    let theme: ColorfulTheme = ColorfulTheme::default();

    let auto_refresh = true;
    let scopes: Vec<String> = vec![
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    if let Ok(mut f) = File::open("token.txt") {
        let mut token = String::new();
        f.read_to_string(&mut token)
            .map_err(|_| "token.txt exists but couldn't read it")?;
        let auth_code_flow = AuthCodeFlow::new(
            "dd5192114eb24212b167e154bb908a4c",
            "41918c7f458d49ca841537960cc0682e",
            &scopes,
        );
        if let Ok(client) =
            Client::from_refresh_token(auth_code_flow, auto_refresh, token.clone()).await
        {
            println!("Authenticated using existing token!");
            return Ok(client);
        }
    }

    let auth_code_flow = AuthCodeFlow::new(
        "dd5192114eb24212b167e154bb908a4c",
        "41918c7f458d49ca841537960cc0682e",
        &scopes,
    );
    let redirect_url =
        RedirectUrl::new("https://playlistjockeycallback.flami.dev".to_owned()).unwrap();
    let (client, url) = AuthCodeClient::new(auth_code_flow, redirect_url, auto_refresh);
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
    if let Some(token) = spotify.refresh_token() {
        let mut f = File::create("token.txt").map_err(|_| "could not create token.txt")?;
        f.write(token.as_bytes())
            .map_err(|_| "could not write to token.txt")?;
    };
    println!("Authentication successful!");
    Ok(spotify)
}
