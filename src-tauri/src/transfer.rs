use crate::settings::SettingsStore;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

pub const DISCOVERY_PORT: u16 = 47653;
pub const TRANSFER_PORT: u16 = 47654;
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);
const PEER_TIMEOUT: Duration = Duration::from_secs(20);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(30);
const DISCOVERY_PREFIX: &str = "DROP_AIR_DISCOVERY_V1|";
const TRANSFER_PREFIX: &str = "DROP_AIR_TRANSFER_V1|";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub last_seen: u64,
}

#[derive(Default)]
pub struct PeersState {
    peers: Vec<PeerInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransferHeader {
    kind: String,
    name: String,
    size: u64,
}

pub fn setup(app: &AppHandle) -> Result<(), String> {
    app.manage(Mutex::new(PeersState::default()));
    let app = app.clone();
    thread::spawn(move || listen_for_discovery(app));

    let app = app.clone();
    thread::spawn(move || broadcast_discovery(app));

    let app = app.clone();
    thread::spawn(move || listen_for_transfers(app));
    Ok(())
}

fn device_identity(app: &AppHandle) -> (String, String) {
    let state = app.state::<Mutex<SettingsStore>>();
    match state.lock() {
        Ok(store) => {
            let settings = store.settings();
            (settings.device_id, settings.device_name)
        }
        Err(_) => ("unknown".to_string(), "DropAir".to_string()),
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn listen_for_discovery(app: AppHandle) {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) else {
        return;
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_reuse_address(true);
    let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
    let mut buffer = [0u8; 2048];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((size, source)) => {
                let Ok(message) = std::str::from_utf8(&buffer[..size]) else {
                    continue;
                };
                handle_discovery_message(&app, message, source.ip().to_string());
            }
            Err(_) => {}
        }
        prune_stale_peers(&app);
    }
}

fn handle_discovery_message(app: &AppHandle, message: &str, address: String) {
    let Some(payload) = message.strip_prefix(DISCOVERY_PREFIX) else {
        return;
    };
    let mut parts = payload.splitn(4, '|');
    let (Some(id), Some(name), Some(port_text)) = (parts.next(), parts.next(), parts.next()) else {
        return;
    };
    let Ok(port) = port_text.trim().parse::<u16>() else {
        return;
    };
    let (own_id, _) = device_identity(app);
    if id == own_id {
        return;
    }

    let peer = PeerInfo {
        id: id.to_string(),
        name: name.to_string(),
        address,
        port,
        last_seen: now_millis(),
    };
    let state = app.state::<Mutex<PeersState>>();
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let changed = if let Some(existing) = state.peers.iter_mut().find(|peer| peer.id == id) {
        let structural_change = existing.address != peer.address
            || existing.port != peer.port
            || existing.name != peer.name;
        *existing = peer;
        structural_change
    } else {
        state.peers.push(peer);
        true
    };
    if changed {
        let peers = state.peers.clone();
        drop(state);
        let _ = app.emit("peers-changed", peers);
    }
}

fn prune_stale_peers(app: &AppHandle) {
    let state = app.state::<Mutex<PeersState>>();
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let cutoff = now_millis().saturating_sub(PEER_TIMEOUT.as_millis() as u64);
    let before = state.peers.len();
    state.peers.retain(|peer| peer.last_seen >= cutoff);
    if state.peers.len() != before {
        let peers = state.peers.clone();
        drop(state);
        let _ = app.emit("peers-changed", peers);
    }
}

fn broadcast_discovery(app: AppHandle) {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
        return;
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_reuse_address(true);
    loop {
        let (id, name) = device_identity(&app);
        let message = format!("{DISCOVERY_PREFIX}{id}|{name}|{TRANSFER_PORT}");
        let _ = socket.send_to(message.as_bytes(), ("255.255.255.255", DISCOVERY_PORT));
        let _ = socket.send_to(message.as_bytes(), ("127.0.0.1", DISCOVERY_PORT));
        thread::sleep(DISCOVERY_INTERVAL);
    }
}

fn listen_for_transfers(app: AppHandle) {
    let Ok(listener) = TcpListener::bind(("0.0.0.0", TRANSFER_PORT)) else {
        return;
    };
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            continue;
        };
        let app = app.clone();
        thread::spawn(move || {
            let _ = handle_incoming_transfer(&app, stream);
        });
    }
}

