import { computed, onMounted } from 'vue';
import { useGameStore } from '../gameStore.js';
import LandingPage from './LandingPage.js';
import JoinRoomPage from './JoinRoomPage.js';
import LobbyPage from './LobbyPage.js';
import GamePage from './GamePage.js';

export default {
  name: 'App',
  components: { LandingPage, JoinRoomPage, LobbyPage, GamePage },
  setup() {
    const store = useGameStore();

    onMounted(() => {
      // Read room id from URL path
      const part = location.pathname.replace(/^\//,'');
      if (part && /^[0-9a-fA-F-]{36}$/.test(part)) {
        store.roomId = part;
      }
    });

    const currentView = computed(() => {
      if (!store.roomId) return LandingPage;
      if (!store.token) return JoinRoomPage;
      if (store.inGame) return GamePage;
      return LobbyPage;
    });

    return { store, currentView };
  },
  template: `
  <div>
    <header class="sticky top-0 z-10 border-b border-white/10 bg-black/30 backdrop-blur">
      <div class="mx-auto flex max-w-6xl items-center justify-between px-4 py-3">
        <a class="flex items-center gap-3" href="/">
          <img src="/favicon.svg" alt="Z" class="h-7 w-7" />
          <span class="text-lg font-bold tracking-tight">Zobbo</span>
        </a>
        <nav class="flex items-center gap-2 text-sm text-white/70">
          <a href="#" class="hover:text-white hidden">Rules</a>
          <a href="#" class="hover:text-white hidden">About</a>
        </nav>
      </div>
    </header>

    <main class="mx-auto max-w-4xl px-3 sm:px-4 py-6 sm:py-10">
      <component :is="currentView" />
      <div v-if="store.toast" class="fixed bottom-4 right-4 rounded-lg bg-black/80 px-4 py-2 text-sm shadow-lg">{{ store.toast }}</div>
    </main>
  </div>
  `,
};
