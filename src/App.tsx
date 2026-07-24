function App() {
  return (
    <main className="app-shell">
      <section className="smoke-panel" aria-label="DropAir smoke test">
        <div className="mark" aria-hidden="true">
          DA
        </div>
        <p className="eyebrow">macOS cloud build validation</p>
        <h1>DropAir Build Smoke Test</h1>
        <p className="summary">
          If this window opens on macOS, GitHub Actions successfully produced a
          runnable Tauri application without requiring a local Mac build
          environment.
        </p>
        <dl className="facts">
          <div>
            <dt>App</dt>
            <dd>DropAir</dd>
          </div>
          <div>
            <dt>Version</dt>
            <dd>0.1.0</dd>
          </div>
          <div>
            <dt>Build Target</dt>
            <dd>macOS .app.zip</dd>
          </div>
        </dl>
      </section>
    </main>
  );
}

export default App;
