import { sveltekit } from '@sveltejs/kit/vite';
import { execSync } from 'child_process';
import { defineConfig } from 'vite';

function gitCommit(): string {
	try {
		return execSync('git rev-parse --short=8 HEAD').toString().trim();
	} catch {
		return process.env.BUILD_COMMIT || 'unknown';
	}
}

export default defineConfig({
	plugins: [sveltekit()],
	define: {
		__APP_VERSION__: JSON.stringify(process.env.npm_package_version || '0.2.0'),
		__BUILD_COMMIT__: JSON.stringify(process.env.BUILD_COMMIT || gitCommit()),
	},
	server: {
		port: 5174,
	}
});
