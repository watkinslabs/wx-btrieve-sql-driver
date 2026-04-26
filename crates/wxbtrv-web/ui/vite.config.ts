import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "src") },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
  server: {
    // `npm run dev` proxies /api/* to a locally running wxbtrv-web binary
    // so the React side can hot-reload while talking to the real server.
    proxy: {
      "/api": "http://127.0.0.1:18900",
    },
  },
});
