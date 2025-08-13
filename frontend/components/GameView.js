import { useGameStore } from '../store.js';

export default {
  name: 'GameView',
  setup() {
    const store = useGameStore();
    // Placeholder until full game UI is migrated
    return { store };
  },
  template: `
    <section class="card mt-8 p-6">
      <h2 class="text-xl font-semibold">Game</h2>
      <p class="mt-2 text-white/70">Game UI migration to Vue is in progress.</p>
      <div class="mt-4 text-sm text-white/60">Room: {{ store.roomId }} | Player: {{ store.playerId }}</div>
    </section>
  `,
};
