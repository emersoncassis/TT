'use strict';
/* TT web UI. No build step, no framework. DOM is built with h() (never innerHTML with data). */

const $app = document.getElementById('app');
const state = { me: null, profiles: [], hosts: [], tabs: [], active: null, brand: {}, filter: '' };

function h(tag, props, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, '');
    else if (v !== false && v != null) el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) if (kid != null && kid !== false) el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  return el;
}

async function api(path, opts = {}) {
  const o = { credentials: 'same-origin', ...opts };
  if (o.body && typeof o.body === 'object' && !(o.body instanceof Blob) && !(o.body instanceof ArrayBuffer)) {
    o.headers = { 'Content-Type': 'application/json', ...(o.headers || {}) };
    o.body = JSON.stringify(o.body);
  }
  const r = await fetch(path, o);
  const ct = r.headers.get('content-type') || '';
  const data = ct.includes('json') ? await r.json() : null;
  if (!r.ok) throw Object.assign(new Error((data && data.error) || r.statusText), { status: r.status });
  return data;
}

function modal(title, content, { wide } = {}) {
  const close = () => ov.remove();
  const ov = h('div', { class: 'overlay', onmousedown: e => { if (e.target === ov) close(); } },
    h('div', { class: 'modal', style: wide ? 'width:min(760px,100%)' : '' }, h('h2', {}, title), content));
  document.body.append(ov);
  return { close, el: ov };
}

function field(label, input) { return h('div', {}, h('label', {}, label), input); }

/* ---------- boot ---------- */
async function boot() {
  try { state.brand = await api('/api/branding'); } catch { state.brand = { name: 'TT' }; }
  applyBrand();
  const st = await api('/api/status');
  if (st.needs_setup) return renderAuth('setup');
  try { state.me = await api('/api/me'); } catch { return renderAuth('login'); }
  await loadAll();
  renderShell();
}

function applyBrand() {
  const b = state.brand;
  if (b.accent) document.documentElement.style.setProperty('--accent', b.accent);
  document.title = `${b.name || 'TT'} — ${b.tagline || 'Terminal Tools'}`;
}

function brandEl() {
  const b = state.brand;
  return h('div', { class: 'brand' }, h('img', { src: b.logo ? '/branding/logo' : '/assets/logo-ic-mark.png', alt: '' }), h('span', {}, b.name || 'TT'), b.tagline === '' ? null : h('small', {}, b.tagline || 'Terminal Tools'));
}

function renderAuth(mode) {
  const err = h('div', { class: 'err' });
  const user = h('input', { autocomplete: 'username', autofocus: true });
  const pass = h('input', { type: 'password', autocomplete: mode === 'setup' ? 'new-password' : 'current-password' });
  const pass2 = h('input', { type: 'password', autocomplete: 'new-password' });
  const submit = async e => {
    e.preventDefault(); err.textContent = '';
    if (mode === 'setup' && pass.value !== pass2.value) { err.textContent = 'As senhas não conferem.'; return; }
    try {
      await api(mode === 'setup' ? '/api/setup' : '/api/login', { method: 'POST', body: { username: user.value, password: pass.value } });
      state.me = await api('/api/me'); await loadAll(); renderShell();
    } catch (ex) { err.textContent = ex.message; }
  };
  $app.replaceChildren(h('div', { class: 'center' }, h('form', { class: 'card', onsubmit: submit },
    brandEl(),
    h('h1', { style: 'margin-top:18px' }, mode === 'setup' ? 'Criar administrador' : 'Entrar'),
    h('div', { class: 'muted' }, mode === 'setup' ? 'Primeiro acesso: esta conta será a administradora e poderá criar os demais usuários.' : 'Acesse com seu usuário e senha.'),
    field('Usuário', user), field(mode === 'setup' ? 'Senha (mín. 10 caracteres)' : 'Senha', pass),
    mode === 'setup' ? field('Repita a senha', pass2) : null,
    err, h('div', { style: 'margin-top:14px' }, h('button', { class: 'primary', style: 'width:100%' }, mode === 'setup' ? 'Criar e entrar' : 'Entrar')))));
}

async function loadAll() {
  [state.profiles, state.hosts] = await Promise.all([api('/api/profiles'), api('/api/hosts')]);
}
const profileOf = id => state.profiles.find(p => p.id === id) || state.profiles.find(p => p.id === 'generic');

/* ---------- shell ---------- */
let $hosts, $tabs, $panes;
function renderShell() {
  $hosts = h('div', { class: 'hosts' });
  $tabs = h('div', { class: 'tabs' });
  $panes = h('div', { class: 'panes' });
  const search = h('input', { placeholder: 'Buscar host…', oninput: e => { state.filter = e.target.value.toLowerCase(); renderHosts(); } });
  const localMode = false;
  $app.replaceChildren(h('div', { class: 'shell' },
    h('div', { class: 'topbar' }, brandEl(),
      h('div', { class: 'row' },
        h('span', { class: 'muted' }, state.me.username, state.me.role === 'admin' ? ' · administrador' : ''),
        h('button', { onclick: tunnelsDialog }, 'Túneis'),
        h('button', { onclick: aiDialog }, 'IA e MCP'),
        state.me.role === 'admin' ? h('button', { onclick: usersDialog }, 'Usuários') : null,
        h('button', { onclick: passwordDialog }, 'Minha senha'),
        h('button', { onclick: async () => { await api('/api/logout', { method: 'POST' }); location.reload(); } }, 'Sair'))),
    h('div', { class: 'body' },
      h('div', { class: 'side' }, h('div', { class: 'tools' }, h('button', { class: 'primary', onclick: () => hostDialog() }, '+ Novo host'), search), $hosts),
      h('div', { class: 'main' }, $tabs, $panes))));
  renderHosts(); renderEmpty();
}

