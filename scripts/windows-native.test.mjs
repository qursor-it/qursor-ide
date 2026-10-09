import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { windowsConfigProblems } from './windows-native.mjs';

const windowsConfig = JSON.parse(
	await readFile(
		new URL('../apps/desktop/src-tauri/tauri.windows.conf.json', import.meta.url),
		'utf8',
	),
);

test('Windows native configuration creates only an NSIS bundle without updater artifacts', () => {
	assert.deepEqual(windowsConfigProblems(windowsConfig), []);
});

test('Windows native configuration fails closed if an upstream updater feed is restored', () => {
	const config = structuredClone(windowsConfig);
	config.plugins.updater.endpoints.push('https://example.invalid/latest.json');
	config.plugins.updater.pubkey = 'unexpected-key';

	assert.deepEqual(windowsConfigProblems(config), [
		'plugins.updater.endpoints must be an empty array',
		'plugins.updater.pubkey must be empty',
	]);
});

test('Windows native configuration fails closed if a non-NSIS target or updater artifacts are enabled', () => {
	const config = structuredClone(windowsConfig);
	config.bundle.targets = ['nsis', 'msi'];
	config.bundle.createUpdaterArtifacts = true;

	assert.deepEqual(windowsConfigProblems(config), [
		'bundle.targets must contain only "nsis"',
		'bundle.createUpdaterArtifacts must be false',
	]);
});