fn handle_incoming_transfer(app: &AppHandle, stream: TcpStream) -> Result<(), String> {
    stream.set_read_timeout(Some(SOCKET_TIMEOUT)).map_err(|error| error.to_string())?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT)).map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut handshake = String::new();
    reader.read_line(&mut handshake).map_err(|error| error.to_string())?;
    let peer_name = handshake
        .trim()
        .strip_prefix(TRANSFER_PREFIX)
        .and_then(|payload| payload.splitn(3, '|').nth(1))
        .unwrap_or("Unknown device")
        .to_string();

    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line).map_err(|error| error.to_string())? == 0 {
            break;
        }
        let header: TransferHeader =
            serde_json::from_str(header_line.trim()).map_err(|error| error.to_string())?;
        let file_name = sanitize_file_name(&header.name);
        let received_dir = received_directory(app)?;
        std::fs::create_dir_all(&received_dir).map_err(|error| error.to_string())?;
        let target_path = received_dir.join(format!("{}_{}", now_millis(), file_name));
        let mut file = std::fs::File::create(&target_path).map_err(|error| error.to_string())?;
        let mut remaining = header.size;
        let mut buffer = vec![0u8; 64 * 1024];
        while remaining > 0 {
            let chunk_size = remaining.min(buffer.len() as u64) as usize;
            let read = reader
                .read(&mut buffer[..chunk_size])
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("connection closed before payload finished".to_string());
            }
            file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
            remaining -= read as u64;
        }
        let target = target_path.to_string_lossy().to_string();
        let _ = crate::add_shelf_paths(vec![target], app.clone());
        let _ = app.emit(
            "transfer-status",
            serde_json::json!({
                "peerId": peer_name,
                "state": "received",
                "message": format!("Received {} from {}", header.name, peer_name)
            }),
        );
    }
    Ok(())
}

fn received_directory(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("received");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn sanitize_file_name(name: &str) -> String {
    let name = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("received");
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_control()
                || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "received".to_string()
    } else {
        cleaned
    }
}

#[tauri::command]
pub fn list_peers(state: tauri::State<'_, Mutex<PeersState>>) -> Vec<PeerInfo> {
    match state.lock() {
        Ok(state) => state.peers.clone(),
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
pub fn send_shelf_items(
    peer_id: String,
    item_ids: Vec<u64>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let address = {
        let state = app.state::<Mutex<PeersState>>();
        let state = state.lock().map_err(|_| "failed to lock peers".to_string())?;
        state
            .peers
            .iter()
            .find(|peer| peer.id == peer_id)
            .map(|peer| format!("{}:{}", peer.address, peer.port))
            .ok_or_else(|| "device is no longer on the network".to_string())?
    };

    let app = app.clone();
    thread::spawn(move || {
        let result = send_items_to_peer(&app, &address, &item_ids);
        let message = match &result {
            Ok(sent) => format!("Sent {sent} item(s)"),
            Err(error) => format!("Transfer failed: {error}"),
        };
        let _ = app.emit(
            "transfer-status",
            serde_json::json!({
                "peerId": peer_id,
                "state": if result.is_ok() { "sent" } else { "error" },
                "message": message
            }),
        );
    });
    Ok(())
}

fn send_items_to_peer(app: &AppHandle, address: &str, item_ids: &[u64]) -> Result<usize, String> {
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let (id, name) = device_identity(app);
    writeln!(stream, "{TRANSFER_PREFIX}{id}|{name}")
        .map_err(|error| error.to_string())?;

    let mut sent = 0usize;
    for item_id in item_ids {
        let item = {
            let state = app.state::<Mutex<crate::AppState>>();
            let state = state.lock().map_err(|_| "failed to lock shelf".to_string())?;
            state
                .shelf_items
                .iter()
                .find(|item| item.id == *item_id)
                .cloned()
                .ok_or_else(|| format!("shelf item {item_id} no longer exists"))?
        };
        match item.kind {
            crate::ShelfItemKind::Text => {
                let content = item.content.unwrap_or_default();
                let header = TransferHeader {
                    kind: "text".to_string(),
                    name: format!("{}.txt", item.name),
                    size: content.len() as u64,
                };
                let header_json = serde_json::to_string(&header).map_err(|error| error.to_string())?;
                writeln!(stream, "{header_json}").map_err(|error| error.to_string())?;
                stream
                    .write_all(content.as_bytes())
                    .map_err(|error| error.to_string())?;
                sent += 1;
            }
            crate::ShelfItemKind::File => {
                let path = Path::new(&item.path);
                let size = std::fs::metadata(path)
                    .map_err(|error| error.to_string())?
                    .len();
                let header = TransferHeader {
                    kind: "file".to_string(),
                    name: item.name,
                    size,
                };
                let header_json = serde_json::to_string(&header).map_err(|error| error.to_string())?;
                writeln!(stream, "{header_json}").map_err(|error| error.to_string())?;
                let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
                let mut buffer = vec![0u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
                    if read == 0 {
                        break;
                    }
                    stream
                        .write_all(&buffer[..read])
                        .map_err(|error| error.to_string())?;
                }
                sent += 1;
            }
            crate::ShelfItemKind::Directory | crate::ShelfItemKind::Other => {
                continue;
            }
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_unsafe_file_names() {
        assert_eq!(sanitize_file_name("../secret:name?.txt"), "secret_name_.txt");
        assert_eq!(sanitize_file_name("plain.txt"), "plain.txt");
        assert_eq!(sanitize_file_name(""), "received");
    }
}
