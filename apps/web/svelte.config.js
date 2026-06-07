// ADR-014 §A — adapter-static so the build output is pre-rendered HTML +
// JS that the Rust API binary embeds via rust-embed (ADR-002 §H).
// `strict: true` makes the build fail if any route can't be prerendered;
// that is the contract this codebase commits to (dynamic routes land
// alongside the apps/admin-web staff plane, not here).

import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      pages: 'build',
      assets: 'build',
      fallback: undefined,
      precompress: false,
      strict: true,
    }),
  },
};

export default config;
