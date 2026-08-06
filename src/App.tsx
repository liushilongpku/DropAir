import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  CheckCircle2,
  ExternalLink,
  FileArchive,
  FileText,
  Folder,
  FolderOpen,
  Laptop,
  Loader2,
  PanelTopOpen,
  Settings2,
  Send,
  ShieldCheck,
  Trash2,
  X
} from "lucide-react";
import { DragEvent, MouseEvent, useEffect, useMemo, useState } from "react";

const isShelfWindow = new URLSearchParams(window.location.search).has("shelf");

if (isShelfWindow) {
  document.body.classList.add("shake-shelf-body");
}

type ShelfItemKind = "file" | "directory" | "text" | "other";

type ShelfItem = {
  id: number;
  path: string;
  content: string | null;
  name: string;
  kind: ShelfItemKind;
  size: number | null;
};

type DropAirFile = File & {
  path?: string;
};

type ShakeDiagnostics = {
  mouseDowns: number;
  motionSamples: number;
  maxDirectionChanges: number;
  triggers: number;
};

type AppSettings = {
  shakeEnabled: boolean;
  shakeSensitivity: number;
};

type PlatformCapabilities = {
  platform: string;
  shakeSupported: boolean;
  nativeFileDragSupported: boolean;
  accessibilityRequired: boolean;
};

type PeerInfo = {
  id: string;
  name: string;
  address: string;
  port: number;
  lastSeen: number;
};

type MainView = "shelf" | "devices" | "settings";

