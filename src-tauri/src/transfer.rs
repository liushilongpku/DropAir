use crate::settings::SettingsStore;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
const TRANSFER_ACK_PREFIX: &str = "DROP_AIR_ACK_V1|";
const MAX_TRANSFER_ITEM_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TRANSFER_TEXT_SIZE: u64 = 16 * 1024 * 1024;
const PEERS_FILE: &str = "peers.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub addresses: Vec<String>,
    pub last_seen: u64,
    pub manual: bool,
    pub linked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedPeer {
    id: String,
    name: String,
    address: String,
    port: u16,
    #[serde(default)]
    addresses: Vec<String>,
    linked: bool,
}

#[derive(Default)]
pub struct PeersState {
    peers: Vec<PeerInfo>,
    last_error: Option<String>,
}

static UDP_LISTENER_UP: AtomicBool = AtomicBool::new(false);
static DISCOVERY_BROADCAST_UP: AtomicBool = AtomicBool::new(false);
static TCP_LISTENER_UP: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransferHeader {
    kind: String,
    name: String,
    size: u64,
}

pub fn setup(app: &AppHandle) -> Result<(), String> {
    app.manage(Mutex::new(PeersState {
        peers: load_saved_peers(app),
        last_error: None,
    }));
    let discovery_app = app.clone();
    thread::spawn(move || listen_for_discovery(discovery_app));

    let broadcast_app = app.clone();
    thread::spawn(move || broadcast_discovery(broadcast_app));

    let transfer_app = app.clone();
    thread::spawn(move || listen_for_transfers(transfer_app));
    Ok(())
}

fn peers_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?
        .join(PEERS_FILE))
}

fn load_saved_peers(app: &AppHandle) -> Vec<PeerInfo> {
    let Ok(path) = peers_path(app) else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<SavedPeer>>(&contents)
        .unwrap_or_default()
        .into_iter()
        .map(|peer| PeerInfo {
            id: peer.id,
            name: peer.name,
            address: peer.address.clone(),
            port: peer.port,
            addresses: if peer.addresses.is_empty() {
                vec![peer.address]
            } else {
                peer.addresses
            },
            last_seen: now_millis(),
            manual: true,
            linked: peer.linked,
        })
        .collect()
}

fn save_peers(app: &AppHandle) -> Result<(), String> {
    let peers = app
        .state::<Mutex<PeersState>>()
        .lock()
        .map_err(|_| "failed to lock peers".to_string())?
        .peers
        .iter()
        .filter(|peer| peer.manual || peer.linked)
        .map(|peer| SavedPeer {
            id: peer.id.clone(),
            name: peer.name.clone(),
            address: peer.address.clone(),
            port: peer.port,
            addresses: peer.addresses.clone(),
            linked: peer.linked,
        })
        .collect::<Vec<_>>();
    let path = peers_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "peers path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary_path = path.with_extension("json.tmp");
    let contents = serde_json::to_vec_pretty(&peers).map_err(|error| error.to_string())?;
    std::fs::write(&temporary_path, contents).map_err(|error| error.to_string())?;
    std::fs::rename(temporary_path, path).map_err(|error| error.to_string())
}