function renderEmpty() {
  if (state.tabs.length === 0) $panes.replaceChildren(h('div', { class: 'empty' }, h('div', {}, h('h2', {}, 'Nenhuma sessão aberta'), h('div', {}, 'Cadastre um host e clique nele para abrir o terminal.'))));
}

function renderHosts() {
  const groups = new Map();
  for (const x of state.hosts) {
    if (state.filter && !(`${x.name} ${x.host} ${x.folder}`.toLowerCase().includes(state.filter))) continue;
    const f = x.folder || 'Sem pasta';
    if (!groups.has(f)) groups.set(f, []);
    groups.get(f).push(x);
  }
  if (!groups.size) { $hosts.replaceChildren(h('div', { class: 'muted', style: 'padding:14px' }, 'Nenhum host.')); return; }
  const out = [];
  for (const [folder, list] of groups) {
    out.push(h('div', { class: 'folder' }, folder));
    for (const x of list) out.push(h('div', { class: 'host', ondblclick: () => openSession(x), onclick: () => openSession(x) },
      h('div', { class: 'grow' }, h('div', { class: 'name' }, x.name), h('div', { class: 'sub' }, `${x.username}@${x.host}:${x.port}`)),
      h('span', { class: 'badge' }, profileOf(x.profile_id).name.split(' ')[0]),
      h('button', { class: 'ghost edit', title: 'Editar', onclick: e => { e.stopPropagation(); hostDialog(x); } }, '✎')));
  }
  $hosts.replaceChildren(...out);
}

/* ---------- host dialog ---------- */
function hostDialog(x) {
  const isNew = !x;
  x = x || { name: '', folder: '', host: '', port: 22, username: '', auth_kind: 'password', profile_id: 'generic', legacy: false, notes: '' };
  const f = {
    name: h('input', { value: x.name }), folder: h('input', { value: x.folder, placeholder: 'ex.: Clientes/Loja 01' }),
    host: h('input', { value: x.host, placeholder: 'IP ou nome' }), port: h('input', { type: 'number', value: x.port }),
    username: h('input', { value: x.username, autocomplete: 'off' }),
    auth_kind: h('select', {}, h('option', { value: 'password' }, 'Senha'), h('option', { value: 'key' }, 'Chave privada')),
    secret: h('input', { type: 'password', autocomplete: 'new-password', placeholder: x.has_secret ? '(mantida — digite para trocar)' : '' }),
    key: h('textarea', { rows: 5, placeholder: x.has_secret ? '(mantida — cole outra para trocar)' : '-----BEGIN OPENSSH PRIVATE KEY-----', style: 'font-family:var(--mono);font-size:11px' }),
    passphrase: h('input', { type: 'password', autocomplete: 'new-password', placeholder: x.has_passphrase ? '(mantida)' : 'opcional' }),
    enable: h('input', { type: 'password', autocomplete: 'new-password', placeholder: x.has_enable ? '(mantida)' : 'senha do modo enable (opcional)' }),
    profile: h('select', {}, state.profiles.map(p => h('option', { value: p.id }, p.name))),
    legacy: h('input', { type: 'checkbox', style: 'width:auto' }),
    notes: h('textarea', { rows: 2 }, x.notes),
  };
  f.auth_kind.value = x.auth_kind; f.profile.value = x.profile_id; f.legacy.checked = x.legacy;
  const info = h('div'); const err = h('div', { class: 'err' });
  const secretBox = h('div'), enableBox = h('div'), legacyBox = h('div');
  const refresh = () => {
    const p = profileOf(f.profile.value);
    secretBox.replaceChildren(...(f.auth_kind.value === 'key' ? [field('Chave privada (PEM/OpenSSH)', f.key), field('Senha da chave', f.passphrase)] : [field('Senha', f.secret)]));
    enableBox.replaceChildren(...(p.enable_support ? [field('Senha enable (Cisco)', f.enable)] : []));
    legacyBox.replaceChildren(h('label', { style: 'display:flex;gap:8px;align-items:center;color:var(--text)' }, f.legacy, 'Algoritmos legados (equipamento antigo; menos seguro)'));
    info.replaceChildren(h('div', { class: 'note' }, p.description, p.notes.length ? h('ul', {}, p.notes.map(n => h('li', {}, n))) : null));
  };
  f.profile.addEventListener('change', () => { const p = profileOf(f.profile.value); if (isNew || f.port.value == profileOf(x.profile_id).default_port) f.port.value = p.default_port; f.legacy.checked = f.legacy.checked || p.legacy_hint && false; refresh(); });
  f.auth_kind.addEventListener('change', refresh);
  refresh();
  const save = async () => {
    err.textContent = '';
    const body = { name: f.name.value, folder: f.folder.value, host: f.host.value, port: +f.port.value, username: f.username.value,
      auth_kind: f.auth_kind.value, profile_id: f.profile.value, legacy: f.legacy.checked, notes: f.notes.value,
      secret: f.auth_kind.value === 'key' ? f.key.value : f.secret.value, passphrase: f.passphrase.value, enable_secret: f.enable.value };
    if (isNew && !body.secret) { err.textContent = 'Informe a senha ou a chave.'; return; }
    try {
      await api(isNew ? '/api/hosts' : `/api/hosts/${x.id}`, { method: isNew ? 'POST' : 'PUT', body });
      m.close(); state.hosts = await api('/api/hosts'); renderHosts();
    } catch (e) { err.textContent = e.message; }
  };
  const del = async () => {
    if (!confirm(`Excluir o host "${x.name}"?`)) return;
    await api(`/api/hosts/${x.id}`, { method: 'DELETE' }); m.close(); state.hosts = await api('/api/hosts'); renderHosts();
  };
  const forget = async () => { await api(`/api/known-hosts?host=${encodeURIComponent(x.host)}&port=${x.port}`, { method: 'DELETE' }); alert('Chave do host esquecida. Será pedida novamente na próxima conexão.'); };
  const m = modal(isNew ? 'Novo host' : 'Editar host', h('div', {},
    h('div', { class: 'grid2' }, field('Nome', f.name), field('Pasta', f.folder)),
    field('Tipo de equipamento', f.profile), info,
    h('div', { class: 'grid2' }, field('Host / IP', f.host), field('Porta', f.port)),
    h('div', { class: 'grid2' }, field('Usuário', f.username), field('Autenticação', f.auth_kind)),
    secretBox, enableBox, legacyBox, field('Notas', f.notes), err,
    h('div', { class: 'row', style: 'margin-top:14px' },
      h('button', { class: 'primary', onclick: save }, 'Salvar'), h('button', { onclick: () => m.close() }, 'Cancelar'),
      h('span', { class: 'grow' }),
      isNew ? null : [h('button', { onclick: forget, title: 'Esquecer a chave SSH confiada' }, 'Esquecer chave'), h('button', { class: 'danger', onclick: del }, 'Excluir')])), { wide: true });
}

