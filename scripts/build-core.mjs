// Build the qursor-core binary the app starts
// (docs/superpowers/specs/2026-11-26-local-core-design.md §1).
//
// `node scripts/build-core.mjs`: a debug build in `target/debug`, beside the
// app `tauri dev` runs, which is where the app looks for it.
// `node scripts/build-core.mjs --bundle`: a release build copied to
// `apps/desktop/src-tauri/binaries/qursor-core-<target triple>[.exe]`, the name
// Tauri's `bundle.externalBin` wants; the bundler puts it beside the app.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const cargo = (args) => execFileSync('cargo', args, { cwd: ROOT, stdio: 'inherit' });

if (!process.argv.includes('--bundle')) {
	cargo(['build', '-p', 'qursor-core', '--bin', 'qursor-core']);
} else {
	// Set by the Tauri CLI for its before-commands; the host otherwise.
	const triple =
		process.env.TAURI_ENV_TARGET_TRIPLE ??
		/^host: (.+)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))[1];
	cargo(['build', '--release', '-p', 'qursor-core', '--bin', 'qursor-core', '--target', triple]);
	const exe = triple.includes('windows') ? '.exe' : '';
	const out = resolve(ROOT, 'apps/desktop/src-tauri/binaries');
	mkdirSync(out, { recursive: true });
	copyFileSync(
		resolve(ROOT, 'target', triple, 'release', `qursor-core${exe}`),
		resolve(out, `qursor-core-${triple}${exe}`),
	);
}
