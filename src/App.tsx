import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { relaunch } from "@tauri-apps/plugin-process";
import { check as checkForAppUpdate, type Update } from "@tauri-apps/plugin-updater";
import {
  CheckCircle2,
  ClipboardPaste,
  Download,
  ExternalLink,
  File,
  FileArchive,
  FileBraces,
  FileCode,
  FileCog,
  FileImage,
  FileKey,
  FileMusic,
  FileSpreadsheet,
  FileText,
  FileType,
  FileVideoCamera,
  Folder,
  FolderOpen,
  Laptop,
  Loader2,
  PanelTopOpen,
  Presentation,
  RefreshCw,
  Settings2,
  Send,
  ShieldCheck,
  SquareCheck,
  Trash2,
  X,
  type LucideIcon
} from "lucide-react";
import { DragEvent, MouseEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

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

type TooltipState = {
  text: string;
  x: number;
  y: number;
  anchorX: number;
  placement: "above" | "below";
} | null;

type ThemePreference = "system" | "light" | "dark";

const THEME_STORAGE_KEY = "dropair.theme";

const THEME_OPTIONS: Array<{ value: ThemePreference; label: string; tooltip: string }> = [
  { value: "system", label: "System", tooltip: "Match the operating system theme" },
  { value: "light", label: "Light", tooltip: "Always use the light theme" },
  { value: "dark", label: "Dark", tooltip: "Always use the dark theme" }
];

function resolveTheme(preference: ThemePreference) {
  if (preference === "system") {
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return preference;
}

function applyTheme(preference: ThemePreference) {
  document.documentElement.dataset.theme = resolveTheme(preference);
}

const TOOLTIP_SELECTOR = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  "[role='switch']",
  "[role='button']",
  "[data-tooltip]"
].join(", ");

function readTooltipText(element: Element) {
  const explicit = element.getAttribute("data-tooltip");
  if (explicit) return explicit;
  return element.getAttribute("aria-label");
}

function tooltipTargetAt(x: number, y: number) {
  const element = document.elementFromPoint(x, y);
  if (!element) return null;
  const target = element.closest(TOOLTIP_SELECTOR);
  if (!target || !readTooltipText(target)) return null;
  return target;
}

type ShakeDiagnostics = {
  mouseDowns: number;
  motionSamples: number;
  maxDirectionChanges: number;
  triggers: number;
};

type AppSettings = {
  shakeEnabled: boolean;
  shakeSensitivity: number;
  deviceName: string;
  downloadDirectory: string | null;
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
  addresses: string[];
  lastSeen: number;
  manual: boolean;
  linked: boolean;
};

type TransferStatusInfo = {
  udpListenerUp: boolean;
  discoveryBroadcastUp: boolean;
  tcpListenerUp: boolean;
  discoveryPort: number;
  transferPort: number;
};

type TransferCheck = {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
};

type TransferSelfCheck = {
  checkedAt: number;
  diagnostics: TransferStatusInfo & {
    peerCount: number;
    receivedDirectoryWritable: boolean;
    loopbackTcpOk: boolean;
    lastError: string | null;
  };
  checks: TransferCheck[];
};

type MainView = "shelf" | "devices" | "sent" | "received" | "settings";

type TransferEvent = {
  peerId: string;
  state: "sent" | "received" | "error";
  message: string;
};

type TransferRecord = TransferEvent & {
  id: string;
  at: number;
};

function App() {
  const [items, setItems] = useState<ShelfItem[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [tooltip, setTooltip] = useState<TooltipState>(null);
  const [themePreference, setThemePreference] = useState<ThemePreference>(() => {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  });
  const [shelfSelectionMode, setShelfSelectionMode] = useState(false);
  const [shelfSelectedIds, setShelfSelectedIds] = useState<number[]>([]);
  const [appVersion, setAppVersion] = useState("");
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [updateProgress, setUpdateProgress] = useState("");
  const pendingUpdateRef = useRef<Update | null>(null);
  const [status, setStatus] = useState("Ready");
  const [isBusy, setIsBusy] = useState(false);
  const [shakeStatus, setShakeStatus] = useState("starting");
  const [shakeDiagnostics, setShakeDiagnostics] = useState<ShakeDiagnostics | null>(null);
  const [mainView, setMainView] = useState<MainView>("shelf");
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [shakeEnabled, setShakeEnabledState] = useState(true);
  const [shakeSensitivity, setShakeSensitivityState] = useState(3);
  const [accessibilityAllowed, setAccessibilityAllowed] = useState<boolean | null>(null);
  const [deviceName, setDeviceName] = useState("");
  const [downloadDirectory, setDownloadDirectory] = useState("");
  const [settingsReady, setSettingsReady] = useState(false);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [selectedItemIds, setSelectedItemIds] = useState<number[]>([]);
  const [selectionMode, setSelectionMode] = useState(false);
  const [sentRecords, setSentRecords] = useState<TransferRecord[]>([]);
  const [receivedRecords, setReceivedRecords] = useState<TransferRecord[]>([]);
  const [transferStatusInfo, setTransferStatusInfo] = useState<TransferStatusInfo | null>(null);
  const [transferSelfCheck, setTransferSelfCheck] = useState<TransferSelfCheck | null>(null);
  const [isCheckingTransfer, setIsCheckingTransfer] = useState(false);
  const [isScanningDevices, setIsScanningDevices] = useState(false);
  const [isAddingPeer, setIsAddingPeer] = useState(false);
  const [showManualPeerForm, setShowManualPeerForm] = useState(false);
  const [manualAddress, setManualAddress] = useState("");
  const [manualPort, setManualPort] = useState("47654");
  const [manualName, setManualName] = useState("");
  const [shelfText, setShelfText] = useState("");
  const [shelfSendItemId, setShelfSendItemId] = useState<number | null>(null);
  const [testingPeerId, setTestingPeerId] = useState<string | null>(null);
  const [platformCapabilities, setPlatformCapabilities] =
    useState<PlatformCapabilities | null>(null);
  const shakeSupported = platformCapabilities?.shakeSupported ?? true;
  const isWindows = platformCapabilities?.platform === "windows";
  const accessibilityRequired = platformCapabilities?.accessibilityRequired ?? false;

  const totalSize = useMemo(
    () => items.reduce((sum, item) => sum + (item.size ?? 0), 0),
    [items]
  );
  const linkedPeers = useMemo(() => peers.filter((peer) => peer.linked), [peers]);

  useEffect(() => {
    void refreshShelf();
  }, []);

  function showTooltipFor(element: Element | null) {
    const target = element ? element.closest(TOOLTIP_SELECTOR) : null;
    const text = target ? readTooltipText(target) : null;
    if (!target || !text) {
      setTooltip(null);
      return;
    }
    const rect = target.getBoundingClientRect();
    const placement = rect.bottom + 48 > window.innerHeight ? "above" : "below";
    const maxWidth = Math.min(280, window.innerWidth - 16);
    const estimatedWidth = Math.min(maxWidth, text.length * 6.5 + 20);
    const halfWidth = estimatedWidth / 2 + 8;
    const centerX = rect.left + rect.width / 2;
    setTooltip({
      text,
      anchorX: centerX,
      x: Math.min(Math.max(centerX, halfWidth), window.innerWidth - halfWidth),
      y: placement === "below" ? rect.bottom + 8 : rect.top - 8,
      placement
    });
  }

  useEffect(() => {
    const handleMouseOver = (event: globalThis.MouseEvent) => {
      if (event.target instanceof Element) showTooltipFor(event.target);
    };
    const handleMouseOut = (event: globalThis.MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const element = target.closest(TOOLTIP_SELECTOR);
      if (element && element.contains(event.relatedTarget as Node | null)) return;
      setTooltip(null);
    };
    const hideTooltip = () => setTooltip(null);

    document.addEventListener("mouseover", handleMouseOver);
    document.addEventListener("mouseout", handleMouseOut);
    document.addEventListener("pointerdown", hideTooltip, true);
    window.addEventListener("scroll", hideTooltip, true);
    window.addEventListener("blur", hideTooltip);
    return () => {
      document.removeEventListener("mouseover", handleMouseOver);
      document.removeEventListener("mouseout", handleMouseOut);
      document.removeEventListener("pointerdown", hideTooltip, true);
      window.removeEventListener("scroll", hideTooltip, true);
      window.removeEventListener("blur", hideTooltip);
    };
  }, []);

  // The floating Shelf is a non-activating panel, so it does not receive hover
  // events until it becomes key. Rust reports the pointer position inside the
  // panel instead, and we resolve the element under it here.
  useEffect(() => {
    if (!isShelfWindow) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<[number, number] | null>("shelf-pointer", (event) => {
      const point = event.payload;
      if (!point) {
        setTooltip(null);
        return;
      }
      showTooltipFor(tooltipTargetAt(point[0], point[1]));
    }).then((nextUnlisten) => {
      if (cancelled) {
        nextUnlisten();
      } else {
        unlisten = nextUnlisten;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    applyTheme(themePreference);
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = () => applyTheme(themePreference);
    media.addEventListener("change", handleChange);
    return () => media.removeEventListener("change", handleChange);
  }, [themePreference]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<ThemePreference>("theme-changed", (event) => {
      setThemePreference(event.payload);
    }).then((nextUnlisten) => {
      if (cancelled) {
        nextUnlisten();
      } else {
        unlisten = nextUnlisten;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    try {
      const sent = localStorage.getItem("dropair.sent-records");
      const received = localStorage.getItem("dropair.received-records");
      if (sent) setSentRecords(JSON.parse(sent) as TransferRecord[]);
      if (received) setReceivedRecords(JSON.parse(received) as TransferRecord[]);
    } catch {
      // Ignore malformed history and continue with an empty view.
    }
  }, []);

  useEffect(() => {
    localStorage.setItem("dropair.sent-records", JSON.stringify(sentRecords));
  }, [sentRecords]);

  useEffect(() => {
    localStorage.setItem("dropair.received-records", JSON.stringify(receivedRecords));
  }, [receivedRecords]);

  useEffect(() => {
    let unlistenPeers: (() => void) | undefined;
    let unlistenTransfer: (() => void) | undefined;
    void listen<PeerInfo[]>("peers-changed", (event) => setPeers(event.payload)).then(
      (nextUnlisten) => {
        unlistenPeers = nextUnlisten;
      }
    );
    void listen<TransferEvent>("transfer-status", (event) => {
      const record = { ...event.payload, id: `${Date.now()}-${Math.random()}`, at: Date.now() };
      setStatus(event.payload.message);
      if (event.payload.state === "sent") {
        setSentRecords((records) => [record, ...records].slice(0, 100));
      } else if (event.payload.state === "received") {
        setReceivedRecords((records) => [record, ...records].slice(0, 100));
      }
    }).then(
      (nextUnlisten) => {
        unlistenTransfer = nextUnlisten;
      }
    );
    void invoke<PeerInfo[]>("list_peers")
      .then(setPeers)
      .catch(() => undefined);
    void invoke<TransferStatusInfo>("transfer_status")
      .then(setTransferStatusInfo)
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
        const [autostart, appSettings, capabilities, accessibility, effectiveDownloadDirectory] = await Promise.all([
          invoke<boolean>("autostart_enabled"),
          invoke<AppSettings>("app_settings"),
          invoke<PlatformCapabilities>("platform_capabilities"),
          invoke<boolean>("accessibility_permission_status"),
          invoke<string>("download_directory")
        ]);
        setLaunchAtLogin(autostart);
        setShakeEnabledState(appSettings.shakeEnabled);
        setShakeSensitivityState(appSettings.shakeSensitivity);
        setDeviceName(appSettings.deviceName);
        setDownloadDirectory(appSettings.downloadDirectory ?? effectiveDownloadDirectory);
        setPlatformCapabilities(capabilities);
        setAccessibilityAllowed(accessibility);
      } catch (error) {
        setStatus(toErrorMessage(error));
      } finally {
        setSettingsReady(true);
      }
    };
    void loadSettings();
    void getVersion().then(setAppVersion).catch(() => undefined);
    void checkForUpdates(false);
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
        if (event.payload.paths.length > 0) {
          void addPaths(event.payload.paths);
        } else {
          void captureDraggedText();
        }
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

  async function captureDraggedText() {
    try {
      const text = await invoke<string | null>("capture_dragged_text");
      if (text) await addText(text);
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function pasteText() {
    try {
      if (!navigator.clipboard?.readText) {
        throw new Error("Clipboard text access is unavailable");
      }
      await addText(await navigator.clipboard.readText());
    } catch (error) {
      setStatus(toErrorMessage(error));
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

  async function sendShelfItems(peerId: string, selectedItemIds?: number[]) {
    const itemIds = selectedItemIds ?? items
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

  function toggleItemSelection(id: number) {
    setSelectedItemIds((ids) => (ids.includes(id) ? ids.filter((itemId) => itemId !== id) : [...ids, id]));
  }

  async function sendSelectedItems() {
    const peerId = linkedPeers.some((peer) => peer.id === selectedPeerId)
      ? selectedPeerId
      : linkedPeers[0]?.id;
    if (!peerId) {
      setStatus("Add a linked device first");
      return;
    }
    await sendShelfItems(peerId, selectedItemIds);
    setSelectedItemIds([]);
    setSelectionMode(false);
  }

  async function togglePeerLinked(peer: PeerInfo) {
    try {
      const nextPeers = await invoke<PeerInfo[]>("set_peer_linked", {
        peerId: peer.id,
        linked: !peer.linked
      });
      setPeers(nextPeers);
      if (!peer.linked) {
        setSelectedPeerId(peer.id);
      } else if (selectedPeerId === peer.id) {
        setSelectedPeerId(null);
      }
      setStatus(!peer.linked ? `${peer.name} linked` : `${peer.name} unlinked`);
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function testPeerConnection(peer: PeerInfo) {
    setTestingPeerId(peer.id);
    try {
      await invoke("test_peer_connection", { peerId: peer.id });
      setStatus(`${peer.name} connection test passed`);
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setTestingPeerId(null);
    }
  }

  async function addShelfText() {
    const text = shelfText;
    if (!text.trim()) {
      setStatus("Enter text to add");
      return;
    }
    await addText(text);
    setShelfText("");
  }

  async function runTransferSelfCheck() {
    setIsCheckingTransfer(true);
    try {
      const result = await invoke<TransferSelfCheck>("transfer_self_check");
      setTransferSelfCheck(result);
      setTransferStatusInfo(result.diagnostics);
      const failed = result.checks.filter((check) => !check.ok).length;
      setStatus(failed === 0 ? "Transfer self-check passed" : `${failed} transfer checks need attention`);
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsCheckingTransfer(false);
    }
  }

  async function scanLanDevices() {
    setIsScanningDevices(true);
    try {
      await invoke("scan_lan_devices");
      setStatus("LAN device scan started");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsScanningDevices(false);
    }
  }

  async function addManualPeer() {
    const address = manualAddress.trim();
    const port = Number(manualPort);
    if (!address) {
      setStatus("Enter a device address");
      return;
    }
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      setStatus("Enter a valid port");
      return;
    }
    setIsAddingPeer(true);
    try {
      const peer = await invoke<PeerInfo>("add_manual_peer", {
        address,
        port,
        name: manualName.trim() || null
      });
      setPeers((currentPeers) => {
        const withoutEndpoint = currentPeers.filter(
          (currentPeer) => currentPeer.address !== peer.address || currentPeer.port !== peer.port
        );
        return [...withoutEndpoint, peer];
      });
      setSelectedPeerId(peer.id);
      setManualAddress("");
      setManualName("");
      setShowManualPeerForm(false);
      setStatus(`Added ${peer.name}`);
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsAddingPeer(false);
    }
  }

  async function removePeer(peer: PeerInfo) {
    try {
      await invoke("remove_peer", { peerId: peer.id });
      setPeers((currentPeers) => currentPeers.filter((currentPeer) => currentPeer.id !== peer.id));
      if (selectedPeerId === peer.id) setSelectedPeerId(null);
      setStatus(`Removed ${peer.name}`);
    } catch (error) {
      setStatus(toErrorMessage(error));
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

  async function updateDownloadDirectory(directory: string) {
    setIsBusy(true);
    try {
      const settings = await invoke<AppSettings>("set_download_directory", { directory });
      const effective = await invoke<string>("download_directory");
      setDownloadDirectory(settings.downloadDirectory ?? effective);
      setStatus(settings.downloadDirectory ? "Download location updated" : "Using default Downloads folder");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
    }
  }

  function updateTheme(preference: ThemePreference) {
    setThemePreference(preference);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, preference);
    } catch {
      // Ignore storage failures; the choice still applies for this session.
    }
    void emit("theme-changed", preference);
  }

  async function checkForUpdates(manual: boolean) {
    if (isShelfWindow) return;
    if (manual) {
      setIsBusy(true);
      setStatus("Checking for updates…");
    }
    try {
      const update = await checkForAppUpdate();
      if (!update) {
        pendingUpdateRef.current = null;
        setUpdateVersion(null);
        if (manual) setStatus("DropAir is up to date");
        return;
      }
      pendingUpdateRef.current = update;
      setUpdateVersion(update.version);
      setStatus(`Update available: v${update.version}`);
    } catch (error) {
      if (manual) setStatus(toErrorMessage(error));
    } finally {
      if (manual) setIsBusy(false);
    }
  }

  async function installUpdate() {
    const update = pendingUpdateRef.current;
    if (!update) return;
    setIsBusy(true);
    setUpdateProgress("Preparing download…");
    try {
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
          setUpdateProgress("Downloading update…");
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setUpdateProgress(
            total > 0
              ? `Downloading update… ${Math.min(100, Math.round((downloaded / total) * 100))}%`
              : "Downloading update…"
          );
        } else if (event.event === "Finished") {
          setUpdateProgress("Installing update…");
        }
      });
      setUpdateProgress("Restarting…");
      await relaunch();
    } catch (error) {
      setUpdateProgress("");
      setStatus(toErrorMessage(error));
      setIsBusy(false);
    }
  }

  async function updateDeviceName() {
    setIsBusy(true);
    try {
      const settings = await invoke<AppSettings>("set_device_name", { name: deviceName });
      setDeviceName(settings.deviceName);
      setStatus("Device name updated");
    } catch (error) {
      setStatus(toErrorMessage(error));
    } finally {
      setIsBusy(false);
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

  function beginNativeFileDrag(event: MouseEvent<HTMLElement>, paths: string[]) {
    if (event.button !== 0 || paths.length === 0) return;
    event.preventDefault();
    void invoke("begin_native_file_drag", { paths }).catch((error) => {
      setStatus(toErrorMessage(error));
    });
  }

  function beginTextDrag(event: DragEvent<HTMLElement>, content: string) {
    event.dataTransfer.effectAllowed = "copy";
    event.dataTransfer.setData("text/plain", content);
  }

  function beginWindowsFileDrag(event: DragEvent<HTMLElement>, paths: string[]) {
    if (paths.length === 0) return;
    event.dataTransfer.effectAllowed = "copy";
    const fileUrls = paths.map(pathToFileUrl).join("\r\n");
    event.dataTransfer.setData("text/uri-list", fileUrls);
    event.dataTransfer.setData("text/plain", fileUrls);
  }

  function shelfDragPaths(item: ShelfItem) {
    if (shelfSelectionMode && shelfSelectedIds.includes(item.id)) {
      const selected = items.filter(
        (candidate) =>
          shelfSelectedIds.includes(candidate.id) &&
          (candidate.kind === "file" || candidate.kind === "directory")
      );
      if (selected.length > 0) {
        return selected.map((candidate) => candidate.path);
      }
    }
    return [item.path];
  }

  function toggleShelfSelection(id: number) {
    setShelfSelectedIds((ids) =>
      ids.includes(id) ? ids.filter((itemId) => itemId !== id) : [...ids, id]
    );
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
    if (text.trim()) {
      void addText(text);
    } else {
      void captureDraggedText();
    }
  }

  if (isShelfWindow) {
    return (
      <main
        className={`shake-shelf${isDragging ? " is-dragging" : ""}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <span className="shake-shelf-resize-edge is-top" aria-hidden="true" />
        <span className="shake-shelf-resize-edge is-right" aria-hidden="true" />
        <span className="shake-shelf-resize-edge is-bottom" aria-hidden="true" />
        <span className="shake-shelf-resize-edge is-left" aria-hidden="true" />
        <div className="shake-shelf-topline">
          <span className="shake-shelf-drag-handle" onMouseDown={startShakeShelfDrag} data-tooltip="Drag to move the Shelf">
            DropAir Shelf
          </span>
          <div className="shake-shelf-actions">
            <button
              className={`shake-shelf-icon${shelfSelectionMode ? " is-active" : ""}`}
              type="button"
              onClick={() => {
                setShelfSelectionMode((enabled) => !enabled);
                setShelfSelectedIds([]);
              }}
              data-tooltip={shelfSelectionMode ? "Finish selecting" : "Select items to drag out together"}
              aria-label={shelfSelectionMode ? "Finish selecting" : "Select items"}
            >
              <SquareCheck size={14} />
            </button>
            <button
              className="shake-shelf-icon"
              type="button"
              onClick={() => void openMainWindow()}
              data-tooltip="Open DropAir"
              aria-label="Open DropAir"
            >
              <PanelTopOpen size={14} />
            </button>
            <button
              className="shake-shelf-close"
              type="button"
              onClick={() => void hideShakeShelf()}
              data-tooltip="Close Shelf"
              aria-label="Close Shelf"
            >
              <X size={14} />
            </button>
          </div>
        </div>
        <div
          className="shake-shelf-drop-space"
          onMouseDown={startShakeShelfDrag}
          aria-hidden="true"
        />
        <div className="shake-shelf-paste">
          <input
            type="text"
            value={shelfText}
            onChange={(event) => setShelfText(event.target.value)}
            onPaste={(event) => {
              const text = event.clipboardData.getData("text/plain");
              if (text.trim()) {
                event.preventDefault();
                setShelfText("");
                void addText(text);
              }
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void addShelfText();
              }
            }}
            placeholder="Paste text here…"
            aria-label="Paste text into Shelf"
          />
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
                className={`shake-shelf-item${item.kind === "file" ? " is-file" : ""}${item.kind === "text" ? " is-text" : ""}${item.kind === "file" || item.kind === "directory" ? " is-draggable" : ""}${shelfSelectionMode && shelfSelectedIds.includes(item.id) ? " is-selected" : ""}`}
                key={item.id}
                data-tooltip={
                  item.kind === "text"
                    ? "Drag to copy this text, or use Send"
                    : shelfSelectionMode
                      ? "Drag to move selected items out"
                      : item.kind === "directory"
                        ? "Drag this folder to another app, or use Send"
                        : "Drag this file to another app, or use Send"
                }
                draggable={
                  item.kind === "text" ||
                  (isWindows &&
                    (item.kind === "file" || item.kind === "directory") &&
                    !platformCapabilities?.nativeFileDragSupported)
                }
                onDragStart={
                  item.kind === "text" && item.content
                    ? (event) => beginTextDrag(event, item.content as string)
                    : isWindows && (item.kind === "file" || item.kind === "directory")
                      ? (event) => beginWindowsFileDrag(event, shelfDragPaths(item))
                      : undefined
                }
                onMouseDown={
                  (item.kind === "file" || item.kind === "directory") &&
                  platformCapabilities?.nativeFileDragSupported
                    ? (event) => beginNativeFileDrag(event, shelfDragPaths(item))
                    : undefined
                }
              >
                {shelfSelectionMode && (item.kind === "file" || item.kind === "directory") && (
                  <input
                    type="checkbox"
                    className="shake-shelf-select"
                    checked={shelfSelectedIds.includes(item.id)}
                    onChange={() => toggleShelfSelection(item.id)}
                    onMouseDown={(event) => event.stopPropagation()}
                    onDragStart={(event) => event.stopPropagation()}
                    data-tooltip={`Select ${item.name}`}
                    aria-label={`Select ${item.name}`}
                  />
                )}
                <ShelfItemIcon item={item} size={16} className="shake-shelf-item-icon" />
                <span className="shake-shelf-item-name">{item.name}</span>
                {item.kind !== "text" && (
                  <>
                    <button
                      className="shake-shelf-icon"
                      type="button"
                      data-tooltip={`Open ${item.name}`}
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
                      data-tooltip={isWindows ? "Show in Explorer" : "Show in Finder"}
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
                {shelfSendItemId === item.id && linkedPeers.length > 0 && (
                  <select
                    className="shake-shelf-item-peer-select"
                    value=""
                    autoFocus
                    onChange={(event) => {
                      const peerId = event.target.value;
                      if (peerId) {
                        setShelfSendItemId(null);
                        void sendShelfItems(peerId, [item.id]);
                      }
                    }}
                    onMouseDown={(event) => event.stopPropagation()}
                    aria-label={`Choose a device for ${item.name}`}
                  >
                    <option value="">Send to…</option>
                    {linkedPeers.map((peer) => (
                      <option value={peer.id} key={peer.id}>
                        {peer.name}
                      </option>
                    ))}
                  </select>
                )}
                <button
                  className="shake-shelf-send"
                  type="button"
                  draggable={false}
                  disabled={isBusy || item.kind === "directory"}
                  onMouseDown={(event) => event.stopPropagation()}
                  onDragStart={(event) => event.stopPropagation()}
                  onDoubleClick={(event) => event.stopPropagation()}
                  onClick={() => {
                    if (linkedPeers.length === 0) {
                      setStatus("Add a linked device first");
                    } else {
                      setShelfSendItemId((current) => (current === item.id ? null : item.id));
                    }
                  }}
                  data-tooltip={
                    linkedPeers.length === 0
                      ? "Add a linked device first"
                      : "Choose a linked device"
                  }
                  aria-label={`Send ${item.name} to selected device`}
                >
                  <Send size={13} />
                </button>
                <button
                  className="shake-shelf-remove"
                  type="button"
                  draggable={false}
                  onMouseDown={(event) => event.stopPropagation()}
                  onDragStart={(event) => event.stopPropagation()}
                  onDoubleClick={(event) => event.stopPropagation()}
                  onClick={() => void removeItem(item.id)}
                  data-tooltip={`Remove ${item.name}`}
                  aria-label={`Remove ${item.name}`}
                >
                  <X size={14} />
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="shake-shelf-statusbar" aria-live="polite">
          <span>{shelfSelectionMode ? `${shelfSelectedIds.length} selected` : `${items.length} queued`}</span>
          <span>{status}</span>
        </div>
        <TooltipBubble tooltip={tooltip} />
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
            <span>{appVersion || "0.2.4"}</span>
          </div>
        </div>

        <nav className="nav-list" aria-label="Primary">
          <button
            className={`nav-item${mainView === "shelf" ? " is-active" : ""}`}
            type="button"
            data-tooltip="Show the temporary Shelf"
            onClick={() => setMainView("shelf")}
          >
            <FileArchive size={18} />
            Shelf
          </button>
          <button
            className={`nav-item${mainView === "devices" ? " is-active" : ""}`}
            type="button"
            data-tooltip="Discover and link nearby devices"
            onClick={() => setMainView("devices")}
          >
            <Laptop size={18} />
            Devices
          </button>
          <button
            className={`nav-item${mainView === "sent" ? " is-active" : ""}`}
            type="button"
            data-tooltip="Transfers you have sent"
            onClick={() => setMainView("sent")}
          >
            <Send size={18} />
            Sent
          </button>
          <button
            className={`nav-item${mainView === "received" ? " is-active" : ""}`}
            type="button"
            data-tooltip="Files you have received"
            onClick={() => setMainView("received")}
          >
            <FolderOpen size={18} />
            Received
          </button>
          <button
            className={`nav-item${mainView === "settings" ? " is-active" : ""}`}
            type="button"
            data-tooltip="Shake, startup, and download preferences"
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
              className="primary-button"
              type="button"
              onClick={() => void pasteText()}
              disabled={isBusy}
              data-tooltip="Paste text from clipboard"
            >
              <ClipboardPaste size={18} />
              Paste
            </button>
            <button
              className="icon-button"
              type="button"
              onClick={() => void testShakeShelf()}
              data-tooltip="Show Shelf"
              aria-label="Show Shelf"
            >
              <PanelTopOpen size={18} />
            </button>
            <button
              className="icon-button"
              type="button"
              onClick={() => void clearItems()}
              disabled={items.length === 0 || isBusy}
              data-tooltip="Clear shelf"
              aria-label="Clear shelf"
            >
              <Trash2 size={18} />
            </button>
            <button
              className={`secondary-button${selectionMode ? " is-linked" : ""}`}
              type="button"
              data-tooltip={selectionMode ? "Exit selection mode" : "Select multiple items to send"}
              onClick={() => {
                setSelectionMode((enabled) => !enabled);
                setSelectedItemIds([]);
              }}
            >
              {selectionMode ? "Cancel select" : "Select items"}
            </button>
            {selectionMode ? (
              <button
                className="primary-button"
                type="button"
                disabled={selectedItemIds.length === 0 || linkedPeers.length === 0 || isBusy}
                data-tooltip="Send selected items"
                onClick={() => void sendSelectedItems()}
              >
                <Send size={18} />
                Send selected ({selectedItemIds.length})
              </button>
            ) : (
            <button
              className="primary-button"
              type="button"
              disabled={items.length === 0 || linkedPeers.length === 0 || isBusy}
              data-tooltip="Send to device"
              onClick={() => {
                const peerId = linkedPeers.some((peer) => peer.id === selectedPeerId)
                  ? selectedPeerId
                  : linkedPeers[0]?.id;
                if (peerId) void sendShelfItems(peerId);
              }}
            >
              <Send size={18} />
              Send
            </button>
            )}
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
                  className={`shelf-item${item.kind === "text" ? " is-text" : ""}${selectionMode ? " is-selecting" : ""}`}
                  key={item.id}
                  data-tooltip={item.kind === "text" ? "Drag to copy this text" : undefined}
                  draggable={item.kind === "text"}
                  onDragStart={
                    item.kind === "text" && item.content
                      ? (event) => beginTextDrag(event, item.content as string)
                      : undefined
                  }
                >
                  {selectionMode && item.kind !== "directory" && (
                    <input
                      className="item-select-checkbox"
                      type="checkbox"
                      checked={selectedItemIds.includes(item.id)}
                      onChange={() => toggleItemSelection(item.id)}
                      aria-label={`Select ${item.name}`}
                    />
                  )}
                  <ShelfItemIcon item={item} size={20} className="item-icon" />
                  <div className="item-copy">
                    <div className="item-title">
                      <strong>{item.name}</strong>
                    </div>
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
                          data-tooltip="Open item"
                          aria-label={`Open ${item.name}`}
                        >
                          <ExternalLink size={16} />
                        </button>
                        <button
                          className="icon-button small"
                          type="button"
                          onClick={() => void revealShelfPath(item.path)}
                          data-tooltip="Show in Finder"
                          aria-label={`Show ${item.name} in Finder`}
                        >
                          <FolderOpen size={16} />
                        </button>
                      </>
                    )}
                    <button
                      className="icon-button small"
                      type="button"
                      onClick={() => {
                        const peerId = linkedPeers.some((peer) => peer.id === selectedPeerId)
                          ? selectedPeerId
                          : linkedPeers[0]?.id;
                        if (peerId && item.kind !== "directory") void sendShelfItems(peerId, [item.id]);
                      }}
                      disabled={item.kind === "directory" || linkedPeers.length === 0 || isBusy}
                      data-tooltip="Send item"
                      aria-label={`Send ${item.name}`}
                    >
                      <Send size={16} />
                    </button>
                    <button
                      className="icon-button small"
                      type="button"
                      onClick={() => void removeItem(item.id)}
                      disabled={isBusy}
                      data-tooltip="Remove item"
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
          <span>
            {linkedPeers.length > 0
              ? `${linkedPeers.length} linked device${linkedPeers.length === 1 ? "" : "s"}`
              : peers.length > 0
                ? "Devices found — link one to send"
                : "Searching for devices"}
          </span>
        </footer>
      </section>
      ) : mainView === "devices" ? (
        <section className="workspace" aria-label="Devices workspace">
          <header className="toolbar">
            <div>
              <p className="eyebrow">LAN</p>
              <h1>Devices</h1>
            </div>
            <div className="toolbar-actions">
              <button
                className="secondary-button"
                type="button"
                data-tooltip={showManualPeerForm ? "Hide the manual add form" : "Add a device by IP address"}
                onClick={() => setShowManualPeerForm((visible) => !visible)}
              >
                <Laptop size={16} />
                {showManualPeerForm ? "Close add form" : "Add device"}
              </button>
              <button
                className="secondary-button"
                type="button"
                disabled={isScanningDevices}
                data-tooltip="Scan the local network for devices"
                onClick={() => void scanLanDevices()}
              >
                {isScanningDevices ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
                Scan now
              </button>
              <button
                className="secondary-button"
                type="button"
                disabled={isCheckingTransfer}
                data-tooltip="Check transfer listeners and connectivity"
                onClick={() => void runTransferSelfCheck()}
              >
                {isCheckingTransfer ? <Loader2 className="spin" size={16} /> : <ShieldCheck size={16} />}
                Run self-check
              </button>
            </div>
          </header>
          {showManualPeerForm && (
            <form
              className="manual-peer-form"
              onSubmit={(event) => {
                event.preventDefault();
                void addManualPeer();
              }}
            >
              <div className="manual-peer-heading">
                <strong>Add a device manually</strong>
                <span>Use an IP address or resolvable host name when automatic discovery is unavailable.</span>
              </div>
              <label>
                Address
                <input
                  value={manualAddress}
                  onChange={(event) => setManualAddress(event.target.value)}
                  placeholder="192.168.1.20"
                  autoComplete="off"
                  data-tooltip="IP address or host name of the other device"
                />
              </label>
              <label>
                Port
                <input
                  type="number"
                  min="1"
                  max="65535"
                  value={manualPort}
                  onChange={(event) => setManualPort(event.target.value)}
                  data-tooltip="TCP port the other device listens on"
                />
              </label>
              <label>
                Name <span className="optional-label">optional</span>
                <input
                  value={manualName}
                  onChange={(event) => setManualName(event.target.value)}
                  placeholder="Other computer"
                  autoComplete="off"
                  data-tooltip="Friendly name shown in the device list"
                />
              </label>
              <button
                className="secondary-button"
                type="submit"
                disabled={isAddingPeer}
                data-tooltip="Add this device to the list"
              >
                {isAddingPeer ? <Loader2 className="spin" size={16} /> : <Laptop size={16} />}
                Add device
              </button>
            </form>
          )}
          <div className="devices-list">
            {peers.length === 0 ? (
              <div className="empty-state">
                <Laptop size={34} />
                <strong>No devices found</strong>
                <span>
                  Start DropAir on another computer on the same network. Discovery runs in the
                  background. If receiving fails, allow DropAir through Windows Firewall on private
                  networks.
                </span>
              </div>
            ) : (
              peers.map((peer) => (
                <article
                  className={`device-row${selectedPeerId === peer.id ? " is-selected" : ""}`}
                  key={peer.id}
                  data-tooltip="Select this device"
                  onClick={() => setSelectedPeerId(peer.id)}
                >
                  <div className="item-icon" aria-hidden="true">
                    <Laptop size={20} />
                  </div>
                  <div className="item-copy">
                    <strong>{peer.name}</strong>
                    <span>
                      {peer.address.includes(":") && !peer.address.startsWith("[")
                        ? `[${peer.address}]`
                        : peer.address}
                      :{peer.port}
                      {peer.manual ? " · Manual" : ""}
                      {peer.addresses.length > 1 ? ` · ${peer.addresses.length} addresses` : ""}
                    </span>
                  </div>
                  <div className="device-actions">
                    <div
                      className={`peer-link-status${peer.linked ? " is-linked" : ""}`}
                      data-tooltip={peer.linked ? "This device is linked and available for sending" : "Link this device before sending"}
                    >
                      <span className="peer-status-dot" aria-hidden="true" />
                      <span>{peer.linked ? "Linked" : "Not linked"}</span>
                    </div>
                    <button
                      className="secondary-button"
                      type="button"
                      data-tooltip={peer.linked ? "Stop sending to this device" : "Allow sending to this device"}
                      onClick={(event) => {
                        event.stopPropagation();
                        void togglePeerLinked(peer);
                      }}
                    >
                      {peer.linked ? "Unlink" : "Link"}
                    </button>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={!peer.linked || testingPeerId === peer.id}
                      data-tooltip="Test the connection to this device"
                      onClick={(event) => {
                        event.stopPropagation();
                        void testPeerConnection(peer);
                      }}
                    >
                      {testingPeerId === peer.id ? <Loader2 className="spin" size={16} /> : <ShieldCheck size={16} />}
                      Test
                    </button>
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={!peer.linked || items.length === 0 || isBusy}
                      data-tooltip="Send every Shelf item to this device"
                      onClick={(event) => {
                        event.stopPropagation();
                        void sendShelfItems(peer.id);
                      }}
                    >
                      <Send size={16} />
                      Send items
                    </button>
                    {peer.manual && (
                      <button
                        className="icon-button small"
                        type="button"
                        data-tooltip="Remove manual device"
                        aria-label={`Remove ${peer.name}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          void removePeer(peer);
                        }}
                      >
                        <X size={16} />
                      </button>
                    )}
                  </div>
                </article>
              ))
            )}
          </div>
          <div className="transfer-status-box">
            {transferStatusInfo?.udpListenerUp &&
            transferStatusInfo.discoveryBroadcastUp &&
            transferStatusInfo.tcpListenerUp ? (
              <span>
                Discovery UDP {transferStatusInfo.discoveryPort} and receiving TCP {transferStatusInfo.transferPort} are listening
              </span>
            ) : (
              <span className="is-warning">
                One or more transfer listeners are not running. Check firewall permissions and restart DropAir.
              </span>
            )}
          </div>
          {transferSelfCheck && (
            <div className="transfer-checks" aria-live="polite">
              <div className="transfer-checks-header">
                <strong>Self-check results</strong>
                <span>{new Date(transferSelfCheck.checkedAt).toLocaleTimeString()}</span>
              </div>
              <div className="transfer-check-list">
                {transferSelfCheck.checks.map((check) => (
                  <div className={`transfer-check${check.ok ? " is-ok" : " is-failed"}`} key={check.id}>
                    {check.ok ? <CheckCircle2 size={16} /> : <X size={16} />}
                    <div>
                      <strong>{check.label}</strong>
                      <span>{check.detail}</span>
                    </div>
                  </div>
                ))}
              </div>
              {transferSelfCheck.diagnostics.lastError && (
                <p className="transfer-last-error">
                  Recent error: {transferSelfCheck.diagnostics.lastError}
                </p>
              )}
            </div>
          )}
          <div className="disclaimer-box" role="note">
            <strong>Transfer disclaimer</strong>
            <p>
              DropAir transfers are <strong>not encrypted and not authenticated</strong>. Files and
              text are sent in the clear over your local network, and any device that can reach the
              port can connect. There is no pairing or identity check.
            </p>
            <ul>
              <li>
                <strong>Fine for:</strong> your own devices on a trusted home or office LAN, moving
                non-sensitive files between them.
              </li>
              <li>
                <strong>Not for:</strong> public or untrusted Wi-Fi, shared or corporate networks,
                the open internet, or confidential, personal, or regulated data.
              </li>
            </ul>
            <p>
              The software is provided "as is", without warranty of any kind. You use it at your own
              risk. The author is not responsible for any data loss, leakage, corruption, or other
              damage caused by using DropAir.
            </p>
          </div>
        </section>
      ) : mainView === "sent" || mainView === "received" ? (
        <section className="workspace" aria-label={`${mainView} history`}>
          <header className="toolbar">
            <div>
              <p className="eyebrow">Transfer history</p>
              <h1>{mainView === "sent" ? "Sent" : "Received"}</h1>
            </div>
          </header>
          <div className="history-list">
            {(mainView === "sent" ? sentRecords : receivedRecords).length === 0 ? (
              <div className="empty-state">
                <FileArchive size={34} />
                <strong>No transfers yet</strong>
                <span>Completed transfers will appear here.</span>
              </div>
            ) : (
              (mainView === "sent" ? sentRecords : receivedRecords).map((record) => (
                <article className="history-row" key={record.id}>
                  <div className="item-icon" aria-hidden="true">
                    {mainView === "sent" ? <Send size={20} /> : <FolderOpen size={20} />}
                  </div>
                  <div className="item-copy">
                    <strong>{record.message}</strong>
                    <span>{new Date(record.at).toLocaleString()}</span>
                  </div>
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
            <div className="setting-row">
              <div className="setting-copy">
                <strong>Appearance</strong>
                <span>Follow the system theme, or force a light or dark interface.</span>
              </div>
              <div className="theme-control" role="group" aria-label="Appearance">
                {THEME_OPTIONS.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    className={`theme-option${themePreference === option.value ? " is-active" : ""}`}
                    aria-pressed={themePreference === option.value}
                    data-tooltip={option.tooltip}
                    onClick={() => updateTheme(option.value)}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            </div>

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
                    data-tooltip={shakeEnabled ? "Turn shake detection off" : "Turn shake detection on"}
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
                data-tooltip={launchAtLogin ? "Stop launching DropAir at login" : "Launch DropAir at login"}
                disabled={!settingsReady || isBusy}
                onClick={() => void toggleAutostart()}
              >
                <span />
              </button>
            </div>

            <div className="setting-row download-location-row">
              <div className="setting-copy">
                <strong>Device name</strong>
                <span>This name is shown to other DropAir devices on the network.</span>
              </div>
              <div className="download-location-control">
                <input
                  value={deviceName}
                  onChange={(event) => setDeviceName(event.target.value)}
                  aria-label="Device name"
                  placeholder="Computer name"
                  maxLength={80}
                  disabled={!settingsReady || isBusy}
                />
                <button
                  className="secondary-button"
                  type="button"
                  disabled={!settingsReady || isBusy || !deviceName.trim()}
                  data-tooltip="Save the device name"
                  onClick={() => void updateDeviceName()}
                >
                  Save
                </button>
              </div>
            </div>

            <div className="setting-row download-location-row">
              <div className="setting-copy">
                <strong>Download location</strong>
                <span>Received files are saved here. The default is a DropAir folder inside Downloads.</span>
              </div>
              <div className="download-location-control">
                <input
                  value={downloadDirectory}
                  onChange={(event) => setDownloadDirectory(event.target.value)}
                  aria-label="Download location"
                  placeholder="Default Downloads folder"
                  spellCheck={false}
                  disabled={!settingsReady || isBusy}
                />
                <button
                  className="secondary-button"
                  type="button"
                  disabled={!settingsReady || isBusy || !downloadDirectory.trim()}
                  data-tooltip="Save the download location"
                  onClick={() => void updateDownloadDirectory(downloadDirectory)}
                >
                  Save
                </button>
                <button
                  className="secondary-button"
                  type="button"
                  disabled={!settingsReady || isBusy}
                  data-tooltip="Reset to the default Downloads folder"
                  onClick={() => void updateDownloadDirectory("")}
                >
                  Use default
                </button>
              </div>
            </div>

            <div className="setting-row download-location-row">
              <div className="setting-copy">
                <strong>Updates</strong>
                <span>
                  {updateVersion
                    ? `Version ${updateVersion} is available. Installed version ${appVersion || "0.2.4"}.`
                    : "DropAir checks for updates on launch. Installed version " +
                      `${appVersion || "0.2.4"}.`}
                </span>
              </div>
              <div className="download-location-control">
                {updateVersion ? (
                  <button
                    className="primary-button"
                    type="button"
                    disabled={isBusy}
                    data-tooltip="Download the update and restart DropAir"
                    onClick={() => void installUpdate()}
                  >
                    <Download size={16} />
                    {updateProgress || "Install update"}
                  </button>
                ) : (
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={isBusy}
                    data-tooltip="Check for a newer DropAir release"
                    onClick={() => void checkForUpdates(true)}
                  >
                    {isBusy ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
                    Check for updates
                  </button>
                )}
              </div>
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
                  data-tooltip="Open macOS Accessibility settings"
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
      <TooltipBubble tooltip={tooltip} />
    </main>
  );
}

type FileTypePresentation = {
  Icon: LucideIcon;
  tone: string;
  badge: string | null;
};

const FILE_TYPE_GROUPS: Array<{
  tone: string;
  Icon: LucideIcon;
  extensions: readonly string[];
}> = [
  {
    tone: "image",
    Icon: FileImage,
    extensions: [
      "png", "jpg", "jpeg", "jfif", "gif", "webp", "bmp", "svg", "tif", "tiff",
      "heic", "heif", "avif", "ico", "raw", "cr2", "nef", "psd", "ai"
    ]
  },
  {
    tone: "video",
    Icon: FileVideoCamera,
    extensions: ["mp4", "mov", "mkv", "avi", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp"]
  },
  {
    tone: "audio",
    Icon: FileMusic,
    extensions: ["mp3", "wav", "flac", "aac", "ogg", "m4a", "opus", "wma", "aiff", "mid", "midi"]
  },
  {
    tone: "archive",
    Icon: FileArchive,
    extensions: ["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "lzma", "cab", "iso"]
  },
  {
    tone: "pdf",
    Icon: FileText,
    extensions: ["pdf"]
  },
  {
    tone: "document",
    Icon: FileText,
    extensions: ["doc", "docx", "odt", "rtf", "txt", "md", "markdown", "pages", "tex", "epub"]
  },
  {
    tone: "sheet",
    Icon: FileSpreadsheet,
    extensions: ["xls", "xlsx", "csv", "tsv", "ods", "numbers"]
  },
  {
    tone: "slides",
    Icon: Presentation,
    extensions: ["ppt", "pptx", "key", "odp"]
  },
  {
    tone: "code",
    Icon: FileCode,
    extensions: [
      "js", "jsx", "mjs", "cjs", "ts", "tsx", "html", "htm", "css", "scss", "sass", "less",
      "vue", "svelte", "rs", "py", "rb", "php", "go", "java", "kt", "kts", "swift", "c", "cc",
      "cpp", "cxx", "h", "hpp", "cs", "sh", "bash", "zsh", "fish", "ps1", "bat", "cmd", "sql",
      "lua", "pl", "r", "dart", "scala", "clj", "ex", "exs", "erl", "hs"
    ]
  },
  {
    tone: "data",
    Icon: FileBraces,
    extensions: ["json", "jsonc", "json5", "yaml", "yml", "toml", "xml", "ini", "cfg", "conf", "env", "properties"]
  },
  {
    tone: "font",
    Icon: FileType,
    extensions: ["ttf", "otf", "woff", "woff2", "eot"]
  },
  {
    tone: "key",
    Icon: FileKey,
    extensions: ["pem", "key", "crt", "cer", "p12", "pfx", "gpg", "asc"]
  },
  {
    tone: "app",
    Icon: FileCog,
    extensions: ["exe", "msi", "dmg", "pkg", "deb", "rpm", "appimage", "apk", "bin", "run", "app"]
  }
];

const FILE_TYPE_BY_EXTENSION = new Map<string, { tone: string; Icon: LucideIcon }>();
for (const group of FILE_TYPE_GROUPS) {
  for (const extension of group.extensions) {
    FILE_TYPE_BY_EXTENSION.set(extension, { tone: group.tone, Icon: group.Icon });
  }
}

function fileExtension(name: string) {
  const trimmed = name.trim();
  const dotIndex = trimmed.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === trimmed.length - 1) {
    return "";
  }
  return trimmed.slice(dotIndex + 1).toLowerCase();
}

function describeShelfItem(item: ShelfItem): FileTypePresentation {
  if (item.kind === "directory") {
    return { Icon: Folder, tone: "folder", badge: null };
  }

  if (item.kind === "text") {
    return { Icon: FileText, tone: "document", badge: "TXT" };
  }

  const extension = fileExtension(item.name);
  const badge = extension ? extension.slice(0, 4).toUpperCase() : null;
  const match = FILE_TYPE_BY_EXTENSION.get(extension);

  if (match) {
    return { Icon: match.Icon, tone: match.tone, badge };
  }

  return { Icon: File, tone: "generic", badge };
}

function ShelfItemIcon({
  item,
  size,
  className
}: {
  item: ShelfItem;
  size: number;
  className: string;
}) {
  const { Icon, tone, badge } = describeShelfItem(item);
  return (
    <span className={`${className} shelf-type-icon tone-${tone}`} aria-hidden="true">
      <Icon size={size} />
      {badge && <span className="shelf-type-tag">{badge}</span>}
    </span>
  );
}

function TooltipBubble({ tooltip }: { tooltip: TooltipState }) {
  const ref = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || !tooltip) return;
    const rect = element.getBoundingClientRect();
    const arrowX = Math.min(Math.max(tooltip.anchorX - rect.left, 10), Math.max(10, rect.width - 10));
    element.style.setProperty("--arrow-x", `${arrowX}px`);
  }, [tooltip]);

  if (!tooltip) return null;
  return (
    <div
      ref={ref}
      className={`app-tooltip${tooltip.placement === "above" ? " is-above" : ""}`}
      style={{ left: tooltip.x, top: tooltip.y }}
      role="tooltip"
    >
      {tooltip.text}
    </div>
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
