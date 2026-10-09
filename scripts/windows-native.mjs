import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WINDOWS_CONFIG = resolve(ROOT, 'apps/desktop/src-tauri/tauri.windows.conf.json');

/** Validate the Windows bundle defaults from ADR-0074 before packaging. */
export function windowsConfigProblems(config) {
	const problems = [];
	const bundle = config?.bundle;
	const targets = bundle?.targets;
	const updater = config?.plugins?.updater;

	if (!Array.isArray(targets) || targets.length !== 1 || targets[0] !== 'nsis') {
		problems.push('bundle.targets must contain only "nsis"');
	}
	if (bundle?.createUpdaterArtifacts !== false) {
		problems.push('bundle.createUpdaterArtifacts must be false');
	}
	if (!Array.isArray(updater?.endpoints) || updater.endpoints.length !== 0) {
		problems.push('plugins.updater.endpoints must be an empty array');
	}
	if (updater?.pubkey !== '') {
		problems.push('plugins.updater.pubkey must be empty');
	}

	return problems;
}

export async function checkWindowsConfig(configPath = WINDOWS_CONFIG) {
	const config = JSON.parse(await readFile(configPath, 'utf8'));
	const problems = windowsConfigProblems(config);
	if (problems.length > 0) {
		throw new Error(`Invalid Windows native configuration:\n- ${problems.join('\n- ')}`);
	}
	return 'Windows native NSIS defaults are valid.';
}

const invokedFile = process.argv[1] ? resolve(process.argv[1]) : null;
if (invokedFile === fileURLToPath(import.meta.url)) {
	const command = process.argv[2] ?? 'check';
	if (command !== 'check') {
		console.error('Usage: node scripts/windows-native.mjs [check]');
		process.exitCode = 2;
	} else {
		try {
			console.log(await checkWindowsConfig());
		} catch (error) {
			console.error(error instanceof Error ? error.message : String(error));
			process.exitCode = 1;
		}
	}
}
