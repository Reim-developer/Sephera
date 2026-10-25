import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: "https://sephera.vercel.app",
	integrations: [
		starlight({
			title: 'Sephera Docs',
			disable404Route: true,
			description:
				'Documentation for Sephera, a Rust tool focused on fast LOC analysis and deterministic LLM-ready context packs.',
			sidebar: [
				{
					label: 'Start Here',
					items: [
						{ label: 'Overview', link: '/' },
						{ slug: 'getting-started' },
					],
				},
				{
					label: 'Commands',
					items: [
						{ slug: 'commands/loc' },
						{ slug: 'commands/symbols' },
						{ slug: 'commands/context' },
						{ slug: 'commands/graph' },
						{ slug: 'commands/impact' },
						{ slug: 'commands/affected-tests' },
						{ slug: 'commands/why' },
						{ slug: 'commands/coupling' },
						{ slug: 'commands/review' },
						{ slug: 'commands/risk' },
						{ slug: 'commands/dead' },
						{ slug: 'commands/breaking' },
						{ slug: 'commands/untested' },
						{ slug: 'commands/merge' },
						{ slug: 'commands/guard' },
						{ slug: 'commands/watch' },
						{ slug: 'commands/mcp' },
						{ slug: 'commands/skill' },
					],
				},
				{
					label: 'Configuration',
					items: [{ slug: 'configuration/sephera-toml' }],
				},
				{
					label: 'Benchmarks',
					items: [{ slug: 'benchmarks' }],
				},
				{
					label: 'Architecture',
					items: [
						{ slug: 'architecture/overview' },
						{ slug: 'architecture/ffi-boundaries' },
						{ slug: 'architecture/releasing' },
					],
				},
			],
			customCss: ['./src/styles/custom.css'],
		}),
	],
});