/* ---------- sessions ---------- */
function openSession(x) {
  const p = profileOf(x.profile_id);
  const tab = { id: crypto.randomUUID(), host: x, profile: p };
  state.tabs.push(tab);
  tab.pane = h('div', { class: 'pane' });
  tab.label = h('span', {}, x.name);
  tab.tabEl = h('div', { class: 'tab', onclick: () => activate(tab) }, tab.label, h('span', { class: 'x', onclick: e => { e.stopPropagation(); closeTab(tab); } }, '✕'));
  $tabs.append(tab.tabEl);
  if (state.tabs.length === 1) $panes.replaceChildren();
  $panes.append(tab.pane);
  (p.mode === 'netconf' ? buildNetconf : buildTerminal)(tab);
  activate(tab);
}

function activate(tab) {
  state.active = tab;
  for (const t of state.tabs) { t.tabEl.classList.toggle('active', t === tab); t.pane.classList.toggle('active', t === tab); }
  if (tab.fit) requestAnimationFrame(() => { try { tab.fit.fit(); } catch {} tab.term && tab.term.focus(); });
}

function closeTab(tab) {
  if (tab.ws) try { tab.ws.close(); } catch {}
  if (tab.term) tab.term.dispose();
  tab.tabEl.remove(); tab.pane.remove();
  state.tabs = state.tabs.filter(t => t !== tab);
  if (state.tabs.length) activate(state.tabs[state.tabs.length - 1]); else { state.active = null; renderEmpty(); }
}

function wsUrl(path) { return `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}${path}`; }

function hostKeyDialog(tab, m) {
  const ws = tab.ws;
  const dlg = modal(m.changed ? '⚠ A chave do host MUDOU' : 'Confiar nesta chave de host?', h('div', {},
    m.changed ? h('div', { class: 'warn' }, 'A chave SSH deste host é diferente da que você confiou antes. Pode ser reinstalação/atualização do equipamento — ou um ataque man-in-the-middle. Só continue se você souber o motivo.') : h('div', { class: 'muted' }, 'Primeira conexão com este host. Compare com a fingerprint do equipamento (ex.: MikroTik: /ip ssh print).'),
    h('div', { class: 'fp' }, `${m.host}:${m.port}\n${m.key_type}\n${m.fingerprint}`),
    m.changed ? h('div', { class: 'fp' }, `Anterior:\n${m.previous}`) : null,
    h('div', { class: 'row' },
      h('button', { class: m.changed ? 'danger' : 'primary', onclick: () => { ws.send(JSON.stringify({ t: 'trust' })); dlg.close(); } }, m.changed ? 'Aceitar nova chave' : 'Confiar e conectar'),
      h('button', { onclick: () => { ws.send(JSON.stringify({ t: 'reject' })); dlg.close(); } }, 'Recusar'))));
}

