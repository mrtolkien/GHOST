import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  integrations: [
    starlight({
      title: "GHOST",
      favicon: "/favicon.svg",
      description: "Personal AI agent platform",
      components: {
        SiteTitle: "./src/components/SiteTitle.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
      },
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
        { label: "Project Status", slug: "disclaimer" },
        {
          label: "Getting Started",
          items: [
            { label: "Installation", slug: "getting-started/installation" },
            { label: "Onboarding", slug: "getting-started/onboarding" },
            { label: "Services", slug: "getting-started/services" },
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
          label: "Chat",
          items: [
            { label: "Sessions & Messages", slug: "chat/sessions" },
            { label: "Compaction", slug: "chat/compaction" },
          ],
        },
        {
          label: "GHOST HACK",
          items: [
            { label: "Overview", slug: "ghost-hack/overview" },
            {
              label: "Project Context",
              slug: "ghost-hack/project-context",
            },
            { label: "Configuration", slug: "ghost-hack/configuration" },
          ],
        },
        {
          label: "Knowledge",
          items: [
            { label: "Knowledge Base", slug: "knowledge/knowledge" },
            {
              label: "Reference Import",
              slug: "knowledge/reference-import",
            },
            { label: "Reflection", slug: "knowledge/reflection" },
            { label: "Knowledge Tools", slug: "knowledge/tools" },
          ],
        },
        {
          label: "Skills & Tools",
          items: [
            { label: "Overview", slug: "skills-and-tools/overview" },
            {
              label: "Default Skills",
              slug: "skills-and-tools/default-skills",
            },
            {
              label: "Creating Skills",
              slug: "skills-and-tools/creating-skills",
            },
            { label: "Core Tools", slug: "skills-and-tools/core-tools" },
          ],
        },
        {
          label: "Web Research",
          items: [
            { label: "Overview", slug: "web-research/overview" },
            { label: "Web Tools", slug: "web-research/tools" },
          ],
        },
        {
          label: "Agents",
          items: [
            { label: "Introduction", slug: "agents/introduction" },
            { label: "Syntax Reference", slug: "agents/syntax" },
            { label: "Context", slug: "agents/context" },
            { label: "Agent Control", slug: "agents/agent-control" },
            { label: "Cron Jobs", slug: "agents/cron" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI", slug: "reference/cli" },
            {
              label: "External Dependencies",
              slug: "reference/dependencies",
            },
          ],
        },
        { label: "Roadmap", slug: "roadmap" },
      ],
    }),
  ],
});
