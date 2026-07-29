import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  CheckCircle2,
  FileArchive,
  FileText,
  Folder,
  Laptop,
  Loader2,
  PanelTopOpen,
  Send,
  Trash2,
  X
} from "lucide-react";
import { DragEvent, useEffect, useMemo, useState } from "react";

const isShelfWindow = new URLSearchParams(window.location.search).has("shelf");

type ShelfItemKind = "file" | "directory" | "other";

type ShelfItem = {
  id: number;
  path: string;
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

function App() {
  const [items, setItems] = useState<ShelfItem[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [status, setStatus] = useState("Ready");
  const [isBusy, setIsBusy] = useState(false);
  const [shakeStatus, setShakeStatus] = useState("starting");
  const [shakeDiagnostics, setShakeDiagnostics] = useState<ShakeDiagnostics | null>(null);

  const totalSize = useMemo(
    () => items.reduce((sum, item) => sum + (item.size ?? 0), 0),
    [items]
  );

  useEffect(() => {
    void refreshShelf();
  }, []);

  useEffect(() => {
    const refreshDiagnostics = () => {
      void invoke<ShakeDiagnostics>("shake_monitor_diagnostics")
        .then(setShakeDiagnostics)
        .catch(() => undefined);
    };
    refreshDiagnostics();
    const timer = window.setInterval(refreshDiagnostics, 500);
    return () => window.clearInterval(timer);
  }, []);

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
      if (isShelfWindow) {
        await getCurrentWindow().hide();
      }
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

  async function testShakeShelf() {
    try {
      await invoke("show_shake_shelf_for_test");
      setStatus("Test Shelf opened");
    } catch (error) {
      setStatus(toErrorMessage(error));
    }
  }

  async function hideShakeShelf() {
    await getCurrentWindow().hide();
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

    void addPaths(paths);
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
          <span>DropAir Shelf</span>
          <div className="shake-shelf-actions">
            <span>{items.length} queued</span>
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
            <strong>Drop files here</strong>
          </div>
        ) : (
          <div className="shake-shelf-items">
            {items.slice(0, 2).map((item) => (
              <div className="shake-shelf-item" key={item.id}>
                {item.kind === "directory" ? <Folder size={16} /> : <FileText size={16} />}
                <span>{item.name}</span>
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
          <button className="nav-item is-active" type="button">
            <FileArchive size={18} />
            Shelf
          </button>
          <button className="nav-item" type="button" disabled>
            <Laptop size={18} />
            Devices
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

      <section className="workspace" aria-label="Shelf workspace">
        <header className="toolbar">
          <div>
            <p className="eyebrow">Temporary shelf</p>
            <h1>{items.length} item{items.length === 1 ? "" : "s"}</h1>
          </div>
          <div className="toolbar-actions">
            <button
              className="icon-button"
              type="button"
              onClick={() => void testShakeShelf()}
              title="Test Shake Shelf"
              aria-label="Test Shake Shelf"
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
              disabled={items.length === 0}
              title="Send to device"
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
              <strong>Drop files or folders here</strong>
              <span>Drag while holding the mouse, then shake left and right to reveal Shelf.</span>
            </div>
          ) : (
            <div className="item-list">
              {items.map((item) => (
                <article className="shelf-item" key={item.id}>
                  <div className="item-icon" aria-hidden="true">
                    {item.kind === "directory" ? <Folder size={20} /> : <FileText size={20} />}
                  </div>
                  <div className="item-copy">
                    <strong>{item.name}</strong>
                    <span>{item.path}</span>
                  </div>
                  <div className="item-meta">
                    <span>{formatSize(item.size)}</span>
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
          <span>Direct transfer pending</span>
        </footer>
      </section>
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
  if (status === "listening") return "Shake monitor: Listening";
  if (status === "permissionRequired") return "Shake monitor: Permission required";
  if (status === "unsupported") return "Shake monitor: macOS only";
  return "Shake monitor: Starting";
}

export default App;