function buildTerminal(tab) {
  const p = tab.profile;
  const term = new Terminal({ fontFamily: getComputedStyle(document.documentElement).getPropertyValue('--mono'), fontSize: 14, cursorBlink: true, scrollback: 10000, theme: { background: '#000000', foreground: '#f3ecdc', cursor: '#c8b077', selectionBackground: '#3a352b' } });
  const fit = new FitAddon.FitAddon(); term.loadAddon(fit);
  tab.term = term; tab.fit = fit;
  const box = h('div', { class: 'termbox' });
  const drawer = h('div', { class: 'drawer' });
  const btn = (label, which) => h('button', { onclick: () => toggleDrawer(which) }, label);
  const tools = h('div', { class: 'paneltools' }, btn('Assistente', 'ai'), p.quick_commands.length ? btn('Comandos', 'cmds') : null, p.sftp === 'none' ? null : btn('Arquivos', 'files'), h('button', { onclick: () => { const n = state.hosts.find(x => x.id === tab.host.id); openSession(n || tab.host); } }, 'Duplicar'));
  tab.pane.append(h('div', { class: 'termwrap' }, box), drawer, tools);
  let drawerKind = null;
  const showDrawer = kind => {
    drawerKind = kind; drawer.classList.add('open');
    drawer.replaceChildren(kind === 'cmds' ? commandsPanel(tab) : kind === 'ai' ? aiPanel(tab) : filesPanel(tab));
    setTimeout(() => fit.fit(), 50);
  };
  const toggleDrawer = kind => {
    if (drawerKind === kind) { drawer.classList.remove('open'); drawerKind = null; setTimeout(() => fit.fit(), 50); }
    else showDrawer(kind);
  };
  tab.showDrawer = showDrawer;
  term.open(box); fit.fit();
  const ws = new WebSocket(wsUrl(`/api/term/${tab.host.id}`)); ws.binaryType = 'arraybuffer'; tab.ws = ws;
  const enc = new TextEncoder();
  ws.onopen = () => ws.send(JSON.stringify({ t: 'resize', cols: term.cols, rows: term.rows }));
  ws.onmessage = ev => {
    if (typeof ev.data !== 'string') { term.write(new Uint8Array(ev.data)); return; }
    const m = JSON.parse(ev.data);
    if (m.t === 'hostkey') hostKeyDialog(tab, m);
    else if (m.t === 'status') term.writeln(`\x1b[90m${m.msg}\x1b[0m`);
    else if (m.t === 'error') term.writeln(`\r\n\x1b[31m${m.msg}\x1b[0m`);
    else if (m.t === 'ready') { tab.label.textContent = tab.host.name; fit.fit(); term.focus(); }
    else if (m.t === 'closed') term.writeln('\r\n\x1b[90m[sessão encerrada]\x1b[0m');
    else if (m.t === 'suggestion') { aiState(tab); tab.ai.suggestions.push(m); showDrawer('ai'); }
  };
  ws.onclose = () => { tab.label.textContent = tab.host.name + ' ⏹'; };
  term.onData(d => { if (ws.readyState === 1) ws.send(enc.encode(d)); });
  term.onResize(({ cols, rows }) => { if (ws.readyState === 1) ws.send(JSON.stringify({ t: 'resize', cols, rows })); });
  new ResizeObserver(() => { if (state.active === tab) try { fit.fit(); } catch {} }).observe(box);
  tab.sendText = (text, run) => { if (ws.readyState === 1) ws.send(enc.encode(run ? text + '\n' : text)); term.focus(); };
}

/* ---------- AI assistant (suggests only, never executes by itself) ---------- */
function aiState(tab) { if (!tab.ai) tab.ai = { messages: [], suggestions: [], log: null, sendScreen: true }; return tab.ai; }

function termScreen(term, n = 80) {
  const b = term.buffer.active, out = [];
  for (let i = Math.max(0, b.length - n); i < b.length; i++) { const l = b.getLine(i); if (l) out.push(l.translateToString(true)); }
  return out.join('\n').replace(/\s+$/, '');
}

function cmdCard(tab, command, readonly, note) {
  const run = () => { if (!readonly && !confirm(`Executar este comando no equipamento?\n\n${command}`)) return; tab.sendText(command, true); };
  return h('div', { class: 'cmdcard' }, note ? h('div', { class: 'dpad muted' }, note) : null, h('pre', {}, command),
    h('div', { class: 'acts' }, h('button', { onclick: () => tab.sendText(command, false) }, 'Colar'),
      h('button', { class: readonly ? 'primary' : '', onclick: run }, readonly ? '▶ Executar (leitura)' : '▶ Executar…'),
      readonly ? null : h('span', { class: 'warn', style: 'font-size:12px;align-self:center' }, 'pode alterar o equipamento')));
}

