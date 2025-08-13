import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './components/App.js';

const app = createApp(App);
app.use(createPinia());
app.mount('#app');
