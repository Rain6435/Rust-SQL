//! Embedded single-page HTML/JS frontend served at `/` by the `--web` server.

/// Single-page application HTML/JS bundle communicating with `/api/ipc`.
pub const RUST_SQL_WEB_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>Rust-SQL</title>
  <style>
    * { box-sizing: border-box; font-family: 'Lato', 'Helvetica Neue', Arial, sans-serif; }
    body { margin: 0; background: #f8fafc; color: #0f172a; display: flex; flex-direction: column; height: 100vh; }
    body.dark-theme { background: #0f172a; color: #f8fafc; }
    header { height: 50px; background: #1e293b; color: #fff; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; }
    footer { height: 32px; background: #e2e8f0; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; font-size: 12px; }
    #main-area { flex: 1; display: flex; overflow: hidden; }
    #server-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 16px; padding: 16px; width: 100%; align-content: start; }
    .server-card { background: #fff; border: 1px solid #cbd5e1; border-radius: 6px; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
    #sidebar { width: 250px; border-right: 1px solid #cbd5e1; padding: 12px; overflow-y: auto; background: #fff; }
    #workspace { flex: 1; display: flex; flex-direction: column; padding: 12px; gap: 12px; overflow: hidden; }
    textarea.query-editor { width: 100%; height: 150px; font-family: monospace; font-size: 13px; padding: 8px; border: 1px solid #cbd5e1; border-radius: 4px; }
    .grid-query-wrapper { flex: 1; overflow: auto; border: 1px solid #cbd5e1; background: #fff; }
    table { width: 100%; border-collapse: collapse; font-size: 13px; }
    th, td { border: 1px solid #e2e8f0; padding: 6px 10px; text-align: left; }
    th { background: #f1f5f9; font-weight: bold; }
    button { cursor: pointer; padding: 6px 12px; border-radius: 4px; border: 1px solid #94a3b8; background: #f8fafc; }
    button.primary { background: #16a34a; color: white; border-color: #15803d; }
    .emulated-banner { background: #b91c1c; color: #fff; font-weight: bold; padding: 6px 12px; }
  </style>
</head>
<body>
  <header>
    <div><strong>Rust-SQL</strong></div>
    <div id="breadcrumb">servers</div>
    <div id="conn-actions"></div>
  </header>
  <div id="emulated-banner" class="emulated-banner" data-testid="emulated-banner" hidden></div>
  <div id="main-area">
    <div id="server-list"></div>
  </div>
  <footer>
    <span id="status-bar">Ready</span>
    <span>Rust-SQL v1.39.0</span>
  </footer>
  <script>
    async function ipc(event, ...args) {
      const res = await fetch('/api/ipc', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ event, args })
      });
      const json = await res.json();
      if (!json.ok) throw new Error(json.error || 'IPC Error');
      return json.data;
    }

    window.rustSql = { ipc };
    let emulatedBanner = null;

    function setConnectionMode(info) {
      emulatedBanner = info && info.mode === 'emulated' ? info.banner : null;
      const el = document.getElementById('emulated-banner');
      el.textContent = emulatedBanner || '';
      el.hidden = !emulatedBanner;
    }

    async function loadServers() {
      const cfg = await ipc('CONFIG_GET_FULL', true);
      const container = document.getElementById('main-area');
      container.innerHTML = '<div id="server-list"></div>';
      const list = document.getElementById('server-list');
      for (const srv of cfg.servers) {
        const meta = srv.host ? `${srv.host}:${srv.port || ''}` : (srv.socketPath || srv.database || '');
        const card = document.createElement('div');
        card.className = 'server-card';
        card.innerHTML = `
          <div data-testid="server-name" style="font-weight:bold">${srv.name}</div>
          <div data-testid="server-meta" style="color:#64748b;font-size:12px">${meta}</div>
          <button class="primary" data-testid="connect-button">Connect</button>
        `;
        card.querySelector('[data-testid="connect-button"]').onclick = () => connectServer(srv, cfg.crypto.secret);
        list.appendChild(card);
      }
    }

    async function connectServer(srv, secret) {
      const dec = await ipc('SERVERS_DECRYPT_SECRETS', srv, secret);
      const db = dec.database || ':memory:';
      setConnectionMode(null);
      setConnectionMode(await ipc('DB_CONNECT', dec, db));
      const tables = await ipc('DB_LIST_TABLES', db);
      const container = document.getElementById('main-area');
      container.innerHTML = `
        <div id="sidebar">
          <div data-testid="db-header" style="font-weight:bold;margin-bottom:8px">${db}</div>
          <div id="table-items"></div>
        </div>
        <div id="workspace" class="react-tabs__tab-panel react-tabs__tab-panel--selected">
          <div id="querybox1"><div contenteditable="true" id="editor-box" style="border:1px solid #ccc;padding:8px;min-height:80px;font-family:monospace"></div></div>
          <div><button class="primary" id="exec-btn">Execute</button></div>
          <div id="query-result"></div>
        </div>
      `;
      const tblContainer = document.getElementById('table-items');
      for (const t of tables) {
        const item = document.createElement('div');
        item.setAttribute('data-testid', 'db-item-Table');
        item.style.cursor = 'pointer';
        item.style.padding = '4px 0';
        item.innerHTML = `<span>${t.name}</span>`;
        item.querySelector('span').onclick = async () => {
          const sql = await ipc('DB_GET_QUERY_SELECT_TOP', db, t.name);
          document.getElementById('editor-box').innerText = sql;
          await runQuery(db, sql);
        };
        tblContainer.appendChild(item);
      }
      document.getElementById('exec-btn').onclick = () => {
        runQuery(db, document.getElementById('editor-box').innerText);
      };
    }

    async function runQuery(db, sql) {
      const results = await ipc('DB_EXECUTE_QUERY', db, sql);
      const resDiv = document.getElementById('query-result');
      resDiv.innerHTML = '';
      if (emulatedBanner) {
        const banner = document.createElement('div');
        banner.className = 'emulated-banner';
        banner.setAttribute('data-testid', 'result-emulated-banner');
        banner.textContent = emulatedBanner;
        resDiv.appendChild(banner);
      }
      for (const r of results) {
        const wrap = document.createElement('div');
        wrap.className = 'grid-query-wrapper';
        for (const f of r.fields) {
          const cell = document.createElement('div');
          cell.className = 'ReactVirtualized__Grid__cell';
          cell.style.fontWeight = 'bold';
          cell.innerText = f.name;
          wrap.appendChild(cell);
        }
        for (const row of r.rows) {
          for (const f of r.fields) {
            const cell = document.createElement('div');
            cell.className = 'ReactVirtualized__Grid__cell';
            cell.innerText = row[f.name] === null ? 'NULL' : String(row[f.name]);
            wrap.appendChild(cell);
          }
        }
        resDiv.appendChild(wrap);
      }
    }

    loadServers();
  </script>
</body>
</html>
"#;