fn device_identity(app: &AppHandle) -> (String, String) {
    let state = app.state::<Mutex<SettingsStore>>();
    let identity = match state.lock() {
        Ok(store) => {
            let settings = store.settings();
            (settings.device_id, settings.device_name)
        }
        Err(_) => ("unknown".to_string(), "DropAir".to_string()),
    };
    identity
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn listen_for_discovery(app: AppHandle) {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) else {
        emit_transfer_error(
            &app,
            &format!(
                "Discovery listener failed: UDP port {DISCOVERY_PORT} is unavailable"
            ),
        );
        return;
    };
    UDP_LISTENER_UP.store(true, Ordering::Relaxed);
    let _ = socket.set_broadcast(true);
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
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => {
                emit_transfer_error(&app, &format!("Discovery listener error: {error}"));
                UDP_LISTENER_UP.store(false, Ordering::Relaxed);
                break;
            }
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

    let state = app.state::<Mutex<PeersState>>();
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let display_name = if name.trim().is_empty() {
        format!("DropAir device ({address})")
    } else {
        name.to_string()
    };
    let changed = if let Some(existing) = state.peers.iter_mut().find(|existing| {
        existing.id == id
            || existing.address == address
            || existing.addresses.iter().any(|candidate| candidate == &address)
    }) {
        let mut structural_change = false;
        if existing.addresses.is_empty() {
            existing.addresses.push(existing.address.clone());
        }
        if !existing.addresses.iter().any(|candidate| candidate == &address) {
            existing.addresses.push(address.clone());
            structural_change = true;
        }
        if existing.id != id {
            existing.id = id.to_string();
            structural_change = true;
        }
        if existing.port != port {
            existing.port = port;
            structural_change = true;
        }
        if existing.name != display_name {
            existing.name = display_name;
            structural_change = true;
        }
        existing.last_seen = now_millis();
        structural_change
    } else {
        state.peers.push(PeerInfo {
            id: id.to_string(),
            name: display_name,
            address: address.clone(),
            port,
            addresses: vec![address],
            last_seen: now_millis(),
            manual: false,
            linked: false,
        });
        true
    };
    if changed {
        let peers = state.peers.clone();
        drop(state);
        let _ = save_peers(app);
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
    state
        .peers
        .retain(|peer| peer.manual || peer.last_seen >= cutoff);
    if state.peers.len() != before {
        let peers = state.peers.clone();
        drop(state);
        let _ = app.emit("peers-changed", peers);
    }
}

fn broadcast_discovery(app: AppHandle) {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
        record_transfer_error(&app, "Discovery broadcast could not open a UDP socket");
        return;
    };
    let _ = socket.set_broadcast(true);
    loop {
        if let Err(error) = send_discovery_announcement(&app, &socket) {
            record_transfer_error(&app, &format!("Discovery broadcast failed: {error}"));
        }
        thread::sleep(DISCOVERY_INTERVAL);
    }
}

fn send_discovery_announcement(app: &AppHandle, socket: &UdpSocket) -> Result<(), String> {
    DISCOVERY_BROADCAST_UP.store(false, Ordering::Relaxed);
    let (id, name) = device_identity(app);
    let message = format!("{DISCOVERY_PREFIX}{id}|{name}|{TRANSFER_PORT}");
    socket
        .send_to(message.as_bytes(), ("255.255.255.255", DISCOVERY_PORT))
        .map_err(|error| error.to_string())?;
    socket
        .send_to(message.as_bytes(), ("127.0.0.1", DISCOVERY_PORT))
        .map_err(|error| error.to_string())?;
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            let if_addrs::IfAddr::V4(address) = interface.addr else {
                continue;
            };
            let broadcast = directed_broadcast(address.ip, address.netmask);
            if broadcast != Ipv4Addr::new(255, 255, 255, 255) {
                let _ = socket.send_to(message.as_bytes(), (broadcast, DISCOVERY_PORT));
            }
        }
    }
    DISCOVERY_BROADCAST_UP.store(true, Ordering::Relaxed);
    Ok(())
}

fn directed_broadcast(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(netmask))
}

fn listen_for_transfers(app: AppHandle) {
    let Ok(listener) = TcpListener::bind(("0.0.0.0", TRANSFER_PORT)) else {
        emit_transfer_error(
            &app,
            &format!("Transfer listener failed: TCP port {TRANSFER_PORT} is unavailable"),
        );
        return;
    };
    TCP_LISTENER_UP.store(true, Ordering::Relaxed);
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let app = app.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_incoming_transfer(&app, stream) {
                        emit_transfer_error(&app, &format!("Incoming transfer failed: {error}"));
                    }
                });
            }
            Err(error) => {
                emit_transfer_error(&app, &format!("Transfer listener error: {error}"));
                TCP_LISTENER_UP.store(false, Ordering::Relaxed);
                break;
            }
        }
    }
}

