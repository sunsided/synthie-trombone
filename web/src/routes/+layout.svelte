<script lang="ts">
	import '@fontsource-variable/bricolage-grotesque';
	import favicon from '#lib/assets/favicon.svg';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<meta name="theme-color" content="#1a0d1f" />
</svelte:head>

{@render children()}

<style>
	:global(:root) {
		--throat: #1a0d1f;
		--throat-deep: #12081a;
		--mucosa: #ff4f9a;
		--breath: #5ff2ff;
		--brass: #ffc94d;
		--bone: #f3e9f7;
		--cartilage: #8c6aa6;

		color-scheme: dark;
		font-family: 'Bricolage Grotesque Variable', system-ui, sans-serif;
		font-size: 16px;
		line-height: 1.45;
		color: var(--bone);
		background: var(--throat);
	}

	:global(body) {
		margin: 0;
		min-height: 100dvh;
		background:
			radial-gradient(60rem 40rem at 30% 45%, rgb(255 79 154 / 0.1), transparent 70%),
			var(--throat);
	}

	:global(*, *::before, *::after) {
		box-sizing: border-box;
	}

	@media (prefers-reduced-motion: reduce) {
		:global(*) {
			transition: none !important;
			animation: none !important;
		}
	}
</style>
