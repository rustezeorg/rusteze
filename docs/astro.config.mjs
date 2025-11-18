// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// https://astro.build/config
export default defineConfig({
  integrations: [
    starlight({
      title: "Rusteze",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/rustezeorg/rusteze",
        },
      ],
      sidebar: [
        {
          label: "Get Started",
          slug: "get-started",
        },
        {
          label: "Components",
          autogenerate: { directory: "components" },
        },
        {
          label: "Reference",
          autogenerate: { directory: "reference" },
        },
      ],
    }),
  ],
});