fn handle_incoming_transfer(app: &AppHandle, stream: TcpStream) -> Result<(), String> {
    stream.set_read_timeout(Some(SOCKET_TIMEOUT)).map_err(|error| error.to_string())?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT)).map_err(|error| error.to_string())?;
    let mut writer = stream.try_clone().map_err(|error| error.to_string())?;
    writer
        .set_write_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut handshake = String::new();
    reader.read_line(&mut handshake).map_err(|error| error.to_string())?;
    let peer_name = handshake
        .trim()
        .strip_prefix(TRANSFER_PREFIX)
        .and_then(|payload| payload.splitn(3, '|').nth(1))
        .unwrap_or("Unknown device")
        .to_string();
    let mut received_items = 0usize;

    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line).map_err(|error| error.to_string())? == 0 {
            break;
        }
        let header: TransferHeader =
            serde_json::from_str(header_line.trim()).map_err(|error| error.to_string())?;
        let kind = header.kind.as_str();
        let max_size = if kind == "text" {
            MAX_TRANSFER_TEXT_SIZE
        } else if kind == "file" {
            MAX_TRANSFER_ITEM_SIZE
        } else {
            return Err(format!("unsupported transfer item kind: {}", header.kind));
        };
        if header.size > max_size {
            return Err(format!("transfer item is too large: {} bytes", header.size));
        }

        if kind == "text" {
            let mut content = vec![0u8; header.size as usize];
            reader
                .read_exact(&mut content)
                .map_err(|error| format!("could not read text payload: {error}"))?;
            let content = String::from_utf8(content)
                .map_err(|_| "received text is not valid UTF-8".to_string())?;
            crate::add_shelf_text_to_app(app, content)?;
        } else {
            let file_name = sanitize_file_name(&header.name);
            let received_dir = received_directory(app)?;
            let target_path = received_dir.join(format!("{}_{}", now_millis(), file_name));
            let temporary_path = target_path.with_extension("dropair-partial");
            let mut file = std::fs::File::create(&temporary_path).map_err(|error| error.to_string())?;
            let mut remaining = header.size;
            let mut buffer = vec![0u8; 64 * 1024];
            while remaining > 0 {
                let chunk_size = remaining.min(buffer.len() as u64) as usize;
                let read = reader
                    .read(&mut buffer[..chunk_size])
                    .map_err(|error| error.to_string())?;
                if read == 0 {
                    let _ = std::fs::remove_file(&temporary_path);
                    return Err("connection closed before payload finished".to_string());
                }
                file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
                remaining -= read as u64;
            }
            file.sync_all().map_err(|error| error.to_string())?;
            std::fs::rename(&temporary_path, &target_path).map_err(|error| error.to_string())?;
            let target = target_path.to_string_lossy().to_string();
            crate::add_shelf_paths(vec![target], app.clone())?;
        }
        received_items += 1;
        let _ = app.emit(
            "transfer-status",
            serde_json::json!({
                "peerId": peer_name,
                "state": "received",
                "message": format!("Received {} from {}", header.name, peer_name)
            }),
        );
    }
    writeln!(writer, "{TRANSFER_ACK_PREFIX}complete|{received_items}")
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    Ok(())
}

fn configured_download_directory(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let settings = app.state::<Mutex<SettingsStore>>();
    let directory = settings
        .lock()
        .map_err(|_| "failed to lock settings".to_string())?
        .settings()
        .download_directory;
    Ok(directory
        .filter(|directory| !directory.trim().is_empty())
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute()))
}

fn download_directory_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(directory) = configured_download_directory(app)? {
        return Ok(directory);
    }
    if let Ok(directory) = app.path().download_dir() {
        return Ok(directory.join("DropAir"));
    }
    app.path()
        .app_data_dir()
        .map(|directory| directory.join("received"))
        .map_err(|error| error.to_string())
}

