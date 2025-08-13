import { ref, watch } from 'vue';
import { useGameStore } from '../gameStore.js';

export default {
  name: 'LandingPage',
  setup() {
    const store = useGameStore();
    const name = ref('');
    const joinInput = ref('');
    const joinName = ref('');
    const rounds = ref(store.mode.kind === 'battle' ? store.mode.rounds : 3);

    const setBattle = () => store.setMode('battle', rounds.value);
    const setSudden = () => store.setMode('sudden');
    watch(rounds, (r) => { if (store.mode.kind === 'battle') store.setMode('battle', r); });

    const create = async () => {
      try {
        await store.createRoom(name.value.trim());
        store.tempName = name.value.trim();
      } catch (e) {
        store.showToast(e.message);
      }
    };

    const joinFromLanding = async () => {
      try {
        store.setRoomFromInput(joinInput.value);
        store.tempName = joinName.value.trim();
      } catch (e) {
        store.showToast(e.message);
      }
    };

    return { store, name, joinInput, joinName, rounds, setBattle, setSudden, create, joinFromLanding };
  },
  template: `
    <section class="grid gap-4 sm:gap-6 md:grid-cols-2">
      <div class="card p-6">
        <h1 class="text-3xl font-extrabold">Play Zobbo online</h1>
        <p class="mt-2 text-white/70">Fast, competitive 1v1 card game. Create a room and share the link with a friend.</p>
        <div class="mt-6 grid gap-4">
          <div>
            <label class="label" for="landing-name">Your name</label>
            <input v-model="name" id="landing-name" class="input" placeholder="e.g. Alex" />
          </div>
          <div>
            <label class="label">Mode</label>
            <div class="mt-1 flex gap-2">
              <button @click="setBattle" :class="['btn', store.mode.kind === 'battle' ? 'btn-primary' : 'btn-secondary']" id="mode-battle">Zobbo Battle</button>
              <button @click="setSudden" :class="['btn', store.mode.kind === 'sudden' ? 'btn-primary' : 'btn-secondary']" id="mode-sudden">Sudden Death</button>
            </div>
            <div class="mt-3 flex items-center gap-3" v-show="store.mode.kind === 'battle'">
              <label class="label" for="landing-rounds">Rounds</label>
              <input v-model.number="rounds" id="landing-rounds" type="number" min="1" max="9" class="input w-24" />
            </div>
          </div>
          <div class="flex gap-3">
            <button @click="create" class="btn btn-primary" id="btn-create">Create Room</button>
            <button @click="joinFromLanding" class="btn btn-secondary" id="btn-join-from-landing">Join by Link/ID</button>
          </div>
        </div>
      </div>
      <div class="card p-6">
        <h2 class="text-xl font-semibold">Join a room</h2>
        <p class="mt-2 text-white/70">Paste a shared link or enter a room ID.</p>
        <div class="mt-4 grid gap-3">
          <input v-model="joinInput" class="input" placeholder="https://host/room-id or UUID" id="landing-room" />
          <input v-model="joinName" class="input" placeholder="Your name" id="landing-name-join" />
          <button @click="joinFromLanding" class="btn btn-primary" id="btn-join-go">Join</button>
        </div>
      </div>
    </section>
  `,
};
