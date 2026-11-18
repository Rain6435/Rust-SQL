//! Embedded single-page HTML/JS frontend served at `/` by the `--web` server.
//!
//! The page signs in through `/api/login`, keeps the CSRF token in memory only, and sends it
//! as `X-CSRF-Token` on every IPC call. It connects by saved connection ID and never sees a
//! password. Query parameters are sent as a JSON object, so numbers, booleans, and null keep
//! their types; the server binds them through prepared statements. Values from the server or
//! the database are inserted as text, never as HTML. Autocomplete and import live in
//! [`crate::web::assets_editor`], loaded from `/assets/editor.js`.

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
    <div id="conn-actions"><span id="whoami"></span> <button id="logout-btn" hidden>Sign out</button></div>
  </header>
  <div id="emulated-banner" class="emulated-banner" data-testid="emulated-banner" hidden></div>
  <div id="main-area">
    <div id="server-list"></div>
  </div>
  <footer>
    <span id="status-bar">Ready</span>
    <span id="timeout-label" data-testid="statement-timeout"></span>
    <span>Rust-SQL v1.39.0</span>
  </footer>
  <script src="/assets/editor.js"></script>
  <script>
    let csrfToken = null;

    async function post(path, body) {
      const headers = { 'Content-Type': 'application/json' };
      if (csrfToken) headers['X-CSRF-Token'] = csrfToken;
      const res = await fetch(path, {
        method: 'POST', credentials: 'same-origin', headers, body: JSON.stringify(body)
      });
      if (res.status === 401 && path !== '/api/login') { showLogin('Your session ended. Sign in again.'); }
      return res.json();
    }

    async function ipc(event, ...args) {
      const json = await post('/api/ipc', { event, args });
      if (!json.ok) {
        const err = new Error(json.error || 'IPC Error');
        err.hostKeyChallenge = json.hostKeyChallenge || null;
        throw err;
      }
      return json.data;
    }

    window.rustSql = { ipc, post };
    let emulatedBanner = null;

    function setConnectionMode(info) {
      const secs = info && info.statementTimeoutSecs;
      document.getElementById('timeout-label').textContent = info ? (secs ? `Timeout: ${secs}s` : 'Timeout: none') : '';
      emulatedBanner = info && info.mode === 'emulated' ? info.banner : null;
      const el = document.getElementById('emulated-banner');
      el.textContent = emulatedBanner || '';
      el.hidden = !emulatedBanner;
    }

    function el(tag, attrs, text) {
      const e = document.createElement(tag);
      for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, v);
      if (text !== undefined) e.textContent = text;
      return e;
    }

    function setSession(data) {
      csrfToken = data ? data.csrfToken : null;
      document.getElementById('whoami').textContent = data ? data.username : '';
      document.getElementById('logout-btn').hidden = !data;
    }

    function showLogin(message) {
      setSession(null);
      const container = document.getElementById('main-area');
      container.replaceChildren();
      const form = el('form', { id: 'login-form', style: 'margin:48px auto;display:flex;flex-direction:column;gap:8px;width:280px' });
      form.append(el('strong', {}, 'Sign in to Rust-SQL'), el('div', { id: 'login-error', style: 'color:#b91c1c' }, message || ''),
        el('input', { id: 'login-user', autocomplete: 'username', placeholder: 'Username' }),
        el('input', { id: 'login-pass', type: 'password', autocomplete: 'current-password', placeholder: 'Password' }),
        el('button', { class: 'primary', type: 'submit' }, 'Sign in'));
      form.onsubmit = async (ev) => {
        ev.preventDefault();
        const json = await post('/api/login', {
          username: document.getElementById('login-user').value,
          password: document.getElementById('login-pass').value
        });
        if (!json.ok) { document.getElementById('login-error').textContent = json.error; return; }
        setSession(json.data);
        loadServers();
      };
      container.appendChild(form);
    }

    document.getElementById('logout-btn').onclick = async () => {
      await post('/api/logout', {});
      showLogin('Signed out.');
    };

    async function loadServers() {
      const servers = await ipc('SERVERS_GET_ALL');
      const container = document.getElementById('main-area');
      container.replaceChildren(el('div', { id: 'server-list' }));
      const list = document.getElementById('server-list');
      if (!servers.length) list.appendChild(el('div', {}, 'No saved connections have been shared with you.'));
      for (const srv of servers) {
        const meta = srv.host ? `${srv.host}:${srv.port || ''}` : (srv.socketPath || srv.database || '');
        const card = el('div', { class: 'server-card' });
        const btn = el('button', { class: 'primary', 'data-testid': 'connect-button' }, 'Connect');
        card.append(el('div', { 'data-testid': 'server-name', style: 'font-weight:bold' }, srv.name),
          el('div', { 'data-testid': 'server-meta', style: 'color:#64748b;font-size:12px' }, meta), btn);
        btn.onclick = () => connectServer(srv);
        list.appendChild(card);
      }
    }

    // An unknown or 180-day-old SSH bastion key must be approved by the user before connecting.
    async function connectWithHostKeyCheck(srv, db) {
      for (let attempt = 0; attempt < 2; attempt++) {
        try {
          return await ipc('DB_CONNECT', srv.id, db);
        } catch (err) {
          const ch = err.hostKeyChallenge;
          if (!ch || attempt > 0) throw err;
          const ok = window.confirm(err.message + '\n\nFingerprint: ' + ch.fingerprintSha256 +
            '\n\nTrust this key and connect?');
          if (!ok) throw err;
          await ipc('SSH_HOST_KEY_APPROVE', ch);
        }
      }
    }

    async function connectServer(srv) {
      const db = srv.database || ':memory:';
      setConnectionMode(null);
      setConnectionMode(await connectWithHostKeyCheck(srv, db));
      const tables = await ipc('DB_LIST_TABLES', db);
      const container = document.getElementById('main-area');
      container.innerHTML = `
        <div id="sidebar">
          <div data-testid="db-header" style="font-weight:bold;margin-bottom:8px"></div>
          <div id="table-items"></div>
        </div>
        <div id="workspace" class="react-tabs__tab-panel react-tabs__tab-panel--selected">
          <div id="querybox1"><textarea id="editor-box" class="query-editor" data-testid="query-editor" spellcheck="false"></textarea></div>
          <div><input id="params-input" data-testid="query-params" style="width:60%" placeholder='Parameters as JSON, for example {"id": 5, "name": "x"}' /> <button class="primary" id="exec-btn">Execute</button></div>
          <div id="query-result"></div>
        </div>
      `;
      container.querySelector('[data-testid="db-header"]').textContent = db;
      const tblContainer = document.getElementById('table-items');
      for (const t of tables) {
        const item = document.createElement('div');
        item.setAttribute('data-testid', 'db-item-Table');
        item.style.cursor = 'pointer';
        item.style.padding = '4px 0';
        item.appendChild(el('span', {}, t.name));
        item.querySelector('span').onclick = async () => {
          const sql = await ipc('DB_GET_QUERY_SELECT_TOP', db, t.name);
          document.getElementById('editor-box').value = sql;
          await runQuery(db, sql);
        };
        tblContainer.appendChild(item);
      }
      document.getElementById('exec-btn').onclick = () => {
        const raw = document.getElementById('params-input').value.trim();
        let params = null;
        try {
          params = raw ? JSON.parse(raw) : null;
        } catch (e) {
          document.getElementById('query-result').textContent = 'Parameters must be a JSON object: ' + e.message;
          return;
        }
        runQuery(db, document.getElementById('editor-box').value, params);
      };
      window.rustSqlEditor.attach(db, tables);
    }

    function appendRows(wrap, r) {
      for (const row of r.rows) {
        for (const f of r.fields) {
          const cell = el('div', { class: 'ReactVirtualized__Grid__cell' });
          cell.innerText = row[f.name] === null ? 'NULL' : String(row[f.name]);
          wrap.appendChild(cell);
        }
      }
    }

    // A single SELECT comes back one page at a time; "Load more" fetches the next page.
    async function runQuery(db, sql, params) {
      const data = await ipc('DB_EXECUTE_PAGED', db, sql, params || null, null);
      const resDiv = document.getElementById('query-result');
      resDiv.innerHTML = '';
      if (emulatedBanner) {
        resDiv.appendChild(el('div', { class: 'emulated-banner', 'data-testid': 'result-emulated-banner' }, emulatedBanner));
      }
      for (const r of data.results) {
        const wrap = el('div', { class: 'grid-query-wrapper' });
        for (const f of r.fields) {
          const cell = el('div', { class: 'ReactVirtualized__Grid__cell', style: 'font-weight:bold' });
          cell.innerText = f.name;
          wrap.appendChild(cell);
        }
        appendRows(wrap, r);
        resDiv.appendChild(wrap);
      }
      if (!data.page) return;
      const wrap = resDiv.querySelector('.grid-query-wrapper');
      const info = el('div', { 'data-testid': 'page-info' });
      const more = el('button', { 'data-testid': 'load-more' }, 'Load more');
      let next = data.page.next, loaded = data.results[0].rows.length;
      const show = () => {
        info.textContent = `Rows loaded: ${loaded} (page size ${data.page.cursor.pageSize})` + (next ? '' : ', all rows loaded');
        more.hidden = !next;
      };
      more.onclick = async () => {
        const page = await ipc('DB_EXECUTE_PAGED', db, sql, params || null, next);
        appendRows(wrap, page.results[0]);
        loaded += page.results[0].rows.length;
        next = page.page && page.page.next;
        show();
      };
      show();
      resDiv.append(info, more);
    }

    fetch('/api/session', { credentials: 'same-origin' }).then(r => r.json()).then(json => {
      if (json.ok) { setSession(json.data); loadServers(); } else { showLogin(''); }
    });
  </script>
</body>
</html>
"#;
