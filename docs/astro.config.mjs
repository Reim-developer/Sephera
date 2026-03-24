import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: "https://sephera.vercel.app",
	integrations: [
		starlight({
			title: 'Sephera Docs',
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
						{ slug: 'commands/context' },
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
					items: [{ slug: 'architecture/overview' }],
				},
			],
			customCss: ['./src/styles/custom.css'],
		}),
	],
});