function App() {
  const [items, setItems] = useState<ShelfItem[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [status, setStatus] = useState("Ready");
  const [isBusy, setIsBusy] = useState(false);
  const [shakeStatus, setShakeStatus] = useState("starting");
  const [shakeDiagnostics, setShakeDiagnostics] = useState<ShakeDiagnostics | null>(null);
  const [mainView, setMainView] = useState<MainView>("shelf");
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [shakeEnabled, setShakeEnabledState] = useState(true);
  const [shakeSensitivity, setShakeSensitivityState] = useState(3);
  const [accessibilityAllowed, setAccessibilityAllowed] = useState<boolean | null>(null);
  const [settingsReady, setSettingsReady] = useState(false);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [platformCapabilities, setPlatformCapabilities] =
    useState<PlatformCapabilities | null>(null);
  const shakeSupported = platformCapabilities?.shakeSupported ?? true;
  const isWindows = platformCapabilities?.platform === "windows";
  const accessibilityRequired = platformCapabilities?.accessibilityRequired ?? false;

  const totalSize = useMemo(
    () => items.reduce((sum, item) => sum + (item.size ?? 0), 0),
    [items]
  );

  useEffect(() => {
    void refreshShelf();
  }, []);

  useEffect(() => {
    let unlistenPeers: (() => void) | undefined;
    let unlistenTransfer: (() => void) | undefined;
    void listen<PeerInfo[]>("peers-changed", (event) => setPeers(event.payload)).then(
      (nextUnlisten) => {
        unlistenPeers = nextUnlisten;
      }
    );
    void listen<{ message: string }>("transfer-status", (event) => setStatus(event.payload.message)).then(
      (nextUnlisten) => {
        unlistenTransfer = nextUnlisten;
      }
    );
    void invoke<PeerInfo[]>("list_peers")
      .then(setPeers)
      .catch(() => undefined);
    return () => {
      unlistenPeers?.();
      unlistenTransfer?.();
    };
  }, []);

  useEffect(() => {
    void invoke<PlatformCapabilities>("platform_capabilities")
      .then(setPlatformCapabilities)
      .catch((error) => setStatus(toErrorMessage(error)));
  }, []);

  useEffect(() => {
    if (isShelfWindow) return;
    const loadSettings = async () => {
      try {
        const [autostart, appSettings, capabilities, accessibility] = await Promise.all([
          invoke<boolean>("autostart_enabled"),
          invoke<AppSettings>("app_settings"),
          invoke<PlatformCapabilities>("platform_capabilities"),
          invoke<boolean>("accessibility_permission_status")
        ]);
        setLaunchAtLogin(autostart);
        setShakeEnabledState(appSettings.shakeEnabled);
        setShakeSensitivityState(appSettings.shakeSensitivity);
        setPlatformCapabilities(capabilities);
        setAccessibilityAllowed(accessibility);
      } catch (error) {
        setStatus(toErrorMessage(error));
      } finally {
        setSettingsReady(true);
      }
    };
    void loadSettings();
  }, []);

  useEffect(() => {
    if (isShelfWindow || !platformCapabilities?.accessibilityRequired) return;
    const refreshAccessibility = () => {
      void invoke<boolean>("accessibility_permission_status")
        .then(setAccessibilityAllowed)
        .catch(() => undefined);
    };
    window.addEventListener("focus", refreshAccessibility);
    return () => window.removeEventListener("focus", refreshAccessibility);
  }, [platformCapabilities?.accessibilityRequired]);

  useEffect(() => {
    if (!shakeSupported) {
      setShakeStatus("unsupported");
      setShakeDiagnostics(null);
      return;
    }

    const refreshDiagnostics = () => {
      void invoke<ShakeDiagnostics>("shake_monitor_diagnostics")
        .then(setShakeDiagnostics)
        .catch(() => undefined);
    };
    refreshDiagnostics();
    const timer = window.setInterval(refreshDiagnostics, 500);
    return () => window.clearInterval(timer);
  }, [shakeSupported]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const refreshStatus = () => {
      void invoke<string>("shake_monitor_status").then(setShakeStatus).catch(() => undefined);
    };
    const timer = window.setTimeout(refreshStatus, 500);
    void listen<string>("shake-monitor-status", (event) => setShakeStatus(event.payload)).then(
      (nextUnlisten) => {
        unlisten = nextUnlisten;
      }
    );

    return () => {
      window.clearTimeout(timer);
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<ShelfItem[]>("shelf-changed", (event) => setItems(event.payload)).then(
      (nextUnlisten) => {
        unlisten = nextUnlisten;
      }
    );

    return () => unlisten?.();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setIsDragging(true);
          return;
        }

        if (event.payload.type === "leave") {
          setIsDragging(false);
          return;
        }

        setIsDragging(false);
        void addPaths(event.payload.paths);
      })
      .then((nextUnlisten) => {
        if (cancelled) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      })
      .catch((error) => {
        setStatus(toErrorMessage(error));
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  async function refreshShelf() {
    setIsBusy(true);
    try {
      const nextItems = await invoke<ShelfItem[]>("list_shelf_items");
      setItems(nextItems);
      setStatus("Ready");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function addPaths(paths: string[]) {
    if (paths.length === 0) {
      setStatus("No readable file paths found");
      return;
    }

    setIsBusy(true);
    try {
      const nextItems = await invoke<ShelfItem[]>("add_shelf_paths", { paths });
      setItems(nextItems);
      setStatus(`${paths.length} item${paths.length === 1 ? "" : "s"} added`);
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function addText(text: string) {
    if (!text.trim()) {
      setStatus("No readable text found");
      return;
    }

    setIsBusy(true);
    try {
      const nextItems = await invoke<ShelfItem[]>("add_shelf_text", { text });
      setItems(nextItems);
      setStatus("Text added");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function removeItem(id: number) {
    setIsBusy(true);
    try {
      const nextItems = await invoke<ShelfItem[]>("remove_shelf_item", { id });
      setItems(nextItems);
      setStatus("Item removed");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function clearItems() {
    setIsBusy(true);
    try {
      const nextItems = await invoke<ShelfItem[]>("clear_shelf");
      setItems(nextItems);
      setStatus("Shelf cleared");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function openShelfPath(path: string) {
    try {
      await invoke("open_shelf_path", { path });
      setStatus("Item opened");
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function revealShelfPath(path: string) {
    try {
      await invoke("reveal_shelf_path", { path });
      setStatus(isWindows ? "Item revealed in Explorer" : "Item revealed in Finder");
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function openMainWindow() {
    try {
      await invoke("open_main_window");
      setStatus("DropAir window opened");
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function sendShelfItems(peerId: string) {
    const itemIds = items
      .filter((item) => item.kind !== "directory")
      .map((item) => item.id);
    if (itemIds.length === 0) {
      setStatus("No files or text to send");
      return;
    }
    setIsBusy(true);
    try {
      await invoke("send_shelf_items", { peerId, itemIds });
      setStatus("Transfer started");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function toggleAutostart() {
    setIsBusy(true);
    try {
      const enabled = await invoke<boolean>("set_autostart", {
        enabled: !launchAtLogin
      });
      setLaunchAtLogin(enabled);
      setStatus(enabled ? "Launch at login enabled" : "Launch at login disabled");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function updateShakeEnabled() {
    setIsBusy(true);
    try {
      const settings = await invoke<AppSettings>("set_shake_enabled", {
        enabled: !shakeEnabled
      });
      setShakeEnabledState(settings.shakeEnabled);
      setShakeSensitivityState(settings.shakeSensitivity);
      const monitorStatus = await invoke<string>("shake_monitor_status");
      setShakeStatus(monitorStatus);
      setStatus(settings.shakeEnabled ? "Shake detection enabled" : "Shake detection disabled");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  async function updateShakeSensitivity(sensitivity: number) {
    setShakeSensitivityState(sensitivity);
    try {
      const settings = await invoke<AppSettings>("set_shake_sensitivity", { sensitivity });
      setShakeSensitivityState(settings.shakeSensitivity);
      setStatus(`Shake sensitivity set to ${settings.shakeSensitivity}`);
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function openAccessibilitySettings() {
    try {
      await invoke("open_accessibility_settings");
      setStatus("Accessibility settings opened");
      window.setTimeout(() => {
        void invoke<boolean>("accessibility_permission_status")
          .then(setAccessibilityAllowed)
          .catch(() => undefined);
      }, 1000);
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function testShakeShelf() {
    try {
      await invoke("show_shake_shelf_for_test");
      setStatus("Test Shelf opened");
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function hideShakeShelf() {
    try {
      await invoke("hide_shake_shelf");
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  function startShakeShelfDrag() {
    void invoke("start_shake_shelf_drag").catch((error) => {
      setStatus(toErrorMessage(error));
    });
  }

  function beginNativeFileDrag(event: MouseEvent<HTMLElement>, path: string) {
    if (event.button !== 0) return;
    event.preventDefault();
    void invoke("begin_native_file_drag", { path }).catch((error) => {
      setStatus(toErrorMessage(error));
    });
  }

  function beginTextDrag(event: DragEvent<HTMLElement>, content: string) {
    event.dataTransfer.effectAllowed = "copy";
    event.dataTransfer.setData("text/plain", content);
  }

  function beginWindowsFileDrag(event: DragEvent<HTMLElement>, path: string) {
    event.dataTransfer.effectAllowed = "copy";
    const fileUrl = pathToFileUrl(path);
    event.dataTransfer.setData("text/uri-list", fileUrl);
    event.dataTransfer.setData("text/plain", fileUrl);
  }

  function handleDragOver(event: DragEvent<HTMLElement>) {
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    setIsDragging(true);
  }

  function handleDragLeave(event: DragEvent<HTMLElement>) {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
      setIsDragging(false);
    }
  }

  function handleDrop(event: DragEvent<HTMLElement>) {
    event.preventDefault();
    setIsDragging(false);

    const paths = Array.from(event.dataTransfer.files)
      .map((file) => (file as DropAirFile).path || file.webkitRelativePath)
      .filter((path): path is string => Boolean(path));

    if (paths.length > 0) {
      void addPaths(paths);
      return;
    }

    const text = event.dataTransfer.getData("text/plain");
    void addText(text);
  }

  if (isShelfWindow) {
    return (
      <main
        className={`shake-shelf${isDragging ? " is-dragging" : ""}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <div className="shake-shelf-topline">
          <span className="shake-shelf-drag-handle" onMouseDown={startShakeShelfDrag}>
            DropAir Shelf
          </span>
          <div className="shake-shelf-actions">
            <span>{items.length} queued</span>
            <button
              className="shake-shelf-icon"
              type="button"
              onClick={() => void openMainWindow()}
              title="Open DropAir"
              aria-label="Open DropAir"
            >
              <PanelTopOpen size={14} />
            </button>
            <button
              className="shake-shelf-close"
              type="button"
              onClick={() => void hideShakeShelf()}
              title="Close Shelf"
              aria-label="Close Shelf"
            >
              <X size={14} />
            </button>
          </div>
        </div>
        {items.length === 0 ? (
          <div className="shake-shelf-empty">
            <FileArchive size={24} />
            <strong>Drop files or text here</strong>
          </div>
        ) : (
          <div className="shake-shelf-items">
            {items.map((item) => (
              <div
                className={`shake-shelf-item${item.kind === "file" ? " is-file" : ""}${item.kind === "text" ? " is-text" : ""}`}
                key={item.id}
                draggable={
                  item.kind === "text" ||
                  (isWindows && item.kind === "file" && !platformCapabilities?.nativeFileDragSupported)
                }
                onDragStart={
                  item.kind === "text" && item.content
                    ? (event) => beginTextDrag(event, item.content as string)
                    : isWindows && item.kind === "file"
                      ? (event) => beginWindowsFileDrag(event, item.path)
                      : undefined
                }
                onMouseDown={
                  item.kind === "file" && platformCapabilities?.nativeFileDragSupported
                    ? (event) => beginNativeFileDrag(event, item.path)
                    : undefined
                }
                onDoubleClick={
                  item.kind !== "text"
                    ? () => void openShelfPath(item.path)
                    : undefined
                }
              >
                {item.kind === "directory" ? <Folder size={16} /> : <FileText size={16} />}
                <span>{item.name}</span>
                {item.kind !== "text" && (
                  <>
                    <button
                      className="shake-shelf-icon"
                      type="button"
                      title={`Open ${item.name}`}
                      aria-label={`Open ${item.name}`}
                      draggable={false}
                      onMouseDown={(event) => event.stopPropagation()}
                      onDragStart={(event) => event.stopPropagation()}
                      onDoubleClick={(event) => event.stopPropagation()}
                      onClick={() => void openShelfPath(item.path)}
                    >
                      <ExternalLink size={13} />
                    </button>
                    <button
                      className="shake-shelf-icon"
                      type="button"
                      title={isWindows ? "Show in Explorer" : "Show in Finder"}
                      aria-label={isWindows ? "Show in Explorer" : "Show in Finder"}
                      draggable={false}
                      onMouseDown={(event) => event.stopPropagation()}
                      onDragStart={(event) => event.stopPropagation()}
                      onDoubleClick={(event) => event.stopPropagation()}
                      onClick={() => void revealShelfPath(item.path)}
                    >
                      <FolderOpen size={13} />
                    </button>
                  </>
                )}
                <button
                  className="shake-shelf-remove"
                  type="button"
                  draggable={false}
                  onMouseDown={(event) => event.stopPropagation()}
                  onDragStart={(event) => event.stopPropagation()}
                  onDoubleClick={(event) => event.stopPropagation()}
                  onClick={() => void removeItem(item.id)}
                  title={`Remove ${item.name}`}
                  aria-label={`Remove ${item.name}`}
                >
                  <X size={14} />
                </button>
              </div>
            ))}
          </div>
        )}
      </main>
    );
  }

  return (
    <main
      className={`app-shell${isDragging ? " is-dragging" : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <aside className="sidebar" aria-label="DropAir navigation">
        <div className="brand">
          <div className="mark" aria-hidden="true">
            DA
          </div>
          <div>
            <strong>DropAir</strong>
            <span>0.1.0</span>
          </div>
        </div>

        <nav className="nav-list" aria-label="Primary">
          <button
            className={`nav-item${mainView === "shelf" ? " is-active" : ""}`}
            type="button"
            onClick={() => setMainView("shelf")}
          >
            <FileArchive size={18} />
            Shelf
          </button>
          <button
            className={`nav-item${mainView === "devices" ? " is-active" : ""}`}
            type="button"
            onClick={() => setMainView("devices")}
          >
            <Laptop size={18} />
            Devices
          </button>
          <button
            className={`nav-item${mainView === "settings" ? " is-active" : ""}`}
            type="button"
            onClick={() => setMainView("settings")}
          >
            <Settings2 size={18} />
            Settings
          </button>
        </nav>

        <div className="status-box">
          {isBusy ? <Loader2 className="spin" size={18} /> : <CheckCircle2 size={18} />}
          <span>{status}</span>
        </div>
        <div className={`monitor-status is-${shakeStatus}`}>
          <span className="monitor-dot" aria-hidden="true" />
          <span>{formatShakeStatus(shakeStatus)}</span>
        </div>
        {shakeDiagnostics && (
          <div className="monitor-diagnostics">
            D {shakeDiagnostics.mouseDowns} / Motion {shakeDiagnostics.motionSamples} / Turns{" "}
            {shakeDiagnostics.maxDirectionChanges} / Trigger {shakeDiagnostics.triggers}
          </div>
        )}
      </aside>

      {mainView === "shelf" ? (
      <section className="workspace" aria-label="Shelf workspace">
        <header className="toolbar">
          <div>
            <p className="eyebrow">{isWindows ? "Windows shelf" : "Temporary shelf"}</p>
            <h1>{items.length} item{items.length === 1 ? "" : "s"}</h1>
          </div>
          <div className="toolbar-actions">
            <button
              className="icon-button"
              type="button"
              onClick={() => void testShakeShelf()}
              title="Show Shelf"
              aria-label="Show Shelf"
            >
              <PanelTopOpen size={18} />
            </button>
            <button
              className="icon-button"
              type="button"
              onClick={() => void clearItems()}
              disabled={items.length === 0 || isBusy}
              title="Clear shelf"
              aria-label="Clear shelf"
            >
              <Trash2 size={18} />
            </button>
            <button
              className="primary-button"
              type="button"
              disabled={items.length === 0 || peers.length === 0 || isBusy}
              title="Send to device"
              onClick={() => {
                const peerId = selectedPeerId ?? peers[0]?.id;
                if (peerId) void sendShelfItems(peerId);
              }}
            >
              <Send size={18} />
              Send
            </button>
          </div>
        </header>

        <section className="drop-zone" aria-label="Drop target">
          {items.length === 0 ? (
            <div className="empty-state">
              <FileArchive size={34} />
              <strong>Drop files, folders, or text here</strong>
              <span>
                {isWindows
                  ? "Drag files, folders, or text, then shake left and right (or press Ctrl+Shift+Space) to show Shelf."
                  : "Drag an item or selected text, then shake left and right to reveal Shelf."}
              </span>
            </div>
          ) : (
            <div className="item-list">
              {items.map((item) => (
                <article
                  className={`shelf-item${item.kind === "text" ? " is-text" : ""}`}
                  key={item.id}
                  draggable={item.kind === "text"}
                  onDragStart={
                    item.kind === "text" && item.content
                      ? (event) => beginTextDrag(event, item.content as string)
                      : undefined
                  }
                  onDoubleClick={
                    item.kind === "file" || item.kind === "directory"
                      ? () => void openShelfPath(item.path)
                      : undefined
                  }
                >
                  <div className="item-icon" aria-hidden="true">
                    {item.kind === "directory" ? <Folder size={20} /> : <FileText size={20} />}
                  </div>
                  <div className="item-copy">
                    <strong>{item.name}</strong>
                    <span>{item.content ?? item.path}</span>
                  </div>
                  <div className="item-meta">
                    <span>{formatSize(item.size)}</span>
                    {(item.kind === "file" || item.kind === "directory") && (
                      <>
                        <button
                          className="icon-button small"
                          type="button"
                          onClick={() => void openShelfPath(item.path)}
                          title="Open item"
                          aria-label={`Open ${item.name}`}
                        >
                          <ExternalLink size={16} />
                        </button>
                        <button
                          className="icon-button small"
                          type="button"
                          onClick={() => void revealShelfPath(item.path)}
                          title="Show in Finder"
                          aria-label={`Show ${item.name} in Finder`}
                        >
                          <FolderOpen size={16} />
                        </button>
                      </>
                    )}
                    <button
                      className="icon-button small"
                      type="button"
                      onClick={() => void removeItem(item.id)}
                      disabled={isBusy}
                      title="Remove item"
                      aria-label={`Remove ${item.name}`}
                    >
                      <X size={16} />
                    </button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>

        <footer className="summary-bar">
          <span>{items.length} queued</span>
          <span>{formatSize(totalSize)}</span>
          <span>{peers.length > 0 ? "LAN transfer ready" : "Searching for devices"}</span>
        </footer>
      </section>
      ) : mainView === "devices" ? (
        <section className="workspace" aria-label="Devices workspace">
          <header className="toolbar">
            <div>
              <p className="eyebrow">LAN</p>
              <h1>Devices</h1>
            </div>
          </header>
          <div className="devices-list">
            {peers.length === 0 ? (
              <div className="empty-state">
                <Laptop size={34} />
                <strong>No devices found</strong>
                <span>
                  Start DropAir on another computer on the same network. Discovery runs in the
                  background.
                </span>
              </div>
            ) : (
              peers.map((peer) => (
                <article
                  className={`device-row${selectedPeerId === peer.id ? " is-selected" : ""}`}
                  key={peer.id}
                  onClick={() => setSelectedPeerId(peer.id)}
                >
                  <div className="item-icon" aria-hidden="true">
                    <Laptop size={20} />
                  </div>
                  <div className="item-copy">
                    <strong>{peer.name}</strong>
                    <span>
                      {peer.address}:{peer.port}
                    </span>
                  </div>
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={items.length === 0 || isBusy}
                    onClick={(event) => {
                      event.stopPropagation();
                      void sendShelfItems(peer.id);
                    }}
                  >
                    <Send size={16} />
                    Send items
                  </button>
                </article>
              ))
            )}
          </div>
        </section>
      ) : (
        <section className="workspace settings-workspace" aria-label="Settings">
          <header className="toolbar">
            <div>
              <p className="eyebrow">Application</p>
              <h1>Settings</h1>
            </div>
          </header>

          <div className="settings-list">
            {shakeSupported && (
              <>
                <div className="setting-row">
                  <div className="setting-copy">
                    <strong>Shake detection</strong>
                    <span>Reveal Shelf when a dragged item is shaken left and right.</span>
                  </div>
                  <button
                    className={`toggle-control${shakeEnabled ? " is-on" : ""}`}
                    type="button"
                    role="switch"
                    aria-checked={shakeEnabled}
                    aria-label="Shake detection"
                    disabled={!settingsReady || isBusy}
                    onClick={() => void updateShakeEnabled()}
                  >
                    <span />
                  </button>
                </div>

                <div className="setting-row">
                  <div className="setting-copy">
                    <strong>Shake sensitivity</strong>
                    <span>Higher values require less horizontal movement.</span>
                  </div>
                  <div className={`sensitivity-control${shakeEnabled ? "" : " is-disabled"}`}>
                    <span>Low</span>
                    <input
                      type="range"
                      min="1"
                      max="5"
                      step="1"
                      value={shakeSensitivity}
                      aria-label="Shake sensitivity"
                      disabled={!settingsReady || !shakeEnabled}
                      onChange={(event) => void updateShakeSensitivity(Number(event.target.value))}
                    />
                    <output>{shakeSensitivity}</output>
                    <span>High</span>
                  </div>
                </div>
              </>
            )}

            {isWindows && (
              <div className="setting-row">
                <div className="setting-copy">
                  <strong>Windows Shelf shortcut</strong>
                  <span>Use the global shortcut to show or hide Shelf while working in another app.</span>
                </div>
                <kbd>Ctrl+Shift+Space</kbd>
              </div>
            )}

            <div className="setting-row">
              <div className="setting-copy">
                <strong>Launch at login</strong>
                <span>Start DropAir in the background when you sign in.</span>
              </div>
              <button
                className={`toggle-control${launchAtLogin ? " is-on" : ""}`}
                type="button"
                role="switch"
                aria-checked={launchAtLogin}
                aria-label="Launch at login"
                disabled={!settingsReady || isBusy}
                onClick={() => void toggleAutostart()}
              >
                <span />
              </button>
            </div>

            {accessibilityRequired && <div className="setting-row">
              <div className="setting-copy permission-copy">
                <strong>
                  <ShieldCheck size={16} />
                  Accessibility permission
                </strong>
                <span>Required by macOS for reliable global drag monitoring.</span>
              </div>
              <div className="permission-actions">
                <span className={`permission-status${accessibilityAllowed ? " is-allowed" : ""}`}>
                  {accessibilityAllowed === null
                    ? "Checking"
                    : accessibilityAllowed
                      ? "Allowed"
                      : "Not allowed"}
                </span>
                <button
                  className="secondary-button"
                  type="button"
                  disabled={!settingsReady}
                  onClick={() => void openAccessibilitySettings()}
                >
                  <ExternalLink size={16} />
                  Open System Settings
                </button>
              </div>
            </div>}
          </div>
        </section>
      )}
    </main>
  );
}

function formatSize(size: number | null) {
  if (size === null) {
    return "Folder";
  }

  if (size === 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  const unitIndex = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1);
  const value = size / 1024 ** unitIndex;
  return `${value >= 10 || unitIndex === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`;
}

function toErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatShakeStatus(status: string) {
  if (status === "disabled") return "Shake monitor: Disabled";
  if (status === "listening") return "Shake monitor: Listening";
  if (status === "permissionRequired") return "Shake monitor: Permission required";
  if (status === "unsupported") return "Shelf shortcut: Ctrl+Shift+Space";
  return "Shake monitor: Starting";
}

function pathToFileUrl(path: string) {
  const normalized = path.replace(/\\/g, "/");
  return normalized.startsWith("/") ? `file://${encodeURI(normalized)}` : `file:///${encodeURI(normalized)}`;
}

export default App;
