import process from 'node:process';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// GitHub Pages serves the site from /<repo>; BASE_PATH sets it at build time.
			adapter: adapter({ fallback: '404.html' }),
			paths: {
				base: (process.argv.includes('dev') ? '' : (process.env.BASE_PATH ?? '')) as '' | `/${string}`
			}
		})
	],
	// The AudioWorklet imports the wasm-bindgen glue as an ES module.
	worker: { format: 'es' }
});
