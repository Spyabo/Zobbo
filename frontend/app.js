// Zobbo frontend: minimal SPA without build tooling
// Uses same-origin REST + WebSocket to communicate with Rust backend
// Now powered by Vue + Pinia for centralized state management (no bundler)

import { createApp, markRaw } from 'vue';
import { createPinia, defineStore } from 'pinia';

const $ = (id) => document.getElementById(id);
const show = (el) => el.classList.remove('hidden');
const hide = (el) => el.classList.add('hidden');
const toastEl = () => $("toast");

function toast(msg, timeout = 2000) {
  const el = toastEl();
  el.textContent = msg;
  el.classList.remove('hidden');
  setTimeout(() => el.classList.add('hidden'), timeout);
}

function uuidFromInput(input) {
  try {
    const url = new URL(input);
    const part = url.pathname.replace(/^\//, '');
    return part.match(/^[0-9a-fA-F-]{36}$/) ? part : null;
  } catch (_) {
    const s = input.trim();
    return s.match(/^[0-9a-fA-F-]{36}$/) ? s : null;
  }
}

function apiUrl(path) {
  return `${location.origin}${path}`;
}

function wsUrl(path) {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}${path}`;
}

// Pinia store for global game state
const useGameStore = defineStore('game', {
  state: () => ({
    roomId: null,
    token: null,
    playerId: null,
    ws: null,
    mode: { kind: 'battle', rounds: 3 },
  }),
});

// Boot Vue + Pinia (we don't render a component tree yet; using Pinia for state only)
const app = createApp({ template: '<div></div>' });
const pinia = createPinia();
app.use(pinia);
const store = useGameStore();
app.mount('#app');

// Keep existing imperative UI; use Pinia store instance as our state holder
const state = store;

function setMode(kind) {
  state.mode = kind === 'sudden' ? { kind: 'sudden' } : { kind: 'battle', rounds: Number($("landing-rounds").value || 3) };
  $("mode-battle").classList.toggle('btn-primary', state.mode.kind === 'battle');
  $("mode-battle").classList.toggle('btn-secondary', state.mode.kind !== 'battle');
  $("mode-sudden").classList.toggle('btn-primary', state.mode.kind === 'sudden');
  $("mode-sudden").classList.toggle('btn-secondary', state.mode.kind !== 'sudden');
  $("rounds-wrap").classList.toggle('hidden', state.mode.kind === 'sudden');
}

async function createRoom() {
  const name = $("landing-name").value.trim();
  if (!name) return toast('Enter your name');
  const body = state.mode.kind === 'sudden'
    ? { mode: 'sudden-death' }
    : { mode: { "zobbo-battle": { rounds: Number($("landing-rounds").value || 3) } } };
  try {
    const res = await fetch(apiUrl('/api/room'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    if (!res.ok) throw new Error(await res.text());
    const data = await res.json();
    state.roomId = data.room_id;
    history.replaceState({}, '', `/${state.roomId}`);
    showJoin(state.roomId);
    $("join-name").value = name;
    toast('Room created');
  } catch (e) {
    toast(`Failed to create room: ${e.message}`);
  }
}

function showJoin(roomId) {
  hide($("landing"));
  const join = $("join");
  show(join);
  $("join-room-id").textContent = roomId;
}

async function joinRoomFromLanding() {
  const input = $("landing-room").value;
  const name = $("landing-name-join").value.trim();
  const id = uuidFromInput(input);
  if (!id) return toast('Enter a valid room link or ID');
  history.replaceState({}, '', `/${id}`);
  $("join-name").value = name;
  showJoin(id);
}

async function joinRoom() {
  const name = $("join-name").value.trim();
  if (!name) return toast('Enter your name');
  const roomId = state.roomId || $("join-room-id").textContent.trim();
  try {
    const res = await fetch(apiUrl(`/api/room/${roomId}/join`), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name }) });
    if (!res.ok) throw new Error(await res.text());
    const data = await res.json();
    state.roomId = roomId;
    state.token = data.token;
    state.playerId = data.player_id;
    await connectWs();
  } catch (e) {
    toast(`Failed to join: ${e.message}`);
  }
}

async function connectWs() {
  const url = wsUrl(`/api/room/${state.roomId}/ws?token=${encodeURIComponent(state.token)}`);
  const ws = new WebSocket(url);
  state.ws = markRaw(ws);
  ws.addEventListener('open', () => {
    toast('Connected');
    hide($("join"));
    show($("lobby"));
  });
  ws.addEventListener('message', (ev) => {
    try {
      const msg = JSON.parse(ev.data);
      onServerMessage(msg);
    } catch (e) {
      console.error('Bad message', e);
    }
  });
  ws.addEventListener('close', () => toast('Disconnected'));
  // optional ping
  setInterval(() => { try { ws.send(JSON.stringify({ type: 'ping' })); } catch (_) {} }, 15000);
}

function onServerMessage(msg) {
  switch (msg.type) {
    case 'welcome': {
      if (!state.playerId) state.playerId = msg.player_id;
      renderLobby(msg.lobby);
      break;
    }
    case 'lobby_update': {
      renderLobby(msg.lobby);
      break;
    }
    case 'game_start': {
      // Transition to game view
      hide($("lobby"));
      show($("game"));
      const yourTurn = msg.starting_player === state.playerId;
      $("turn-indicator").textContent = yourTurn ? "Your turn" : "Opponent's turn";
      toast("Game started");
      break;
    }
    case 'error': {
      toast(msg.message || 'Server error');
      break;
    }
    case 'pong': {
      break;
    }
    default:
      console.log('Unhandled', msg);
  }
}

function renderLobby(lobby) {
  state.roomId = lobby.room_id;
  hide($("landing"));
  hide($("join"));
  show($("lobby"));
  const ul = $("players");
  ul.innerHTML = '';
  let readyCount = 0;
  for (const p of lobby.players) {
    if (p.ready) readyCount++;
    const li = document.createElement('li');
    li.className = 'flex items-center justify-between rounded-lg border border-white/10 bg-white/5 px-3 py-2';
    li.innerHTML = `
      <span>${escapeHtml(p.name)}</span>
      <span class="flex items-center gap-2">
        <span class="pill"><span class="h-2 w-2 rounded-full ${p.connected ? 'bg-emerald-400' : 'bg-yellow-400'}"></span>${p.connected ? 'online' : 'offline'}</span>
        <span class="pill ${p.ready ? 'bg-emerald-500/20' : ''}">${p.ready ? 'ready' : 'not ready'}</span>
      </span>
    `;
    ul.appendChild(li);
  }
  const me = lobby.players.find(p => p.id === state.playerId);
  const btn = $("btn-ready");
  if (me?.ready) {
    btn.disabled = true;
    btn.classList.add('opacity-60','cursor-not-allowed');
    btn.textContent = 'Ready';
  } else {
    btn.disabled = false;
    btn.classList.remove('opacity-60','cursor-not-allowed');
    btn.textContent = "I'm Ready";
  }
  $("lobby-status").textContent = lobby.players.length < 2
    ? 'Waiting for opponent…'
    : (readyCount === 2 ? 'Starting…' : 'Both players can ready up');
}

function escapeHtml(s) { return s.replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;','\'':'&#39;'}[c])); }

function copyInvite() {
  const link = `${location.origin}/${state.roomId || $("join-room-id").textContent.trim()}`;
  navigator.clipboard.writeText(link).then(() => toast('Invite link copied'));
}

function readyUp() {
  try {
    state.ws?.send(JSON.stringify({ type: 'ready' }));
    const btn = $("btn-ready");
    btn.disabled = true;
    btn.classList.add('opacity-60','cursor-not-allowed');
    btn.textContent = 'Ready';
    toast('Ready!');
  } catch (_) {}
}

// Event bindings
$("mode-battle").addEventListener('click', () => setMode('battle'));
$("mode-sudden").addEventListener('click', () => setMode('sudden'));
$("landing-rounds").addEventListener('change', () => setMode('battle'));
$("btn-create").addEventListener('click', createRoom);
$("btn-join-from-landing").addEventListener('click', joinRoomFromLanding);
$("btn-join-go").addEventListener('click', joinRoomFromLanding);
$("btn-join").addEventListener('click', joinRoom);
$("btn-copy-invite").addEventListener('click', copyInvite);
$("btn-copy-invite-join").addEventListener('click', copyInvite);
$("btn-ready").addEventListener('click', readyUp);

// Initial route handling
(function init() {
  setMode('battle');
  const part = location.pathname.replace(/^\//,'');
  if (part && /^[0-9a-fA-F-]{36}$/.test(part)) {
    state.roomId = part;
    showJoin(part);
  } else {
    show($("landing"));
  }
})();
