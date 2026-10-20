import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WINDOWS_CONFIG = resolve(ROOT, 'apps/desktop/src-tauri/tauri.windows.conf.json');
const APP_CONFIG = resolve(ROOT, 'apps/desktop/src-tauri/tauri.conf.json');

/** Validate the Windows bundle defaults from ADR-0074 and product identity from ADR-0075. */
export function windowsConfigProblems(config, appConfig) {
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
	if (bundle?.windows?.nsis?.installerHooks !== './windows/hooks.nsh') {
		// The hook removes the pre-rename factorai install (ADR-0085).
		problems.push('bundle.windows.nsis.installerHooks must be "./windows/hooks.nsh"');
	}
	if (config?.productName !== undefined && config.productName !== 'qursor-ide') {
		problems.push('Windows productName override must be "qursor-ide"');
	}
	if (config?.identifier !== undefined && config.identifier !== 'dev.factorai') {
		problems.push('Windows identifier override must remain "dev.factorai"');
	}
	if (appConfig?.productName !== 'qursor-ide') {
		problems.push('Tauri productName must be "qursor-ide"');
	}
	if (appConfig?.identifier !== 'dev.factorai') {
		problems.push('Tauri identifier must remain "dev.factorai"');
	}
	if (!appConfig?.app?.windows?.some((window) => window.title === 'Qursor IDE')) {
		problems.push('Tauri window title must be "Qursor IDE"');
	}

	return problems;
}

export async function checkWindowsConfig(configPath = WINDOWS_CONFIG, appConfigPath = APP_CONFIG) {
	const [config, appConfig] = await Promise.all([
		readFile(configPath, 'utf8').then(JSON.parse),
		readFile(appConfigPath, 'utf8').then(JSON.parse),
	]);
	const problems = windowsConfigProblems(config, appConfig);
	if (problems.length > 0) {
		throw new Error(`Invalid Windows native configuration:\n- ${problems.join('\n- ')}`);
	}
	return 'Qursor IDE Windows native NSIS defaults are valid.';
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
