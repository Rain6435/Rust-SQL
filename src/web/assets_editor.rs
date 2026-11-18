//! Web UI script for SQL autocomplete and file import, served at `/assets/editor.js`.
//!
//! Autocomplete calls `DB_COMPLETE` as you type (debounced) or on Ctrl+Space; Up/Down pick a
//! suggestion, Tab or a click inserts it, Esc hides the list. Cursor positions are sent as
//! Unicode code-point indexes, which is how the server counts characters.
//!
//! Import reads the chosen file in the browser, previews it through `DB_IMPORT_PREVIEW`, lets
//! the user map each source column to a table column (or skip it), and runs `DB_IMPORT_RUN`
//! through `/api/ipc/import`. Large imports need the confirmation checkbox. All values are
//! shown as text, never as HTML, and every call carries the CSRF token.

/// The editor script.
pub const RUST_SQL_WEB_EDITOR_JS: &str = r#"(function () {
  const MAX_IMPORT_BYTES = 16 * 1024 * 1024;

  async function call(path, event, ...args) {
    const json = await window.rustSql.post(path, { event, args });
    if (!json.ok) throw new Error(json.error || 'IPC Error');
    return json.data;
  }

  function el(tag, attrs, text) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, v);
    if (text !== undefined) e.textContent = text;
    return e;
  }

  const codePoints = (s) => Array.from(s).length;

  function attachCompletion(db, editor) {
    const box = el('div', { 'data-testid': 'completion-list', role: 'listbox',
      style: 'border:1px solid #94a3b8;background:#fff;color:#0f172a;max-height:160px;overflow-y:auto;font-family:monospace;font-size:13px' });
    box.hidden = true;
    editor.after(box);
    let items = [], sel = 0, current = null, timer = null, seq = 0;

    const hide = () => { items = []; box.hidden = true; };
    function render() {
      box.replaceChildren();
      items.forEach((item, i) => {
        const row = el('div', { role: 'option', 'data-testid': 'completion-item',
          style: 'padding:2px 8px;cursor:pointer;' + (i === sel ? 'background:#fde68a' : '') });
        row.append(el('span', {}, item.label), el('span', { style: 'color:#64748b;margin-left:12px' }, item.kind));
        row.onmousedown = (ev) => { ev.preventDefault(); accept(i); };
        box.appendChild(row);
      });
      box.hidden = items.length === 0;
    }
    async function request(explicit) {
      const text = editor.value;
      const cursor = codePoints(text.slice(0, editor.selectionStart));
      const mine = ++seq;
      let data;
      try { data = await call('/api/ipc', 'DB_COMPLETE', db, text, cursor, explicit); } catch (e) { return; }
      if (mine !== seq || editor.value !== text) return;
      current = data; items = data.items; sel = 0; render();
    }
    function accept(i) {
      const item = items[i];
      if (!item || !current) return;
      const chars = Array.from(editor.value);
      const end = current.start + codePoints(current.prefix);
      const head = chars.slice(0, current.start).join('') + item.label;
      editor.value = head + chars.slice(end).join('');
      editor.setSelectionRange(head.length, head.length);
      hide();
      editor.focus();
    }
    editor.addEventListener('keydown', (ev) => {
      if (ev.ctrlKey && (ev.code === 'Space' || ev.key === ' ')) { ev.preventDefault(); request(true); return; }
      if (box.hidden) return;
      if (ev.key === 'ArrowDown') { sel = Math.min(sel + 1, items.length - 1); render(); }
      else if (ev.key === 'ArrowUp') { sel = Math.max(sel - 1, 0); render(); }
      else if (ev.key === 'Tab') accept(sel);
      else if (ev.key === 'Escape') hide();
      else return;
      ev.preventDefault();
    });
    editor.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(() => request(false), 150); });
    editor.addEventListener('blur', () => setTimeout(hide, 150));
  }

  function attachImport(db, tables) {
    const open = el('button', { 'data-testid': 'import-button' }, 'Import…');
    document.getElementById('exec-btn').after(' ', open);
    const panel = el('div', { 'data-testid': 'import-panel', style: 'border:1px solid #cbd5e1;padding:8px;background:#fff;color:#0f172a;display:flex;flex-direction:column;gap:6px;max-height:50vh;overflow:auto' });
    panel.hidden = true;
    document.getElementById('query-result').before(panel);
    const file = el('input', { type: 'file', accept: '.csv,.tsv,.txt,.json,.jsonl,.ndjson', 'data-testid': 'import-file' });
    const table = el('select', { 'data-testid': 'import-table' });
    tables.forEach((t, i) => table.appendChild(el('option', { value: String(i) }, t.schema ? `${t.schema}.${t.name}` : t.name)));
    const previewBtn = el('button', {}, 'Preview');
    const msg = el('div', { 'data-testid': 'import-message' });
    const area = el('div');
    panel.append(el('strong', {}, `Import into ${db}`),
      el('div', {}, 'File (.csv, .tsv, .json, .jsonl): '), file,
      el('div', {}, 'Target table: '), table, previewBtn, msg, area);
    open.onclick = () => { panel.hidden = !panel.hidden; };
    let upload = null;
    const say = (text, ok) => { msg.textContent = text; msg.style.color = ok ? '#15803d' : '#b91c1c'; };

    async function readFile() {
      const f = file.files[0];
      if (!f) throw new Error('Choose a file first');
      if (f.size > MAX_IMPORT_BYTES) throw new Error('Files over 16 MiB cannot be imported from the web UI; use --import on the server');
      return { name: f.name, text: await f.text() };
    }
    previewBtn.onclick = async () => {
      area.replaceChildren(); say('', true);
      const t = tables[Number(table.value)];
      try {
        upload = await readFile();
        const p = await call('/api/ipc/import', 'DB_IMPORT_PREVIEW', db, t.name, t.schema || null, upload.name, upload.text);
        showPreview(t, p);
      } catch (e) { say(e.message, false); }
    };

    function showPreview(t, p) {
      area.append(el('div', {}, `${p.rowCount} rows. Preview of the first ${p.preview.length}; pick a target column for each source column:`));
      const grid = el('table'), head = el('tr'), selects = [];
      p.columns.forEach((c, i) => {
        const th = el('th', {}, c), s = el('select', { 'data-testid': 'import-map' });
        s.appendChild(el('option', { value: '' }, '(skip)'));
        for (const tc of p.targetColumns) {
          const o = el('option', { value: tc }, tc);
          if (p.mapping[i] === tc) o.selected = true;
          s.appendChild(o);
        }
        th.append(el('br'), s); head.appendChild(th); selects.push(s);
      });
      grid.appendChild(head);
      for (const row of p.preview) {
        const tr = el('tr');
        for (const v of row) tr.appendChild(el('td', {}, v === null ? 'NULL' : String(v)));
        grid.appendChild(tr);
      }
      const confirm = el('input', { type: 'checkbox', 'data-testid': 'import-confirm' });
      const label = el('label', {}, ` I confirm importing all ${p.rowCount} rows`);
      label.prepend(confirm);
      label.hidden = !p.requiresConfirmation;
      const run = el('button', { class: 'primary', 'data-testid': 'import-run' }, 'Import');
      const ready = () => { run.disabled = p.requiresConfirmation && !confirm.checked; };
      confirm.onchange = ready; ready();
      run.onclick = async () => {
        run.disabled = true;
        const mapping = selects.map((s) => s.value || null);
        try {
          const r = await call('/api/ipc/import', 'DB_IMPORT_RUN', db, t.name, t.schema || null, upload.name, upload.text, mapping, confirm.checked);
          area.replaceChildren();
          say(`Imported ${r.rowsInserted} rows into ${t.name}.`, true);
        } catch (e) { say(e.message, false); ready(); }
      };
      area.append(grid, label, run);
    }
  }

  window.rustSqlEditor = {
    attach(db, tables) {
      attachCompletion(db, document.getElementById('editor-box'));
      attachImport(db, tables);
    }
  };
})();
"#;