function aiPanel(tab) {
  const st = aiState(tab);
  const log = h('div', { class: 'log' }); st.log = log;
  const input = h('textarea', { rows: 3, placeholder: 'Pergunte algo sobre o que está na tela…' });
  const screenChk = h('input', { type: 'checkbox', checked: st.sendScreen, onchange: () => { st.sendScreen = screenChk.checked; } });
  const draw = () => {
    log.replaceChildren(...(st.messages.length || st.suggestions.length ? [] : [h('div', { class: 'muted' }, 'O assistente lê o que está na sua tela (com senhas e chaves mascaradas) e sugere comandos e boas práticas. Ele nunca executa nada sozinho.')]));
    for (const m of st.messages) {
      const el = h('div', { class: 'msg ' + m.role }, m.content.split(/```[\s\S]*?```/g).join('').trim() || (m.role === 'user' ? m.content : ''));
      for (const c of (m.commands || [])) el.append(cmdCard(tab, c.command, c.readonly));
      log.append(el);
    }
    st.suggestions.forEach((sg, i) => { const el = h('div', { class: 'msg' }, h('div', { class: 'muted' }, `Sugerido via ${sg.source}${sg.reason ? ': ' + sg.reason : ''}`), cmdCard(tab, sg.command, sg.readonly), h('div', { class: 'acts', style: 'padding:6px 0 0' }, h('button', { class: 'ghost', onclick: () => { st.suggestions.splice(i, 1); draw(); } }, 'Descartar'))); log.append(el); });
    log.scrollTop = log.scrollHeight;
  };
  const send = async () => {
    const text = input.value.trim(); if (!text) return;
    input.value = ''; st.messages.push({ role: 'user', content: text }); draw();
    const wait = h('div', { class: 'msg muted' }, 'Pensando…'); log.append(wait);
    try {
      const r = await api('/api/ai/chat', { method: 'POST', body: { host_id: tab.host.id, messages: st.messages.map(m => ({ role: m.role, content: m.content })), screen: st.sendScreen ? termScreen(tab.term) : '' } });
      st.messages.push({ role: 'assistant', content: r.reply, commands: r.commands });
    } catch (e) { st.messages.push({ role: 'assistant', content: 'Erro: ' + e.message }); }
    draw();
  };
  input.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } });
  draw();
  return h('div', { class: 'chat' }, h('h3', {}, 'Assistente de IA'), log,
    h('div', { class: 'input' }, h('label', { class: 'chk' }, screenChk, 'Enviar a tela ao assistente (segredos mascarados)'), input,
      h('div', { class: 'row' }, h('button', { class: 'primary', onclick: send }, 'Enviar'), h('button', { class: 'ghost', onclick: () => { st.messages = []; draw(); } }, 'Limpar'), h('button', { class: 'ghost', onclick: aiDialog }, 'Configurar'))));
}

/* ---------- quick commands ---------- */
function commandsPanel(tab) {
  const groups = new Map();
  for (const q of tab.profile.quick_commands) { if (!groups.has(q.group)) groups.set(q.group, []); groups.get(q.group).push(q); }
  const out = [h('h3', {}, 'Comandos — ' + tab.profile.name),
    h('div', { class: 'dpad muted' }, 'Clique no nome para colar no terminal; ▶ executa. Itens em vermelho pedem confirmação.')];
  for (const [g, list] of groups) {
    out.push(h('div', { class: 'folder' }, g));
    for (const q of list) out.push(h('div', { class: 'qc' },
      h('div', { class: 'lbl', onclick: () => fillAndSend(tab, q, false) }, q.dangerous ? h('span', { class: 'warn' }, q.label) : q.label, h('code', {}, q.command)),
      h('button', { title: 'Executar', onclick: () => fillAndSend(tab, q, true) }, '▶')));
  }
  return h('div', {}, out);
}

function fillAndSend(tab, q, run) {
  const names = [...new Set([...q.command.matchAll(/\{\{(\w+)\}\}/g)].map(m => m[1]))];
  const go = vals => {
    const cmd = q.command.replace(/\{\{(\w+)\}\}/g, (_, n) => vals[n]);
    if (run && q.dangerous && !confirm(`Executar comando sensível?\n\n${cmd}`)) return;
    tab.sendText(cmd, run);
  };
  if (!names.length) return go({});
  const inputs = Object.fromEntries(names.map(n => [n, h('input', { autofocus: n === names[0] })]));
  const m = modal(q.label, h('form', { onsubmit: e => { e.preventDefault(); const v = Object.fromEntries(names.map(n => [n, inputs[n].value])); m.close(); go(v); } },
    names.map(n => field(n, inputs[n])), h('div', { style: 'margin-top:14px' }, h('button', { class: 'primary' }, run ? 'Executar' : 'Colar'))));
}

