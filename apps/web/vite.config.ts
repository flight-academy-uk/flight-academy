// Tailwind v4 wires via @tailwindcss/vite (no tailwind.config.js needed
// for the v4 model — theme customisation goes inline via @theme in CSS).
// Plugin order matters: tailwindcss before sveltekit so Tailwind sees
// raw .svelte files at the right pipeline stage.

import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
});
