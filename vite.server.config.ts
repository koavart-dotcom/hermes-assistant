import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Web panel of the server mode (hermesd, docs/SERVER.md): a separate page built
// into dist-server/ and shipped as resources/server-ui/. Served by hermesd at /ui/.
// `pnpm dev:server` proxies the control API of a hermesd on this machine.
export default defineConfig({
  root: fileURLToPath(new URL("./src/server", import.meta.url)),
  base: "/ui/",
  publicDir: false,
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  build: {
    outDir: fileURLToPath(new URL("./dist-server", import.meta.url)),
    emptyOutDir: true,
  },
  server: {
    port: 1430,
    strictPort: true,
    proxy: { "/control": "http://127.0.0.1:8645" },
  },
});