/* ---------- files (SFTP) ---------- */
function filesPanel(tab) {
  const id = tab.host.id; let cwd = '.';
  const list = h('div', { class: 'files dpad' }), pathEl = h('div', { class: 'dpad muted', style: 'word-break:break-all' });
  const fmt = n => n > 1e9 ? (n / 1e9).toFixed(1) + ' GB' : n > 1e6 ? (n / 1e6).toFixed(1) + ' MB' : n > 1e3 ? (n / 1e3).toFixed(1) + ' kB' : n + ' B';
  const join = (a, b) => a.replace(/\/$/, '') + '/' + b;
  async function load(p) {
    list.replaceChildren(h('div', { class: 'muted' }, 'Carregando…'));
    try {
      const r = await api(`/api/sftp/${id}/list?path=${encodeURIComponent(p)}`);
      cwd = r.path; pathEl.textContent = cwd;
      const rows = [];
      if (cwd !== '/') rows.push(h('div', { class: 'item', onclick: () => load(cwd.replace(/\/[^/]*$/, '') || '/') }, '📁 ..'));
      for (const e of r.entries) {
        const full = join(cwd, e.name);
        rows.push(h('div', { class: 'item', onclick: () => { if (e.is_dir) load(full); else location.href = `/api/sftp/${id}/download?path=${encodeURIComponent(full)}`; } },
          h('span', {}, e.is_dir ? '📁' : '📄'), h('span', { class: 'grow', style: 'overflow:hidden;text-overflow:ellipsis' }, e.name), h('span', { class: 'size' }, e.is_dir ? '' : fmt(e.size)),
          h('button', { class: 'ghost', title: 'Renomear', onclick: async ev => { ev.stopPropagation(); const n = prompt('Novo nome', e.name); if (n && n !== e.name) { await act(() => api(`/api/sftp/${id}/rename`, { method: 'POST', body: { from: full, to: join(cwd, n) } })); } } }, '✎'),
          h('button', { class: 'ghost danger', title: 'Excluir', onclick: async ev => { ev.stopPropagation(); if (confirm(`Excluir ${e.name}?`)) await act(() => api(`/api/sftp/${id}/delete`, { method: 'POST', body: { path: full, is_dir: e.is_dir } })); } }, '🗑')));
      }
      list.replaceChildren(...rows);
    } catch (e) { list.replaceChildren(h('div', { class: 'err' }, e.message)); }
  }
  async function act(fn) { try { await fn(); } catch (e) { alert(e.message); } load(cwd); }
  const picker = h('input', { type: 'file', multiple: true, style: 'display:none', onchange: async () => {
    for (const f of picker.files) await act(() => api(`/api/sftp/${id}/upload?path=${encodeURIComponent(join(cwd, f.name))}`, { method: 'PUT', body: f }));
    picker.value = '';
  } });
  load('.');
  return h('div', {}, h('h3', {}, 'Arquivos (SFTP)'), pathEl,
    h('div', { class: 'dpad row' }, h('button', { onclick: () => picker.click() }, '⬆ Enviar'), h('button', { onclick: () => { const n = prompt('Nome da nova pasta'); if (n) act(() => api(`/api/sftp/${id}/mkdir`, { method: 'POST', body: { path: join(cwd, n) } })); } }, '+ Pasta'), h('button', { onclick: () => load(cwd) }, '⟳'), picker),
    list);
}

/* ---------- NETCONF console ---------- */
const NC_TEMPLATES = {
  'get-config (running)': '<get-config><source><running/></source></get-config>',
  'get (estado + config)': '<get/>',
  'lock (running)': '<lock><target><running/></target></lock>',
  'unlock (running)': '<unlock><target><running/></target></unlock>',
  'edit-config (modelo)': '<edit-config><target><candidate/></target><config><!-- seu XML aqui --></config></edit-config>',
  'commit': '<commit/>',
  'discard-changes': '<discard-changes/>',
  'close-session': '<close-session/>',
};
function buildNetconf(tab) {
  const out = h('pre', { class: 'xml' }, 'Conectando…');
  const body = h('textarea', { rows: 7, style: 'font-family:var(--mono);font-size:12px' }, NC_TEMPLATES['get-config (running)']);
  const sel = h('select', { onchange: () => { body.value = NC_TEMPLATES[sel.value]; } }, Object.keys(NC_TEMPLATES).map(k => h('option', {}, k)));
  const log = (kind, xml) => { out.textContent += `\n\n── ${kind} ──\n${pretty(xml)}`; out.scrollTop = out.scrollHeight; };
  const ws = new WebSocket(wsUrl(`/api/netconf/${tab.host.id}`)); tab.ws = ws;
  ws.onmessage = ev => {
    const m = JSON.parse(ev.data);
    if (m.t === 'hostkey') hostKeyDialog(tab, m);
    else if (m.t === 'hello') { out.textContent = `Conectado (framing ${m.framing}).`; log('hello do servidor', m.xml); }
    else if (m.t === 'reply') log('resposta', m.xml);
    else if (m.t === 'error') log('erro', m.msg);
    else if (m.t === 'closed') log('sessão', 'encerrada');
  };
  tab.pane.append(h('div', { class: 'nc' },
    h('div', {}, h('div', { class: 'row' }, h('div', { class: 'grow' }, field('Modelo de operação', sel))),
      field('Corpo da operação (o TT envolve em <rpc> com message-id)', body),
      h('div', { class: 'row', style: 'margin-top:8px' }, h('button', { class: 'primary', onclick: () => { log('enviado', body.value); ws.send(JSON.stringify({ t: 'op', body: body.value })); } }, 'Enviar'), h('button', { onclick: () => { out.textContent = ''; } }, 'Limpar'))),
    out));
}
function pretty(xml) {
  try {
    const d = new DOMParser().parseFromString(xml, 'application/xml');
    if (d.querySelector('parsererror')) return xml;
    const walk = (n, ind) => {
      if (n.nodeType === 3) { const t = n.textContent.trim(); return t ? ind + t + '\n' : ''; }
      if (n.nodeType !== 1) return '';
      const attrs = [...n.attributes].map(a => ` ${a.name}="${a.value}"`).join('');
      const kids = [...n.childNodes].filter(c => c.nodeType === 1 || (c.nodeType === 3 && c.textContent.trim()));
      if (!kids.length) return `${ind}<${n.nodeName}${attrs}/>\n`;
      if (kids.length === 1 && kids[0].nodeType === 3) return `${ind}<${n.nodeName}${attrs}>${kids[0].textContent.trim()}</${n.nodeName}>\n`;
      return `${ind}<${n.nodeName}${attrs}>\n${kids.map(c => walk(c, ind + '  ')).join('')}${ind}</${n.nodeName}>\n`;
    };
    return walk(d.documentElement, '');
  } catch { return xml; }
}

