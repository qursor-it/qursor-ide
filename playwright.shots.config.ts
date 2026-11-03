import { defineConfig, devices } from '@playwright/test';

const PORT = process.env.PLAYWRIGHT_PORT ?? '1440';

/**
 * The guide's and the README's illustrations (ADR-0066, ADR-0071), not a test
 * suite: `pnpm docs:shots` drives the renderer against the mock bridge and
 * writes the images into `assets/images/guide/` and `assets/images/`. Kept out of `pnpm e2e` — a picture is reviewed by
 * looking at it, and re-shooting belongs to whoever changed the surface.
 *
 * Its own port, so it never reuses a `pnpm dev` or `pnpm e2e` server, and
 * device scale 2 so the app's 12px type survives being displayed at half size
 * (see `apps/docs/src/components/Shot`).
 */
export default defineConfig({
	testDir: './tests/docs-shots',
	testMatch: '**/*.shots.ts',
	timeout: 120_000,
	expect: { timeout: 10_000 },
	fullyParallel: false,
	retries: 0,
	workers: 1,
	reporter: 'list',
	use: {
		baseURL: `http://localhost:${PORT}`,
		colorScheme: 'dark',
	},
	projects: [
		{
			name: 'chromium',
			use: {
				...devices['Desktop Chrome'],
				viewport: { width: 1440, height: 900 },
				deviceScaleFactor: 2,
				// Desktop Chrome's UA says Windows, which switches the terminal to
				// WebGL, and a headless WebGL canvas captures blank.
				userAgent:
					'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
			},
		},
	],
	webServer: {
		command: `pnpm --filter @factorai/desktop vite:dev --port ${PORT} --strictPort`,
		// Through `env`, not a `VAR=1 cmd` prefix, so it also runs under cmd.exe.
		env: { VITE_FACTORAI_SCREENSHOT: '1' },
		url: `http://localhost:${PORT}`,
		reuseExistingServer: false,
		timeout: 60_000,
		stdout: 'ignore',
		stderr: 'pipe',
	},
});