fn received_directory(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = download_directory_path(app)?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

#[tauri::command]
pub fn download_directory(app: tauri::AppHandle) -> Result<String, String> {
    Ok(download_directory_path(&app)?.to_string_lossy().to_string())
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

fn emit_transfer_error(app: &AppHandle, message: &str) {
    record_transfer_error(app, message);
    let _ = app.emit(
        "transfer-status",
        serde_json::json!({
            "peerId": "local",
            "state": "error",
            "message": message
        }),
    );
}

fn record_transfer_error(app: &AppHandle, message: &str) {
    eprintln!("DropAir transfer: {message}");
    let state = app.state::<Mutex<PeersState>>();
    if let Ok(mut state) = state.lock() {
        state.last_error = Some(message.to_string());
    };
}

#[tauri::command]
pub fn list_peers(state: tauri::State<'_, Mutex<PeersState>>) -> Vec<PeerInfo> {
    let peers = match state.lock() {
        Ok(state) => state.peers.clone(),
        Err(_) => Vec::new(),
    };
    peers
}

#[tauri::command]
pub fn send_shelf_items(
    peer_id: String,
    item_ids: Vec<u64>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let addresses = {
        let state = app.state::<Mutex<PeersState>>();
        let state = state.lock().map_err(|_| "failed to lock peers".to_string())?;
        state
            .peers
            .iter()
            .find(|peer| peer.id == peer_id && peer.linked)
            .map(|peer| peer_endpoints(peer))
            .ok_or_else(|| "device is no longer on the network".to_string())?
    };

    let app = app.clone();
    thread::spawn(move || {
        let mut last_error = None;
        let result = addresses
            .iter()
            .find_map(|address| match send_items_to_peer(&app, address, &item_ids) {
                Ok(sent) => Some(Ok(sent)),
                Err(error) => {
                    last_error = Some(error);
                    None
                }
            })
            .unwrap_or_else(|| Err(last_error.unwrap_or_else(|| "no device address is available".to_string())));
        if let Err(error) = &result {
            record_transfer_error(&app, &format!("Transfer failed: {error}"));
        }
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
    let mut stream = TcpStream::connect(address).map_err(|error| {
        format!(
            "Could not connect to {address}: {error}. On Windows, allow DropAir through the firewall for private networks."
        )
    })?;
    stream
        .set_read_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
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
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| format!("could not finish transfer: {error}"))?;
    wait_for_transfer_completion(&mut reader, sent)?;
    Ok(sent)
}

fn wait_for_transfer_completion(reader: &mut BufReader<TcpStream>, sent: usize) -> Result<(), String> {
    let mut response = String::new();
    let read = reader
        .read_line(&mut response)
        .map_err(|error| format!("waiting for transfer confirmation failed: {error}"))?;
    if read == 0 {
        // Older DropAir versions do not send a completion acknowledgement.
        return Ok(());
    }
    let Some(payload) = response.trim().strip_prefix(TRANSFER_ACK_PREFIX) else {
        return Err("device returned an invalid transfer confirmation".to_string());
    };
    let mut parts = payload.splitn(2, '|');
    if parts.next() != Some("complete") {
        return Err("device rejected the transfer".to_string());
    }
    let count = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "device returned an invalid item count".to_string())?;
    if count != sent {
        return Err(format!("device confirmed {count} of {sent} item(s)"));
    }
    Ok(())
}

fn peer_endpoint(address: &str, port: u16) -> String {
    if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    }
}

fn peer_endpoints(peer: &PeerInfo) -> Vec<String> {
    let mut addresses = Vec::with_capacity(peer.addresses.len() + 1);
    addresses.push(peer.address.clone());
    for address in &peer.addresses {
        if !addresses.iter().any(|candidate| candidate == address) {
            addresses.push(address.clone());
        }
    }
    addresses
        .into_iter()
        .map(|address| peer_endpoint(&address, peer.port))
        .collect()
}

fn normalize_peer_address(address: &str) -> Result<String, String> {
    let address = address.trim();
    if address.is_empty() {
        return Err("device address is empty".to_string());
    }
    let address = address
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(address);
    if address.contains('/') || address.contains('\\') || address.chars().any(char::is_control) {
        return Err("device address contains invalid characters".to_string());
    }
    Ok(address.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatus {
    pub udp_listener_up: bool,
    pub discovery_broadcast_up: bool,
    pub tcp_listener_up: bool,
    pub discovery_port: u16,
    pub transfer_port: u16,
}

#[tauri::command]
pub fn transfer_status() -> TransferStatus {
    TransferStatus {
        udp_listener_up: UDP_LISTENER_UP.load(Ordering::Relaxed),
        discovery_broadcast_up: DISCOVERY_BROADCAST_UP.load(Ordering::Relaxed),
        tcp_listener_up: TCP_LISTENER_UP.load(Ordering::Relaxed),
        discovery_port: DISCOVERY_PORT,
        transfer_port: TRANSFER_PORT,
    }
}

#[tauri::command]
pub fn scan_lan_devices(app: tauri::AppHandle) -> Result<(), String> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|error| error.to_string())?;
    socket
        .set_broadcast(true)
        .map_err(|error| error.to_string())?;
    send_discovery_announcement(&app, &socket).map_err(|error| {
        record_transfer_error(&app, &format!("Manual discovery scan failed: {error}"));
        error
    })
}

#[tauri::command]
pub fn add_manual_peer(
    address: String,
    port: u16,
    name: Option<String>,
    app: tauri::AppHandle,
) -> Result<PeerInfo, String> {
    if port == 0 {
        return Err("device port must be between 1 and 65535".to_string());
    }
    let address = normalize_peer_address(&address)?;
    let endpoint = peer_endpoint(&address, port);
    endpoint
        .to_socket_addrs()
        .map_err(|error| format!("could not resolve {endpoint}: {error}"))?
        .next()
        .ok_or_else(|| format!("could not resolve {endpoint}"))?;
    let name = name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&address)
        .to_string();
    let peer = PeerInfo {
        id: format!("manual:{endpoint}"),
        name,
        address: address.clone(),
        port,
        addresses: vec![address],
        last_seen: now_millis(),
        manual: true,
        linked: false,
    };
    let state = app.state::<Mutex<PeersState>>();
    let mut state = state.lock().map_err(|_| "failed to lock peers".to_string())?;
    if let Some(existing) = state
        .peers
        .iter_mut()
        .find(|existing| existing.address == peer.address && existing.port == peer.port)
    {
        let linked = existing.linked;
        *existing = peer.clone();
        existing.linked = linked;
    } else {
        state.peers.push(peer.clone());
    }
    let peers = state.peers.clone();
    drop(state);
    let _ = save_peers(&app);
    let _ = app.emit("peers-changed", peers);
    Ok(peer)
}

