import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WINDOWS_CONFIG = resolve(ROOT, 'apps/desktop/src-tauri/tauri.windows.conf.json');
const FORK_FEED = 'https://github.com/qursor-it/qursor-ide/releases/latest/download/latest.json';
const PUBKEY_PLACEHOLDER = 'REPLACE_WITH_QURSOR_UPDATER_PUBKEY';
const APP_CONFIG = resolve(ROOT, 'apps/desktop/src-tauri/tauri.conf.json');

/** Validate the Windows bundle defaults from ADR-0074 and product identity from ADR-0075 and ADR-0088. */
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
	if (
		!Array.isArray(updater?.endpoints) ||
		updater.endpoints.length !== 1 ||
		updater.endpoints[0] !== FORK_FEED
	) {
		problems.push(`plugins.updater.endpoints must be exactly [${FORK_FEED}]`);
	}
	if (
		typeof updater?.pubkey !== 'string' ||
		updater.pubkey.trim() === '' ||
		updater.pubkey === PUBKEY_PLACEHOLDER
	) {
		problems.push('plugins.updater.pubkey must be the Qursor updater public key');
	}
	const appEndpoints = appConfig?.plugins?.updater?.endpoints;
	if (!Array.isArray(appEndpoints) || appEndpoints.length !== 0) {
		// Source builds on macOS/Linux must never poll a feed (ADR-0087).
		problems.push('tauri.conf.json plugins.updater.endpoints must be an empty array');
	}
	if (bundle?.windows?.nsis?.installerHooks !== './windows/hooks.nsh') {
		// The hook removes the pre-rename factorai install (ADR-0085).
		problems.push('bundle.windows.nsis.installerHooks must be "./windows/hooks.nsh"');
	}
	if (config?.productName !== undefined && config.productName !== 'qursor-ide') {
		problems.push('Windows productName override must be "qursor-ide"');
	}
	if (config?.identifier !== undefined && config.identifier !== 'it.qursor.ide') {
		problems.push('Windows identifier override must be "it.qursor.ide"');
	}
	if (appConfig?.productName !== 'qursor-ide') {
		problems.push('Tauri productName must be "qursor-ide"');
	}
	if (appConfig?.identifier !== 'it.qursor.ide') {
		problems.push('Tauri identifier must be "it.qursor.ide"');
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
