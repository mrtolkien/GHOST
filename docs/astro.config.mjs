import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  integrations: [
    starlight({
      title: "GHOST",
      description: "Personal AI agent platform",
      customCss: ["./src/styles/starlight-overrides.css"],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/mrtolkien/ghost",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/mrtolkien/ghost/edit/main/docs/",
      },
      sidebar: [
        { label: "Introduction", slug: "" },
        { label: "Concepts", slug: "concepts" },
        {
          label: "Getting Started",
          items: [
            { label: "Installation", slug: "getting-started/installation" },
            {
              label: "Configuration",
              slug: "getting-started/configuration",
            },
            { label: "Workspace", slug: "getting-started/workspace" },
          ],
        },
        {
          label: "Your GHOST",
          items: [
            { label: "Identity", slug: "ghost/identity" },
            { label: "Providers", slug: "ghost/providers" },
            { label: "Interfaces", slug: "ghost/interfaces" },
          ],
        },
        {
          label: "Features",
          items: [
            { label: "Chat & Sessions", slug: "features/chat" },
            {
              label: "Knowledge",
              collapsed: false,
              items: [
                {
                  label: "Knowledge Base",
                  slug: "features/knowledge",
                },
                {
                  label: "Reflection",
                  slug: "features/reflection",
                },
                {
                  label: "Knowledge Tools",
                  slug: "features/tools-knowledge",
                },
              ],
            },
            { label: "Skills", slug: "features/skills" },
            { label: "Agents", slug: "features/agents" },
            { label: "Web Research", slug: "features/web" },
            {
              label: "Tools",
              collapsed: true,
              items: [
                { label: "Core Tools", slug: "features/tools-core" },
                { label: "Web Tools", slug: "features/tools-web" },
              ],
            },
          ],
        },
        {
          label: "Reference",
          items: [{ label: "CLI", slug: "reference/cli" }],
        },
        { label: "Roadmap", slug: "roadmap" },
      ],
    }),
  ],
});