#[tauri::command]
pub fn remove_peer(peer_id: String, app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Mutex<PeersState>>();
    let mut state = state.lock().map_err(|_| "failed to lock peers".to_string())?;
    let before = state.peers.len();
    state.peers.retain(|peer| peer.id != peer_id);
    if state.peers.len() == before {
        return Err("device is no longer in the list".to_string());
    }
    let peers = state.peers.clone();
    drop(state);
    let _ = save_peers(&app);
    let _ = app.emit("peers-changed", peers);
    Ok(())
}

#[tauri::command]
pub fn set_peer_linked(
    peer_id: String,
    linked: bool,
    app: tauri::AppHandle,
) -> Result<Vec<PeerInfo>, String> {
    let state = app.state::<Mutex<PeersState>>();
    let mut state = state.lock().map_err(|_| "failed to lock peers".to_string())?;
    let peer = state
        .peers
        .iter_mut()
        .find(|peer| peer.id == peer_id)
        .ok_or_else(|| "device is no longer in the list".to_string())?;
    peer.linked = linked;
    let peers = state.peers.clone();
    drop(state);
    let _ = save_peers(&app);
    let _ = app.emit("peers-changed", &peers);
    Ok(peers)
}

#[tauri::command]
pub fn test_peer_connection(peer_id: String, app: tauri::AppHandle) -> Result<(), String> {
    let endpoints = {
        let state = app.state::<Mutex<PeersState>>();
        let state = state.lock().map_err(|_| "failed to lock peers".to_string())?;
        state
            .peers
            .iter()
            .find(|peer| peer.id == peer_id && peer.linked)
            .map(peer_endpoints)
            .ok_or_else(|| "link the device before testing the connection".to_string())?
    };
    let mut last_error = None;
    for endpoint in endpoints {
        let address = match endpoint.to_socket_addrs() {
            Ok(mut addresses) => match addresses.next() {
                Some(address) => address,
                None => {
                    last_error = Some(format!("could not resolve {endpoint}"));
                    continue;
                }
            },
            Err(error) => {
                last_error = Some(format!("could not resolve {endpoint}: {error}"));
                continue;
            }
        };
        let mut stream = match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(stream) => stream,
            Err(error) => {
                last_error = Some(format!("could not connect to {endpoint}: {error}"));
                continue;
            }
        };
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let (id, name) = device_identity(&app);
        if let Err(error) = writeln!(stream, "{TRANSFER_PREFIX}{id}|{name}") {
            last_error = Some(format!("connected to {endpoint}, but handshake failed: {error}"));
            continue;
        }
        return Ok(());
    }
    Err(last_error.unwrap_or_else(|| "no device address is available".to_string()))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferDiagnostics {
    pub udp_listener_up: bool,
    pub discovery_broadcast_up: bool,
    pub tcp_listener_up: bool,
    pub discovery_port: u16,
    pub transfer_port: u16,
    pub peer_count: usize,
    pub received_directory_writable: bool,
    pub loopback_tcp_ok: bool,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSelfCheck {
    pub checked_at: u64,
    pub diagnostics: TransferDiagnostics,
    pub checks: Vec<TransferCheck>,
}

fn received_directory_writable(app: &AppHandle) -> bool {
    let Ok(directory) = received_directory(app) else {
        return false;
    };
    let probe = directory.join(format!(".dropair-self-check-{}", now_millis()));
    match std::fs::File::create(&probe) {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(probe).is_ok()
        }
        Err(_) => false,
    }
}

