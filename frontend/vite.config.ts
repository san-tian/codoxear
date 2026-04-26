import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  base: "/nova-preview/",
  server: {
    host: "127.0.0.1",
    port: 5173,
  },
});
