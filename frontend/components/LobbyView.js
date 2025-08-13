import { computed } from 'vue';
import { useGameStore } from '../store.js';

export default {
  name: 'LobbyView',
  setup() {
    const store = useGameStore();
    const players = computed(() => store.lobby?.players || []);
    const readyCount = computed(() => players.value.filter(p => p.ready).length);
    const me = computed(() => players.value.find(p => p.id === store.playerId));

    const copyInvite = async () => {
      try { await navigator.clipboard.writeText(store.inviteLink); store.showToast('Invite link copied'); } catch (_) {}
    };

    const ready = () => store.readyUp();

    const statusText = computed(() => {
      const len = players.value.length;
      if (len < 2) return 'Waiting for opponent…';
      return readyCount.value === 2 ? 'Starting…' : 'Both players can ready up';
    });

    return { store, players, me, readyCount, statusText, copyInvite, ready };
  },
  template: `
    <section class="card mt-6 sm:mt-8 p-6">
      <div class="flex flex-wrap items-center justify-between gap-4">
        <div class="flex items-center gap-3">
          <span class="pill"><span class="h-2 w-2 rounded-full bg-emerald-400"></span> Room ready</span>
          <button @click="copyInvite" class="btn btn-secondary">Copy Invite Link</button>
        </div>
        <div class="text-sm text-white/70">Share the link and wait for your friend to join.</div>
      </div>
      <div class="mt-6 grid gap-4 md:grid-cols-2">
        <div class="rounded-lg border border-white/10 p-4">
          <div class="label">Players</div>
          <ul class="mt-2 space-y-2">
            <li v-for="p in players" :key="p.id" class="flex items-center justify-between rounded-lg border border-white/10 bg-white/5 px-3 py-2">
              <span>{{ p.name }}</span>
              <span class="flex items-center gap-2">
                <span class="pill"><span :class="['h-2 w-2 rounded-full', p.connected ? 'bg-emerald-400' : 'bg-yellow-400']"></span>{{ p.connected ? 'online' : 'offline' }}</span>
                <span :class="['pill', p.ready ? 'bg-emerald-500/20' : '']">{{ p.ready ? 'ready' : 'not ready' }}</span>
              </span>
            </li>
          </ul>
        </div>
        <div class="rounded-lg border border-white/10 p-4">
          <div class="label">Status</div>
          <div class="mt-2 text-white/80">{{ statusText }}</div>
          <div class="mt-4">
            <button @click="ready" :disabled="me && me.ready" :class="['btn btn-primary', me && me.ready ? 'opacity-60 cursor-not-allowed' : '']">{{ me && me.ready ? 'Ready' : "I'm Ready" }}</button>
          </div>
        </div>
      </div>
    </section>
  `,
};
