import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Every colour in the CSS (app and components) becomes `var(--c-<hex>, #<hex>)`, so a colour
// theme (src/lib/theme.ts) can restyle the whole app; without one, the fallback is the design.
const hexColour = /#([0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![0-9a-fA-F-_])/g;
const themeColours: Plugin = {
  name: 'chitchat-theme-colours',
  enforce: 'pre',
  transform(code, id) {
    if (!/\.css($|\?)|[?&]lang\.css/.test(id)) return;
    // Only in declaration values (after a colon, before ; or }), never in selectors like #app.
    return code.replace(/(:\s*)([^;{}]*?)(?=[;}])/g, (all, colon: string, value: string) =>
      value.includes('var(--c-') ? all : colon + value.replace(hexColour, (hex, digits: string) => `var(--c-${digits.toLowerCase()}, ${hex})`),
    );
  },
};

export default defineConfig({
  plugins: [themeColours, svelte()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:8080' },
      '^/s/[^/]+/api(?:/|$)': { target: 'http://127.0.0.1:8080', ws: true },
    },
  },
  build: { outDir: 'dist', emptyOutDir: true },
});
