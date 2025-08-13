import { ref, onMounted } from 'vue';
import { useGameStore } from '../gameStore.js';

export default {
  name: 'JoinRoomPage',
  setup() {
    const store = useGameStore();
    const name = ref('');

    onMounted(() => {
      if (store.tempName) name.value = store.tempName;
    });

    const join = async () => {
      try {
        await store.joinRoom(name.value.trim());
      } catch (e) {
        store.showToast(e.message);
      }
    };

    const copyInvite = async () => {
      try {
        await navigator.clipboard.writeText(store.inviteLink);
        store.showToast('Invite link copied');
      } catch (_) {}
    };

    return { store, name, join, copyInvite };
  },
  template: `
    <section class="card mt-8 p-6">
      <div class="flex flex-wrap items-center justify-between gap-4">
        <div>
          <div class="label">Room</div>
          <div class="font-mono text-sm">{{ store.roomId }}</div>
        </div>
        <div>
          <button @click="copyInvite" class="btn btn-secondary">Copy Invite Link</button>
        </div>
      </div>
      <div class="mt-6 grid gap-3 md:grid-cols-[1fr_auto]">
        <input v-model="name" class="input" placeholder="Your name" />
        <button @click="join" class="btn btn-primary">Join Room</button>
      </div>
    </section>
  `,
};