/* ---------- tunnels ---------- */
async function tunnelsDialog() {
  const body = h('div');
  const render = async () => {
    const [list, hosts] = await Promise.all([api('/api/tunnels'), api('/api/hosts')]);
    const hostName = id => (hosts.find(x => x.id === id) || {}).name || '?';
    const f = { host: h('select', {}, hosts.map(x => h('option', { value: x.id }, x.name))), name: h('input', { placeholder: 'ex.: Painel da câmera' }),
      kind: h('select', {}, h('option', { value: 'local' }, 'Local: porta → destino'), h('option', { value: 'dynamic' }, 'Dinâmico: proxy SOCKS5')),
      lport: h('input', { type: 'number', placeholder: 'porta local (≥1024)' }), dhost: h('input', { placeholder: 'destino, ex.: 192.168.1.10' }), dport: h('input', { type: 'number', placeholder: 'porta destino' }) };
    const err = h('div', { class: 'err' });
    const destBox = h('div', { class: 'grid2' }, field('Destino (visto pelo host SSH)', f.dhost), field('Porta destino', f.dport));
    const sync = () => { destBox.style.display = f.kind.value === 'dynamic' ? 'none' : ''; }; f.kind.addEventListener('change', sync); sync();
    const act = async (path, method = 'POST') => { try { await api(path, { method }); } catch (e) { alert(e.message); } render(); };
    body.replaceChildren(
      h('div', { class: 'note' }, 'O túnel escuta no computador onde o TT está rodando (127.0.0.1), não no navegador. Para iniciar, abra um terminal do host uma vez para confiar na chave.'),
      list.length ? h('table', {}, list.map(t => h('tr', {}, h('td', {}, h('div', {}, t.name), h('div', { class: 'muted', style: 'font-size:12px' }, `${hostName(t.host_id)} · ${t.kind === 'dynamic' ? 'SOCKS5' : `${t.dest_host}:${t.dest_port}`} · ${t.listen_host}:${t.listen_port}`)),
        h('td', {}, h('span', { class: 'pill' + (t.running ? ' run' : '') }, t.running ? 'ativo' : 'parado')),
        h('td', { style: 'text-align:right;white-space:nowrap' }, t.running ? h('button', { onclick: () => act(`/api/tunnels/${t.id}/stop`) }, 'Parar') : h('button', { class: 'primary', onclick: () => act(`/api/tunnels/${t.id}/start`) }, 'Iniciar'), ' ', h('button', { class: 'danger', onclick: () => { if (confirm('Excluir túnel?')) act(`/api/tunnels/${t.id}`, 'DELETE'); } }, 'Excluir'))))) : h('div', { class: 'muted', style: 'padding:12px 0' }, 'Nenhum túnel.'),
      h('h3', { style: 'margin:16px 0 0' }, 'Novo túnel'), h('div', { class: 'grid2' }, field('Host SSH', f.host), field('Nome', f.name)),
      h('div', { class: 'grid2' }, field('Tipo', f.kind), field('Porta local', f.lport)), destBox, err,
      h('div', { style: 'margin-top:10px' }, h('button', { class: 'primary', onclick: async () => { err.textContent = ''; try { await api('/api/tunnels', { method: 'POST', body: { host_id: +f.host.value, name: f.name.value, kind: f.kind.value, listen_port: +f.lport.value, dest_host: f.dhost.value, dest_port: +f.dport.value || 0 } }); render(); } catch (e) { err.textContent = e.message; } } }, 'Criar')));
  };
  await render();
  modal('Túneis SSH', body, { wide: true });
}

