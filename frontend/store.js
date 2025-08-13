import { defineStore } from 'pinia';
import { markRaw } from 'vue';

function apiUrl(path) { return `${location.origin}${path}`; }
function wsUrl(path) {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}${path}`;
}
function uuidFromInput(input) {
  try {
    const url = new URL(input);
    const part = url.pathname.replace(/^\//, '');
    return /^[0-9a-fA-F-]{36}$/.test(part) ? part : null;
  } catch (_) {
    const s = (input || '').trim();
    return /^[0-9a-fA-F-]{36}$/.test(s) ? s : null;
  }
}

export const useGameStore = defineStore('game', {
  state: () => ({
    roomId: null,
    token: null,
    playerId: null,
    ws: null,
    mode: { kind: 'battle', rounds: 3 },
    lobby: null,
    inGame: false,
    toast: '',
    tempName: '',
  }),
  getters: {
    inviteLink(state) { return state.roomId ? `${location.origin}/${state.roomId}` : location.origin; },
  },
  actions: {
    showToast(msg, timeout = 2000) {
      this.toast = msg;
      setTimeout(() => { this.toast = ''; }, timeout);
    },
    setMode(kind, rounds = 3) {
      this.mode = kind === 'sudden' ? { kind: 'sudden' } : { kind: 'battle', rounds: Number(rounds || 3) };
    },
    setRoomFromPath() {
      const part = location.pathname.replace(/^\//,'');
      if (part && /^[0-9a-fA-F-]{36}$/.test(part)) this.roomId = part;
    },
    setRoomFromInput(input) {
      const id = uuidFromInput(input);
      if (!id) throw new Error('Enter a valid room link or ID');
      this.roomId = id;
      history.replaceState({}, '', `/${id}`);
    },
    async createRoom(playerName) {
      if (!playerName) throw new Error('Enter your name');
      const body = this.mode.kind === 'sudden' ? { mode: 'sudden-death' } : { mode: { 'zobbo-battle': { rounds: this.mode.rounds } } };
      const res = await fetch(apiUrl('/api/room'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      this.roomId = data.room_id;
      history.replaceState({}, '', `/${this.roomId}`);
      this.showToast('Room created');
      return data;
    },
    async joinRoom(playerName) {
      if (!playerName) throw new Error('Enter your name');
      if (!this.roomId) throw new Error('Missing room');
      const res = await fetch(apiUrl(`/api/room/${this.roomId}/join`), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: playerName }) });
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      this.token = data.token;
      this.playerId = data.player_id;
      await this.connectWs();
    },
    async connectWs() {
      const url = wsUrl(`/api/room/${this.roomId}/ws?token=${encodeURIComponent(this.token)}`);
      const ws = new WebSocket(url);
      this.ws = markRaw(ws);
      ws.addEventListener('open', () => {
        this.showToast('Connected');
      });
      ws.addEventListener('message', (ev) => {
        try {
          const msg = JSON.parse(ev.data);
          this.onServerMessage(msg);
        } catch (e) {
          console.error('Bad message', e);
        }
      });
      ws.addEventListener('close', () => this.showToast('Disconnected'));
      setInterval(() => { try { ws.send(JSON.stringify({ type: 'ping' })); } catch (_) {} }, 15000);
    },
    onServerMessage(msg) {
      switch (msg.type) {
        case 'welcome': {
          if (!this.playerId) this.playerId = msg.player_id;
          this.lobby = msg.lobby;
          break;
        }
        case 'lobby_update': {
          this.lobby = msg.lobby;
          break;
        }
        case 'game_start': {
          this.inGame = true;
          this.lobby = null;
          this.showToast('Game started');
          break;
        }
        case 'error': {
          this.showToast(msg.message || 'Server error');
          break;
        }
        case 'pong': {
          break;
        }
        default:
          console.log('Unhandled', msg);
      }
    },
    readyUp() {
      try { this.ws?.send(JSON.stringify({ type: 'ready' })); this.showToast('Ready!'); } catch (_) {}
    },
  },
});