fn transfer_diagnostics(app: &AppHandle) -> TransferDiagnostics {
    let (peer_count, last_error) = app
        .state::<Mutex<PeersState>>()
        .lock()
        .map(|state| (state.peers.len(), state.last_error.clone()))
        .unwrap_or((0, Some("Could not read transfer state".to_string())));
    let tcp_listener_up = TCP_LISTENER_UP.load(Ordering::Relaxed);
    let loopback_tcp_ok = tcp_listener_up
        && format!("127.0.0.1:{TRANSFER_PORT}")
            .parse::<SocketAddr>()
            .ok()
            .and_then(|address| TcpStream::connect_timeout(&address, Duration::from_millis(500)).ok())
            .is_some();
    TransferDiagnostics {
        udp_listener_up: UDP_LISTENER_UP.load(Ordering::Relaxed),
        discovery_broadcast_up: DISCOVERY_BROADCAST_UP.load(Ordering::Relaxed),
        tcp_listener_up,
        discovery_port: DISCOVERY_PORT,
        transfer_port: TRANSFER_PORT,
        peer_count,
        received_directory_writable: received_directory_writable(app),
        loopback_tcp_ok,
        last_error,
    }
}

#[tauri::command]
pub fn transfer_self_check(app: tauri::AppHandle) -> TransferSelfCheck {
    let diagnostics = transfer_diagnostics(&app);
    let checks = vec![
        TransferCheck {
            id: "discovery-listener".to_string(),
            label: "Discovery listener".to_string(),
            ok: diagnostics.udp_listener_up,
            detail: if diagnostics.udp_listener_up {
                format!("UDP {} is listening", diagnostics.discovery_port)
            } else {
                format!("UDP {} is unavailable", diagnostics.discovery_port)
            },
        },
        TransferCheck {
            id: "discovery-broadcast".to_string(),
            label: "Discovery broadcast".to_string(),
            ok: diagnostics.discovery_broadcast_up,
            detail: if diagnostics.discovery_broadcast_up {
                format!("UDP discovery announcements are being sent on port {}", diagnostics.discovery_port)
            } else {
                "UDP discovery announcements could not be sent".to_string()
            },
        },
        TransferCheck {
            id: "transfer-listener".to_string(),
            label: "Transfer listener".to_string(),
            ok: diagnostics.tcp_listener_up,
            detail: if diagnostics.tcp_listener_up {
                format!("TCP {} is listening", diagnostics.transfer_port)
            } else {
                format!("TCP {} is unavailable", diagnostics.transfer_port)
            },
        },
        TransferCheck {
            id: "loopback-connection".to_string(),
            label: "Local loopback connection".to_string(),
            ok: diagnostics.loopback_tcp_ok,
            detail: if diagnostics.loopback_tcp_ok {
                "The local transfer service accepts connections".to_string()
            } else {
                "The local transfer service is not reachable".to_string()
            },
        },
        TransferCheck {
            id: "received-directory".to_string(),
            label: "Received-directory write access".to_string(),
            ok: diagnostics.received_directory_writable,
            detail: if diagnostics.received_directory_writable {
                "The received directory is writable".to_string()
            } else {
                "The received directory is missing or not writable".to_string()
            },
        },
        TransferCheck {
            id: "device-discovery".to_string(),
            label: "Peer discovery".to_string(),
            ok: diagnostics.peer_count > 0,
            detail: if diagnostics.peer_count > 0 {
                format!("{} DropAir device(s) discovered", diagnostics.peer_count)
            } else {
                "No other DropAir device found; check the LAN and firewall".to_string()
            },
        },
    ];
    TransferSelfCheck {
        checked_at: now_millis(),
        diagnostics,
        checks,
    }
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

    #[test]
    fn formats_ipv6_peer_endpoints() {
        assert_eq!(peer_endpoint("192.168.1.20", 47654), "192.168.1.20:47654");
        assert_eq!(peer_endpoint("fe80::1", 47654), "[fe80::1]:47654");
    }

    #[test]
    fn normalizes_manual_peer_addresses() {
        assert_eq!(normalize_peer_address("[fe80::1]"), Ok("fe80::1".to_string()));
        assert!(normalize_peer_address("192.168.1.20/share").is_err());
        assert!(normalize_peer_address(" ").is_err());
    }
}