/* ---------- AI + MCP settings ---------- */
async function aiDialog() {
  const cfg = await api('/api/ai/settings');
  const f = { provider: h('select', {}, h('option', { value: 'anthropic' }, 'Anthropic (Claude)'), h('option', { value: 'openai' }, 'OpenAI ou compatível')), base: h('input', { value: cfg.base_url, placeholder: 'padrão do provedor' }),
    model: h('input', { value: cfg.model, placeholder: 'ex.: claude-sonnet-5-5' }), key: h('input', { type: 'password', autocomplete: 'new-password', placeholder: cfg.has_key ? '(chave salva — digite para trocar)' : 'sua chave de API' }),
    screen: h('input', { type: 'checkbox', checked: cfg.send_screen, style: 'width:auto' }) };
  f.provider.value = cfg.provider;
  const err = h('div', { class: 'err' }), tokBox = h('div'), tokList = h('div');
  const loadTokens = async () => {
    const t = await api('/api/tokens');
    tokList.replaceChildren(t.length ? h('table', {}, t.map(x => h('tr', {}, h('td', {}, x.name), h('td', { class: 'muted' }, x.scope === 'exec' ? 'leitura + comandos seguros' : 'só leitura'), h('td', { style: 'text-align:right' }, h('button', { class: 'danger', onclick: async () => { await api(`/api/tokens/${x.id}`, { method: 'DELETE' }); loadTokens(); } }, 'Revogar'))))) : h('div', { class: 'muted' }, 'Nenhum token.'));
  };
  const tname = h('input', { placeholder: 'nome (ex.: meu assistente)' }), tscope = h('select', {}, h('option', { value: 'read' }, 'Só leitura e sugestões'), h('option', { value: 'exec' }, 'Também executar comandos de leitura'));
  const m = modal('IA e MCP', h('div', {},
    h('h3', { style: 'margin:0' }, 'Assistente de IA'), h('div', { class: 'note' }, 'Usa a sua própria chave de API. A chave fica criptografada no servidor e nunca volta ao navegador. O assistente só sugere comandos; quem executa é você.'),
    h('div', { class: 'grid2' }, field('Provedor', f.provider), field('Modelo', f.model)),
    cfg.can_set_endpoint ? field('Endereço da API (opcional, só administradores)', f.base) : null, field('Chave de API', f.key),
    h('label', { class: 'chk', style: 'margin-top:10px' }, f.screen, 'Enviar a tela do terminal por padrão (senhas e chaves são mascaradas antes)'), err,
    h('div', { style: 'margin:10px 0 20px' }, h('button', { class: 'primary', onclick: async () => { err.textContent = ''; try { await api('/api/ai/settings', { method: 'PUT', body: { provider: f.provider.value, base_url: f.base.value, model: f.model.value, api_key: f.key.value, send_screen: f.screen.checked } }); err.textContent = ''; f.key.value = ''; f.key.placeholder = '(chave salva)'; err.className = 'okc'; err.textContent = 'Salvo.'; } catch (e) { err.className = 'err'; err.textContent = e.message; } } }, 'Salvar')),
    h('h3', { style: 'margin:0' }, 'Servidor MCP'), h('div', { class: 'note' }, 'Deixe outro assistente de IA usar o TT: ler a tela, sugerir comandos (você aprova) e, com token de execução, rodar só comandos de leitura permitidos para cada tipo de equipamento.',
      h('div', {}, 'HTTP: ', h('code', {}, `${location.origin}/mcp`), ' com cabeçalho Authorization: Bearer <token>.'), h('div', {}, 'Cliente que abre processo: ', h('code', {}, `tt mcp --url ${location.origin} --token <token>`))),
    tokList, tokBox, h('div', { class: 'grid2' }, field('Novo token', tname), field('Permissão', tscope)),
    h('div', { style: 'margin-top:10px' }, h('button', { onclick: async () => { try { const r = await api('/api/tokens', { method: 'POST', body: { name: tname.value, scope: tscope.value } }); tokBox.replaceChildren(h('div', { class: 'warn' }, 'Copie agora: o token não será mostrado de novo.'), h('code', { class: 'tok' }, r.token)); tname.value = ''; loadTokens(); } catch (e) { alert(e.message); } } }, 'Criar token'))), { wide: true });
  loadTokens();
}

/* ---------- admin ---------- */
async function usersDialog() {
  const body = h('div');
  const render = async () => {
    const users = await api('/api/users');
    const un = h('input', { placeholder: 'novo usuário' }), pw = h('input', { type: 'password', placeholder: 'senha (mín. 10)', autocomplete: 'new-password' });
    const role = h('select', {}, h('option', { value: 'user' }, 'Usuário'), h('option', { value: 'admin' }, 'Administrador'));
    const err = h('div', { class: 'err' });
    body.replaceChildren(
      h('table', {}, users.map(u => h('tr', {}, h('td', {}, u.username), h('td', { class: 'muted' }, u.role),
        h('td', { style: 'text-align:right' },
          h('button', { onclick: async () => { const p = prompt(`Nova senha para ${u.username} (mín. 10)`); if (p) { try { await api(`/api/users/${u.id}/password`, { method: 'POST', body: { password: p } }); alert('Senha alterada; sessões do usuário foram encerradas.'); } catch (e) { alert(e.message); } } } }, 'Senha'),
          ' ', u.id === state.me.id ? null : h('button', { class: 'danger', onclick: async () => { if (confirm(`Excluir ${u.username} e todos os hosts dele?`)) { try { await api(`/api/users/${u.id}`, { method: 'DELETE' }); render(); } catch (e) { alert(e.message); } } } }, 'Excluir'))))),
      h('h3', {}, 'Criar usuário'), h('div', { class: 'grid2' }, field('Usuário', un), field('Senha', pw)), field('Perfil', role), err,
      h('div', { style: 'margin-top:10px' }, h('button', { class: 'primary', onclick: async () => { err.textContent = ''; try { await api('/api/users', { method: 'POST', body: { username: un.value, password: pw.value, role: role.value } }); render(); } catch (e) { err.textContent = e.message; } } }, 'Criar')));
  };
  await render();
  modal('Usuários', body);
}

function passwordDialog() {
  const cur = h('input', { type: 'password', autocomplete: 'current-password' }), n1 = h('input', { type: 'password', autocomplete: 'new-password' }), err = h('div', { class: 'err' });
  const m = modal('Alterar minha senha', h('div', {}, field('Senha atual', cur), field('Nova senha (mín. 10)', n1), err,
    h('div', { style: 'margin-top:12px' }, h('button', { class: 'primary', onclick: async () => { try { await api('/api/me/password', { method: 'POST', body: { current: cur.value, password: n1.value } }); m.close(); alert('Senha alterada.'); } catch (e) { err.textContent = e.message; } } }, 'Salvar'))));
}

boot().catch(e => { $app.replaceChildren(h('div', { class: 'center' }, h('div', { class: 'card' }, h('h1', {}, 'Erro'), h('div', { class: 'err' }, e.message)))); });
